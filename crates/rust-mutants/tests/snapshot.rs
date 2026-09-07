// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The snapshot: a disposable, byte-exact copy of a source tree at a stable name, with a manifest, a frozen workspace digest, drift detection, and a guarded cleanup.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use jiff::Timestamp;
use rust_mutants::glob::Pattern;
use rust_mutants::snapshot::{
    CLEANUP_ATTEMPTS, CLEANUP_BACKOFF, DEFAULT_REPORT_DIR, DIR_PREFIX, Drift, Entry, Options,
    STABLE_NAME_HEX_LENGTH, SnapshotErrorKind, TREE_NAME, WORKSPACE_DOMAIN, cleanup_guard, create,
    stable_name, workspace_digest,
};
use rust_mutants::tempowner::{self, read_marker};
use sha2::{Digest as _, Sha256};

fn now() -> Timestamp {
    Timestamp::now()
}

fn write(root: &Path, rel: &str, bytes: &[u8]) -> PathBuf {
    let path = root.join(rel);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("mkdir");
    }
    fs::write(&path, bytes).expect("write");
    path
}

fn sha256_hex(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// A source tree and a destination parent that outlive the snapshot.
struct Fixture {
    _temp: tempfile::TempDir,
    source: PathBuf,
    dest: PathBuf,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().expect("tempdir");
    let source = temp.path().join("project");
    let dest = temp.path().join("dest");
    fs::create_dir_all(&source).expect("mkdir source");
    fs::create_dir_all(&dest).expect("mkdir dest");
    write(&source, "Cargo.toml", b"[package]\nname = \"x\"\n");
    write(&source, "src/main.rs", b"fn main() {}\n");
    Fixture {
        _temp: temp,
        source,
        dest,
    }
}

fn options(fx: &Fixture) -> Options {
    Options::new(&fx.dest)
}

/// Every manifest entry names a byte-identical copy with the right size and digest.
fn assert_manifest_matches(source: &Path, snap: &rust_mutants::snapshot::Snapshot) {
    for entry in snap.manifest() {
        let src = fs::read(source.join(&entry.rel_path)).expect("read source");
        let copy = fs::read(snap.root().join(&entry.rel_path)).expect("read copy");
        assert_eq!(src, copy, "{}", entry.rel_path);
        assert_eq!(entry.size, src.len() as u64);
        assert_eq!(entry.sha256, sha256_hex(&src));
    }
}

fn snapshot_dirs(dest: &Path) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = fs::read_dir(dest)
        .expect("read dest")
        .map(|entry| entry.expect("entry").path())
        .collect();
    dirs.sort();
    dirs
}

#[test]
fn the_constants_are_frozen() {
    assert_eq!(WORKSPACE_DOMAIN, "rust-mutants-workspace-v1");
    assert_eq!(DIR_PREFIX, "rust-mutants-snap-");
    assert_eq!(TREE_NAME, "tree");
    assert_eq!(DEFAULT_REPORT_DIR, "reports/mutation");
    assert_eq!(STABLE_NAME_HEX_LENGTH, 16);
    assert_eq!(CLEANUP_ATTEMPTS, 5);
    assert_eq!(CLEANUP_BACKOFF, Duration::from_millis(20));
}

#[test]
fn the_workspace_digest_matches_the_independently_minted_vectors() {
    assert_eq!(
        workspace_digest(&[]),
        "13ff85f1943353b06f3d51ff611a31542f667b429529586787b2c517b93fe27f"
    );
    let main = Entry {
        rel_path: "src/main.rs".to_owned(),
        size: 13,
        sha256: "536e506bb90914c243a12b397b9a998f85ae2cbd9ba02dfd03a9e155ca5ca0f4".to_owned(),
    };
    let manifest = Entry {
        rel_path: "Cargo.toml".to_owned(),
        size: 22,
        sha256: "ae630b460f5f29f1d88c13c045b81c3920eb22c008fa1e9c46e6768993a2d92a".to_owned(),
    };
    assert_eq!(
        workspace_digest(&[manifest.clone(), main.clone()]),
        "c4025b0efb8471560c6870f50e6a61da20c06037f117b27be81163230c2e56e5"
    );
    assert_eq!(
        workspace_digest(&[main, manifest]),
        "75fa0f6b0c5e9ffe505a090ebe0dcafe10335946d62dde0daccbaf33e6d6ed5f"
    );
}

#[test]
fn the_workspace_digest_ignores_sizes_because_the_content_hash_already_pins_them() {
    let entry = |size: u64| Entry {
        rel_path: "a.rs".to_owned(),
        size,
        sha256: sha256_hex(b"x"),
    };
    assert_eq!(workspace_digest(&[entry(1)]), workspace_digest(&[entry(2)]));
}

#[test]
fn the_stable_name_is_the_prefix_plus_sixteen_hex_of_the_path_digest() {
    assert_eq!(
        stable_name(Path::new("/home/alice/project")),
        "rust-mutants-snap-9c2098df26004b24"
    );
    assert_eq!(
        stable_name(Path::new("/home/alice/project/")),
        "rust-mutants-snap-f83c1dd91efbeafc"
    );
    assert_eq!(
        stable_name(Path::new("/x")).len(),
        DIR_PREFIX.len() + STABLE_NAME_HEX_LENGTH
    );
}

#[test]
fn create_copies_the_tree_byte_for_byte_and_records_a_sorted_manifest() {
    let fx = fixture();
    write(&fx.source, "src/lib.rs", b"pub fn f() {}\r\n");
    write(&fx.source, "tests/data/golden.bin", &[0, 255, 10, 13, 0]);
    fs::create_dir_all(fx.source.join("empty/dir")).expect("empty dirs");

    let snap = create(&fx.source, &options(&fx), now()).expect("create");

    assert_eq!(snap.source_root(), fx.source);
    assert_eq!(snap.dir().parent().unwrap(), fx.dest);
    assert_eq!(snap.parent(), fx.dest);
    assert_eq!(snap.root(), snap.dir().join(TREE_NAME));
    assert!(snap.stable_dir());
    assert_eq!(
        snap.dir().file_name().unwrap().to_str().unwrap(),
        stable_name(&fx.source)
    );

    let rel: Vec<&str> = snap
        .manifest()
        .iter()
        .map(|e| e.rel_path.as_str())
        .collect();
    assert_eq!(
        rel,
        [
            "Cargo.toml",
            "src/lib.rs",
            "src/main.rs",
            "tests/data/golden.bin"
        ]
    );
    assert_manifest_matches(&fx.source, &snap);
    assert!(
        snap.root().join("empty/dir").is_dir(),
        "empty directories are recreated"
    );
    assert_eq!(snap.workspace_digest(), workspace_digest(snap.manifest()));

    assert!(snap.dir().join(tempowner::LOCK_NAME).exists());
    assert!(snap.dir().join(tempowner::MARKER_NAME).exists());
    assert!(!snap.root().join(tempowner::MARKER_NAME).exists());
    let marker = read_marker(snap.dir()).expect("marker");
    assert_eq!(marker.pid, std::process::id());
    assert!(!marker.kept);
}

#[test]
fn create_is_deterministic_across_runs() {
    let fx = fixture();
    let first = create(&fx.source, &options(&fx), now()).expect("first");
    let digest = first.workspace_digest().to_owned();
    let manifest = first.manifest().to_vec();
    first.cleanup().expect("cleanup");
    let second = create(&fx.source, &options(&fx), now()).expect("second");
    assert_eq!(second.workspace_digest(), digest);
    assert_eq!(second.manifest(), manifest.as_slice());
    assert!(second.stable_dir(), "the stable name is free again");
}

#[cfg(unix)]
#[test]
fn create_preserves_file_permissions_and_makes_directories_writable() {
    use std::os::unix::fs::PermissionsExt as _;
    let fx = fixture();
    let script = write(&fx.source, "scripts/run.sh", b"#!/bin/sh\n");
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("chmod");
    let readonly = write(&fx.source, "src/frozen.rs", b"// frozen\n");
    fs::set_permissions(&readonly, fs::Permissions::from_mode(0o444)).expect("chmod");
    let locked_dir = fx.source.join("locked");
    fs::create_dir_all(&locked_dir).expect("mkdir");
    write(&locked_dir, "inside.rs", b"// inside\n");
    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o555)).expect("chmod");

    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    let mode = |rel: &str| {
        fs::metadata(snap.root().join(rel))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode("scripts/run.sh"), 0o755);
    assert_eq!(mode("src/frozen.rs"), 0o444);
    assert_eq!(
        mode("locked"),
        0o755,
        "the copy has to be writable by its owner"
    );

    fs::set_permissions(&locked_dir, fs::Permissions::from_mode(0o755)).expect("restore");
    snap.cleanup().expect("cleanup");
}

#[test]
fn a_relative_or_missing_source_root_is_refused_before_anything_is_created() {
    let fx = fixture();
    let missing = create(&fx.source.join("nope"), &options(&fx), now()).unwrap_err();
    assert_eq!(missing.kind(), SnapshotErrorKind::SourceRoot);
    let file = create(&fx.source.join("Cargo.toml"), &options(&fx), now()).unwrap_err();
    assert_eq!(file.kind(), SnapshotErrorKind::SourceRoot);
    let relative = create(Path::new("relative/root"), &options(&fx), now()).unwrap_err();
    assert_eq!(relative.kind(), SnapshotErrorKind::SourceRoot);
    assert!(relative.to_string().contains("RM1002"), "{relative}");
    assert!(snapshot_dirs(&fx.dest).is_empty(), "nothing was created");
}

#[test]
fn git_and_the_report_directories_are_always_excluded_and_patterns_skip_whole_directories() {
    let fx = fixture();
    write(&fx.source, ".git/config", b"[core]\n");
    write(&fx.source, "vendor/.git/HEAD", b"ref\n");
    write(&fx.source, "reports/mutation/run.json", b"{}");
    write(&fx.source, "out/custom/report.html", b"<html>");
    write(&fx.source, "target/debug/deps/x.d", b"x");
    write(&fx.source, "vendor/dep/lib.rs", b"// vendored\n");
    #[cfg(unix)]
    std::os::unix::fs::symlink("/nowhere", fx.source.join("target/link")).expect("symlink");

    let mut opts = options(&fx);
    opts.report_dir = Some("out/custom".to_owned());
    opts.exclude = vec![
        Pattern::compile("vendor/**").expect("pattern"),
        Pattern::compile("target").expect("pattern"),
    ];
    let snap = create(&fx.source, &opts, now()).expect("create");
    let rel: Vec<&str> = snap
        .manifest()
        .iter()
        .map(|e| e.rel_path.as_str())
        .collect();
    assert_eq!(rel, ["Cargo.toml", "src/main.rs"]);
    assert!(
        !snap.root().join("target").exists(),
        "an excluded directory is not descended"
    );
    assert!(snap.root().join("reports").is_dir());
    assert!(!snap.root().join("reports/mutation").exists());
    assert!(!snap.root().join("out/custom").exists());
}

#[test]
fn a_configured_report_directory_that_escapes_the_root_is_an_invalid_option() {
    let fx = fixture();
    for bad in ["../elsewhere", "/abs/reports", ""] {
        let mut opts = options(&fx);
        opts.report_dir = Some(bad.to_owned());
        let error = create(&fx.source, &opts, now()).unwrap_err();
        assert_eq!(error.kind(), SnapshotErrorKind::InvalidOptions, "{bad:?}");
        assert_eq!(error.path(), bad);
    }
    assert!(snapshot_dirs(&fx.dest).is_empty());
}

#[cfg(unix)]
#[test]
fn symbolic_links_are_recorded_and_not_followed() {
    let fx = fixture();
    std::os::unix::fs::symlink("main.rs", fx.source.join("src/zz-link.rs")).expect("symlink");
    std::os::unix::fs::symlink("..", fx.source.join("src/aa-up")).expect("symlink");

    let snapshot = create(&fx.source, &options(&fx), now()).expect("a tree with links in it");

    let passed: Vec<(&str, &str)> = snapshot
        .passed_over()
        .iter()
        .map(|one| (one.rel_path.as_str(), one.kind.name()))
        .collect();
    assert_eq!(
        passed,
        [
            ("src/aa-up", "symbolic-link"),
            ("src/zz-link.rs", "symbolic-link")
        ],
        "following a link can leave the tree; refusing to measure a project that holds one \
         refuses to measure a project the compiler is perfectly happy with"
    );
    assert!(
        !snapshot.root().join("src/zz-link.rs").exists(),
        "and it is not copied"
    );

    let targets: Vec<Option<&str>> = snapshot
        .passed_over()
        .iter()
        .map(|one| one.target.as_deref())
        .collect();
    assert_eq!(
        targets,
        [Some(".."), Some("main.rs")],
        "what the link points at is in the digest, so two trees differing only in a link \
         are two trees"
    );
}

#[cfg(unix)]
#[test]
fn irregular_files_are_recorded_and_not_copied() {
    let fx = fixture();
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        fx.source.join("src/pipe"),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .expect("fifo");

    let snapshot = create(&fx.source, &options(&fx), now()).expect("a tree with a pipe in it");

    let passed: Vec<(&str, &str, Option<&str>)> = snapshot
        .passed_over()
        .iter()
        .map(|one| {
            (
                one.rel_path.as_str(),
                one.kind.name(),
                one.target.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        passed,
        [("src/pipe", "irregular-file", Some("a named pipe"))]
    );
}

#[cfg(unix)]
#[test]
fn a_name_with_a_backslash_is_refused_because_it_cannot_round_trip_through_a_relative_path() {
    let fx = fixture();
    write(&fx.source, "src/a\\b.rs", b"//\n");
    let error = create(&fx.source, &options(&fx), now()).unwrap_err();
    assert_eq!(error.kind(), SnapshotErrorKind::UnsupportedName);
    assert_eq!(error.path(), "src/a\\b.rs");
}

#[cfg(unix)]
#[test]
fn a_file_that_cannot_be_read_fails_the_copy_and_removes_the_partial_snapshot() {
    use std::os::unix::fs::PermissionsExt as _;
    if rustix::process::geteuid().is_root() {
        return; // root reads everything
    }
    let fx = fixture();
    let secret = write(&fx.source, "src/secret.rs", b"//\n");
    fs::set_permissions(&secret, fs::Permissions::from_mode(0o000)).expect("chmod");
    let error = create(&fx.source, &options(&fx), now()).unwrap_err();
    assert_eq!(error.kind(), SnapshotErrorKind::Copy);
    assert_eq!(error.path(), "src/secret.rs");
    assert!(error.source().is_some(), "the OS error is kept");
    assert!(
        snapshot_dirs(&fx.dest).is_empty(),
        "the partial copy is removed"
    );
}

#[test]
fn a_second_live_snapshot_of_the_same_root_falls_back_to_a_random_name() {
    let fx = fixture();
    let first = create(&fx.source, &options(&fx), now()).expect("first");
    let second = create(&fx.source, &options(&fx), now()).expect("second");
    assert!(first.stable_dir());
    assert!(!second.stable_dir());
    assert_ne!(first.dir(), second.dir());
    let name = second
        .dir()
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    assert!(name.starts_with(DIR_PREFIX), "{name}");
    assert_eq!(second.workspace_digest(), first.workspace_digest());
    assert_eq!(snapshot_dirs(&fx.dest).len(), 2);
    second.cleanup().expect("cleanup second");
    first.cleanup().expect("cleanup first");
    assert!(snapshot_dirs(&fx.dest).is_empty());
}

#[test]
fn an_abandoned_stable_directory_is_swept_and_the_name_reused_never_adopted() {
    let fx = fixture();
    let dir = fx.dest.join(stable_name(&fx.source));
    fs::create_dir_all(dir.join(TREE_NAME).join("src")).expect("mkdir");
    write(
        &dir.join(TREE_NAME),
        "src/leftover.rs",
        b"// half-instrumented\n",
    );
    let mut owner = tempowner::claim(&dir, now()).expect("claim");
    owner.release().expect("release"); // the previous run is gone
    drop(owner);

    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    assert!(snap.stable_dir());
    assert_eq!(snap.dir(), dir);
    assert!(
        !snap.root().join("src/leftover.rs").exists(),
        "the leftover tree was swept"
    );
    assert!(snap.root().join("src/main.rs").exists());
}

#[test]
fn a_kept_stable_directory_is_not_reused() {
    let fx = fixture();
    let mut first = create(&fx.source, &options(&fx), now()).expect("first");
    first.keep().expect("keep");
    let kept_dir = first.dir().to_path_buf();
    drop(first);
    assert!(
        kept_dir.join(TREE_NAME).join("src/main.rs").exists(),
        "kept means kept"
    );

    let second = create(&fx.source, &options(&fx), now()).expect("second");
    assert!(!second.stable_dir());
    assert_ne!(second.dir(), kept_dir);
    assert!(
        kept_dir.join(TREE_NAME).join("src/main.rs").exists(),
        "still kept"
    );
}

#[test]
fn a_young_unowned_stable_directory_is_spared_and_the_name_not_taken() {
    let fx = fixture();
    let dir = fx.dest.join(stable_name(&fx.source));
    fs::create_dir_all(&dir).expect("mkdir");
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    assert!(!snap.stable_dir());
    assert!(
        dir.exists(),
        "a run in progress under an older binary is left alone"
    );
}

#[test]
fn redigest_reports_added_removed_and_changed_paths_sorted_and_is_empty_for_a_clean_tree() {
    let fx = fixture();
    write(&fx.source, "src/lib.rs", b"pub fn f() {}\n");
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    assert!(snap.redigest().expect("redigest").is_empty());

    fs::write(
        snap.root().join("src/main.rs"),
        b"fn main() { mutated() }\n",
    )
    .expect("change");
    fs::remove_file(snap.root().join("src/lib.rs")).expect("remove");
    write(snap.root(), "testdata/golden.txt", b"updated by a test\n");
    write(snap.root(), "Cargo.lock", b"# lock\n");

    let drifts = snap.redigest().expect("redigest");
    let summary: Vec<(&str, String)> = drifts
        .iter()
        .map(|d| (d.kind().name(), d.rel_path().to_owned()))
        .collect();
    assert_eq!(
        summary,
        [
            ("added", "Cargo.lock".to_owned()),
            ("removed", "src/lib.rs".to_owned()),
            ("changed", "src/main.rs".to_owned()),
            ("added", "testdata/golden.txt".to_owned()),
        ]
    );
    match &drifts[2] {
        Drift::Changed { want, got, .. } => {
            assert_eq!(want.size, 13);
            assert_eq!(want.sha256, sha256_hex(b"fn main() {}\n"));
            assert_eq!(got.size, 24);
            assert_eq!(got.sha256, sha256_hex(b"fn main() { mutated() }\n"));
        }
        other => panic!("expected Changed, got {other:?}"),
    }
    match &drifts[1] {
        Drift::Removed { want, .. } => assert_eq!(want.sha256, sha256_hex(b"pub fn f() {}\n")),
        other => panic!("expected Removed, got {other:?}"),
    }
    match &drifts[0] {
        Drift::Added { got, .. } => assert_eq!(got.size, 7),
        other => panic!("expected Added, got {other:?}"),
    }
}

#[test]
fn redigest_applies_no_exclusions_so_a_report_written_into_the_tree_is_drift() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    write(snap.root(), "reports/mutation/late.json", b"{}");
    write(snap.root(), ".git/HEAD", b"ref\n");
    let drifts = snap.redigest().expect("redigest");
    let paths: Vec<&str> = drifts.iter().map(Drift::rel_path).collect();
    assert_eq!(paths, [".git/HEAD", "reports/mutation/late.json"]);
}

#[cfg(unix)]
#[test]
fn redigest_refuses_a_link_that_grew_inside_the_snapshot() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    std::os::unix::fs::symlink("/etc", snap.root().join("src/etc")).expect("symlink");
    let error = snap.redigest().unwrap_err();
    assert_eq!(error.kind(), SnapshotErrorKind::Symlink);
    assert_eq!(error.path(), "src/etc");
}

#[test]
fn redigest_of_a_removed_tree_names_the_absolute_root() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    fs::remove_dir_all(snap.root()).expect("remove tree");
    let error = snap.redigest().unwrap_err();
    assert_eq!(error.kind(), SnapshotErrorKind::Walk);
    assert_eq!(Path::new(error.path()), snap.root());
}

#[test]
fn cleanup_removes_the_whole_directory_and_releases_the_lock_first() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    let dir = snap.dir().to_path_buf();
    snap.cleanup().expect("cleanup");
    assert!(!dir.exists());
    assert!(snapshot_dirs(&fx.dest).is_empty());
}

#[test]
fn dropping_an_unkept_snapshot_removes_it_best_effort() {
    let fx = fixture();
    let dir = {
        let snap = create(&fx.source, &options(&fx), now()).expect("create");
        snap.dir().to_path_buf()
    };
    assert!(!dir.exists(), "Drop is the deferred cleanup");
}

#[test]
fn keep_records_the_decision_in_the_marker_and_cleanup_becomes_a_no_op() {
    let fx = fixture();
    let mut snap = create(&fx.source, &options(&fx), now()).expect("create");
    snap.keep().expect("keep");
    assert!(snap.kept());
    let dir = snap.dir().to_path_buf();
    assert!(read_marker(&dir).expect("marker").kept);
    snap.cleanup().expect("cleanup is a no-op after keep");
    assert!(dir.join(TREE_NAME).join("Cargo.toml").exists());
    let swept = tempowner::sweep(&fx.dest, &[DIR_PREFIX], now()).expect("sweep");
    assert_eq!(swept.kept, 1);
    assert!(swept.removed.is_empty());
    fs::remove_dir_all(&dir).expect("tidy");
}

#[test]
fn cleanup_retries_with_a_doubling_backoff_and_reports_a_directory_that_survives() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    let dir = snap.dir().to_path_buf();
    let attempts = RefCell::new(0usize);
    let sleeps = RefCell::new(Vec::new());
    let remove = |_: &Path| -> io::Result<()> {
        *attempts.borrow_mut() += 1;
        Err(io::Error::other("still mapped"))
    };
    let sleep = |d: Duration| sleeps.borrow_mut().push(d);
    let error = snap.cleanup_with(&remove, &sleep).unwrap_err();
    assert_eq!(error.kind(), SnapshotErrorKind::CleanupFailed);
    assert_eq!(*attempts.borrow(), CLEANUP_ATTEMPTS);
    assert_eq!(
        *sleeps.borrow(),
        [20, 40, 80, 160].map(Duration::from_millis)
    );
    assert!(error.to_string().contains("RM1011"), "{error}");
    assert!(dir.exists(), "the seam removed nothing");
    fs::remove_dir_all(&dir).expect("tidy");
}

#[test]
fn a_directory_that_is_already_gone_is_a_directory_that_was_removed() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    let dir = snap.dir().to_path_buf();
    let attempts = RefCell::new(0usize);
    let remove = |_: &Path| -> io::Result<()> {
        *attempts.borrow_mut() += 1;
        Err(io::Error::from(io::ErrorKind::NotFound))
    };
    snap.cleanup_with(&remove, &|_| panic!("nothing to wait for"))
        .expect("a directory nobody can find is a directory nobody has to remove");
    assert_eq!(
        *attempts.borrow(),
        1,
        "the lock is released before the removal, so a sweeper is free to have got there first \
         and retrying waits on nothing"
    );
    fs::remove_dir_all(&dir).expect("tidy");
}

#[test]
fn cleanup_succeeds_on_a_later_attempt_without_reporting_the_earlier_ones() {
    let fx = fixture();
    let snap = create(&fx.source, &options(&fx), now()).expect("create");
    let dir = snap.dir().to_path_buf();
    let attempts = RefCell::new(0usize);
    let sleeps = RefCell::new(0usize);
    let remove = |path: &Path| -> io::Result<()> {
        *attempts.borrow_mut() += 1;
        if *attempts.borrow() < 3 {
            return Err(io::Error::other("busy"));
        }
        fs::remove_dir_all(path)
    };
    let sleep = |_: Duration| *sleeps.borrow_mut() += 1;
    snap.cleanup_with(&remove, &sleep)
        .expect("third time lucky");
    assert_eq!(*attempts.borrow(), 3);
    assert_eq!(*sleeps.borrow(), 2);
    assert!(!dir.exists());
}

#[test]
fn the_cleanup_guard_refuses_anything_that_does_not_look_like_a_snapshot_directory() {
    let parent = Path::new("/tmp/parent");
    let ok = parent.join("rust-mutants-snap-0123456789abcdef");
    cleanup_guard(&ok, parent).expect("a snapshot directory in its parent");

    let cases: [(&str, PathBuf, &str); 4] = [
        ("empty", PathBuf::new(), "empty"),
        (
            "relative",
            PathBuf::from("rust-mutants-snap-0123456789abcdef"),
            "absolute",
        ),
        ("wrong prefix", parent.join("project"), DIR_PREFIX),
        (
            "wrong parent",
            Path::new("/somewhere/else").join("rust-mutants-snap-0123456789abcdef"),
            "parent",
        ),
    ];
    for (name, dir, expected_word) in cases {
        let error = cleanup_guard(&dir, parent).unwrap_err();
        assert_eq!(error.kind(), SnapshotErrorKind::CleanupRefused, "{name}");
        assert!(error.to_string().contains("RM1010"), "{name}: {error}");
        assert!(
            error.to_string().contains(expected_word),
            "{name}: {error} should mention {expected_word}"
        );
    }
}

mod properties {
    use std::collections::BTreeMap;

    use proptest::prelude::*;

    use super::*;

    fn tree() -> impl Strategy<Value = BTreeMap<String, Vec<u8>>> {
        let component = (0u8..3).prop_map(|n| format!("d{n}"));
        let leaf = (0u8..4).prop_map(|n| format!("f{n}.rs"));
        let path = (prop::collection::vec(component, 0..3), leaf).prop_map(|(dirs, leaf)| {
            let mut parts = dirs;
            parts.push(leaf);
            parts.join("/")
        });
        prop::collection::btree_map(path, prop::collection::vec(any::<u8>(), 0..64), 0..12)
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(24))]

        #[test]
        fn a_snapshot_is_exact_sorted_and_free_of_drift(files in tree()) {
            let temp = tempfile::tempdir().expect("tempdir");
            let source = temp.path().join("src-root");
            let dest = temp.path().join("dest");
            fs::create_dir_all(&source).expect("mkdir");
            fs::create_dir_all(&dest).expect("mkdir");
            for (rel, bytes) in &files {
                write(&source, rel, bytes);
            }
            let snap = create(&source, &Options::new(&dest), now()).expect("create");
            let expected: Vec<Entry> = files
                .iter()
                .map(|(rel, bytes)| Entry {
                    rel_path: rel.clone(),
                    size: bytes.len() as u64,
                    sha256: sha256_hex(bytes),
                })
                .collect();
            prop_assert_eq!(snap.manifest(), expected.as_slice());
            prop_assert!(snap.redigest().expect("redigest").is_empty());
            prop_assert_eq!(snap.workspace_digest(), workspace_digest(&expected));
        }
    }
}

#[test]
fn resealing_absorbs_an_intended_rewrite_so_later_drift_means_a_test_wrote() {
    let fx = fixture();
    let mut snap = create(&fx.source, &options(&fx), now()).expect("create");
    let before = snap.workspace_digest().to_owned();

    fs::write(
        snap.root().join("src/main.rs"),
        b"fn main() { guarded() }\n",
    )
    .expect("rewrite");
    write(snap.root(), "src/generated.rs", b"// generated\n");
    let absorbed = snap.reseal().expect("reseal");
    let kinds: Vec<(&str, &str)> = absorbed
        .iter()
        .map(|drift| (drift.kind().name(), drift.rel_path()))
        .collect();
    assert_eq!(
        kinds,
        [("added", "src/generated.rs"), ("changed", "src/main.rs")]
    );
    assert_ne!(
        snap.workspace_digest(),
        before,
        "the tree is not the tree it was"
    );
    assert!(
        snap.redigest().expect("redigest").is_empty(),
        "the rewrite is the new baseline"
    );

    write(snap.root(), "testdata/golden.txt", b"updated by a test\n");
    let drifts = snap.redigest().expect("redigest");
    assert_eq!(drifts.len(), 1);
    assert_eq!(drifts[0].rel_path(), "testdata/golden.txt");
    assert_eq!(drifts[0].kind(), rust_mutants::snapshot::DriftKind::Added);
}

#[test]
fn a_directory_tagged_as_a_cache_is_not_copied() {
    let source = tempfile::tempdir().expect("tempdir");
    let dest = tempfile::tempdir().expect("tempdir");
    fs::write(source.path().join("Cargo.toml"), b"[package]\n").expect("a manifest");
    let target = source.path().join("target");
    fs::create_dir_all(target.join("debug/deps")).expect("a build directory");
    fs::write(
        target.join("CACHEDIR.TAG"),
        b"Signature: 8a477f597d28d172789f06886806bc55\n",
    )
    .expect("the tag");
    fs::write(target.join("debug/deps/huge.rlib"), vec![0_u8; 4096]).expect("build output");

    let snapshot =
        create(source.path(), &Options::new(dest.path()), Timestamp::now()).expect("the snapshot");

    assert!(snapshot.root().join("Cargo.toml").is_file());
    assert!(
        !snapshot.root().join("target").exists(),
        "copying somebody else's build cache would be gigabytes and a race with the \
         cargo that is writing it"
    );
    assert!(
        snapshot
            .manifest()
            .iter()
            .all(|entry| !entry.rel_path.starts_with("target/")),
        "and it is not in the digest either"
    );
}

#[test]
fn a_directory_with_a_file_of_that_name_that_is_not_the_tag_is_copied() {
    let source = tempfile::tempdir().expect("tempdir");
    let dest = tempfile::tempdir().expect("tempdir");
    let ordinary = source.path().join("data");
    fs::create_dir_all(&ordinary).expect("a directory");
    fs::write(ordinary.join("CACHEDIR.TAG"), b"notes about caching\n").expect("a file");

    let snapshot =
        create(source.path(), &Options::new(dest.path()), Timestamp::now()).expect("the snapshot");
    assert!(
        snapshot.root().join("data/CACHEDIR.TAG").is_file(),
        "the signature is what tags a cache, not the name"
    );
}

#[test]
fn a_copied_file_keeps_the_time_the_original_was_written() {
    let source = tempfile::tempdir().expect("a source tree");
    let destination = tempfile::tempdir().expect("somewhere to copy to");
    fs::write(
        source.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\n",
    )
    .expect("a manifest");
    fs::create_dir_all(source.path().join("src")).expect("a source directory");
    let file = source.path().join("src/lib.rs");
    fs::write(&file, "pub fn one() -> u8 { 1 }\n").expect("a library");
    let long_ago = std::time::SystemTime::UNIX_EPOCH
        .checked_add(Duration::from_secs(1_600_000_000))
        .expect("a time before now");
    fs::File::options()
        .write(true)
        .open(&file)
        .expect("the library")
        .set_times(fs::FileTimes::new().set_modified(long_ago))
        .expect("a time somebody could have written it");

    let snapshot = create(source.path(), &Options::new(destination.path()), now())
        .expect("the tree is copied");
    let copied = fs::metadata(snapshot.root().join("src/lib.rs"))
        .expect("the copy")
        .modified()
        .expect("a modification time");
    assert_eq!(
        copied, long_ago,
        "cargo decides whether to compile a file by comparing its time with the artifact's. A \
         copy stamped with now is a copy cargo has to build again, every run, whatever it \
         already built."
    );
    snapshot.cleanup().expect("the copy goes away");
}
