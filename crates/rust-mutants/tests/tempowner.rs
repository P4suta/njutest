// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owners, markers, locks, and the sweep that collects what dead runs left.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::create_dir,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use jiff::Timestamp;
use rust_mutants::tempowner::{
    ClaimError, LOCK_NAME, MARKER_NAME, MarkerError, Released, Role, SCHEMA, acquire, claim,
    claim_cache, claim_cache_of, lock_path, marker_path, read_marker, reclaim, release_kept, sweep,
    sweep_with,
};

/// The time a claim records, which no sweep reads.
fn now() -> Timestamp {
    Timestamp::now()
}

fn make(parent: &Path, name: &str) -> std::path::PathBuf {
    let dir = parent.join(name);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("payload.bin"), vec![0u8; 1024]).expect("payload");
    dir
}

/// Says whether a path exists without turning an inspection failure into absence.
fn exists(path: &Path) -> std::io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Says whether a path is a directory without turning an inspection failure into `false`.
fn is_directory(path: &Path) -> std::io::Result<bool> {
    match fs::metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

#[test]
fn claim_writes_the_marker_and_holds_the_lock_until_release() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = make(temp.path(), "rust-mutants-snap-0001");
    let at = now();
    let mut owner = claim(&dir, at).expect("claims");
    assert_eq!(owner.dir(), dir);
    assert!(
        lock_path(&dir).ends_with(LOCK_NAME)
            && fs::metadata(lock_path(&dir))
                .expect("lock metadata")
                .is_file()
    );
    assert!(
        marker_path(&dir).ends_with(MARKER_NAME)
            && fs::metadata(marker_path(&dir))
                .expect("marker metadata")
                .is_file()
    );

    let marker = read_marker(&dir).expect("readable");
    assert_eq!(marker.schema, SCHEMA);
    assert_eq!(marker.pid, std::process::id());
    assert_eq!(marker.started, at);
    assert!(!marker.kept);
    assert_eq!(&marker, owner.marker());

    assert!(
        acquire(&lock_path(&dir)).expect("opens").is_none(),
        "held by the owner"
    );
    assert!(matches!(claim(&dir, now()), Err(ClaimError::Owned { .. })));

    owner.release().expect("releases");
    owner.release().expect("idempotent");
    let mut taken = acquire(&lock_path(&dir))
        .expect("opens")
        .expect("free after release");
    taken.release().expect("releases");
}

#[test]
fn keep_records_the_decision_and_releases_the_lock() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = make(temp.path(), "rust-mutants-snap-0002");
    let mut owner = claim(&dir, now()).expect("claims");
    owner.keep().expect("keeps");
    assert!(read_marker(&dir).expect("readable").kept);
    assert!(
        acquire(&lock_path(&dir)).expect("opens").is_some(),
        "released"
    );
}

#[test]
fn a_marker_that_is_missing_or_malformed_is_a_distinct_failure() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = make(temp.path(), "rust-mutants-snap-0003");
    assert!(matches!(
        read_marker(&dir),
        Err(MarkerError::Missing { .. })
    ));
    fs::write(marker_path(&dir), b"{ not json").expect("write");
    assert!(matches!(
        read_marker(&dir),
        Err(MarkerError::Malformed { .. })
    ));
    fs::write(
        marker_path(&dir),
        br#"{"schema":"x","pid":1,"started":"2027-01-01T00:00:00Z","kept":false,"extra":1}"#,
    )
    .expect("write");
    assert!(
        matches!(read_marker(&dir), Err(MarkerError::Malformed { .. })),
        "unknown fields are refused"
    );
}

#[test]
fn the_lock_belongs_to_the_open_file_so_two_acquires_in_one_process_contend() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owner.lock");
    let mut first = acquire(&path).expect("opens").expect("free");
    assert!(acquire(&path).expect("opens").is_none());
    first.release().expect("releases");
    assert!(acquire(&path).expect("opens").is_some());
}

#[test]
fn dropping_a_lock_releases_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("owner.lock");
    {
        let held = acquire(&path).expect("opens").expect("free");
        assert!(acquire(&path).expect("opens").is_none());
        drop(held);
    }
    assert!(acquire(&path).expect("opens").is_some());
}

/// The flag Windows wants before it will open a directory as a handle.
#[cfg(windows)]
const BACKUP_SEMANTICS: u32 = 0x0200_0000;

/// The access Windows wants before it will let a handle's timestamps be written.
#[cfg(windows)]
const ATTRIBUTES: u32 = 0x0080 | 0x0100;

/// Opens `dir` as a handle its timestamps can be set through.
fn opened(dir: &Path) -> fs::File {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;
        fs::OpenOptions::new()
            .access_mode(ATTRIBUTES)
            .custom_flags(BACKUP_SEMANTICS)
            .open(dir)
            .expect("open")
    }
    #[cfg(not(windows))]
    {
        fs::File::open(dir).expect("open")
    }
}

fn age(dir: &Path, by: Duration) {
    let past = SystemTime::now().checked_sub(by).expect("in range");
    opened(dir).set_modified(past).expect("set mtime");
}

#[test]
fn a_missing_parent_is_a_machine_on_which_nothing_has_run_yet() {
    let temp = tempfile::tempdir().expect("tempdir");
    let result = sweep(&temp.path().join("nowhere"), &["rust-mutants-snap-"]).expect("ok");
    assert!(result.removed.is_empty() && result.failures.is_empty());
}

#[test]
fn the_sweep_removes_only_abandoned_owned_directories_wearing_a_prefix_however_old_the_rest() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let abandoned = make(parent, "rust-mutants-snap-dead");
    claim(&abandoned, now())
        .expect("claims")
        .release()
        .expect("releases abandoned owner");
    let live = make(parent, "rust-mutants-snap-live");
    let mut live_owner = claim(&live, now()).expect("claims");
    let kept = make(parent, "rust-mutants-snap-kept");
    claim(&kept, now()).expect("claims").keep().expect("keeps");
    let young = make(parent, "rust-mutants-snap-young");
    let old = make(parent, "rust-mutants-snap-old");
    age(&old, Duration::from_hours(24 * 365));
    let unrelated = make(parent, "somebody-else");
    let unrelated_old = make(parent, "somebody-else-old");
    age(&unrelated_old, Duration::from_hours(24 * 365));
    let file_with_prefix = parent.join("rust-mutants-snap-file");
    fs::write(&file_with_prefix, b"not a directory").expect("write");
    let other_prefix = make(parent, "rust-mutants-api-dead");
    claim(&other_prefix, now())
        .expect("claims")
        .release()
        .expect("releases other-prefix owner");

    let result = sweep(parent, &["rust-mutants-snap-", "rust-mutants-api-"]).expect("ok");

    let mut removed = result.removed.clone();
    removed.sort();
    assert_eq!(
        removed,
        [other_prefix.clone(), abandoned.clone()],
        "a directory with no marker names no owner to ask, so no age makes it anybody's to remove"
    );
    assert!(
        result.removed_bytes >= 2 * 1024 && result.removed_bytes < 2 * 1024 + 1024,
        "{}",
        result.removed_bytes
    );
    assert_eq!(result.live, 1);
    assert_eq!(result.kept, 1);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert!(
        !exists(&abandoned).expect("inspect abandoned")
            && !exists(&other_prefix).expect("inspect other prefix")
    );
    assert!(
        exists(&live).expect("inspect live")
            && exists(&kept).expect("inspect kept")
            && exists(&young).expect("inspect young")
            && exists(&old).expect("inspect old")
            && exists(&unrelated).expect("inspect unrelated")
            && exists(&unrelated_old).expect("inspect unrelated old")
    );
    assert!(exists(&file_with_prefix).expect("inspect prefixed file"));
    live_owner.release().expect("releases");
}

#[test]
fn a_directory_that_refuses_to_go_does_not_stop_the_sweep_of_the_others() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let stubborn = make(parent, "rust-mutants-snap-stubborn");
    claim(&stubborn, now())
        .expect("claims")
        .release()
        .expect("releases stubborn owner");
    let willing = make(parent, "rust-mutants-snap-willing");
    claim(&willing, now())
        .expect("claims")
        .release()
        .expect("releases willing owner");
    let remove = |dir: &Path| -> std::io::Result<()> {
        if dir.ends_with("rust-mutants-snap-stubborn") {
            Err(std::io::Error::other("stuck"))
        } else {
            fs::remove_dir_all(dir)
        }
    };
    let result = sweep_with(parent, &["rust-mutants-snap-"], &remove).expect("ok");
    assert_eq!(result.removed, std::slice::from_ref(&willing));
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].dir, stubborn);
    assert!(
        exists(&stubborn).expect("inspect stubborn") && !exists(&willing).expect("inspect willing")
    );
}

#[test]
fn a_half_written_marker_does_not_make_a_dead_directory_immortal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let dir = make(parent, "rust-mutants-snap-half");
    fs::write(marker_path(&dir), b"{\"schema\":\"rust-mut").expect("write");
    let result = sweep(parent, &["rust-mutants-snap-"]).expect("ok");
    assert_eq!(result.removed, std::slice::from_ref(&dir));
}

#[test]
fn a_cache_survives_a_sweep_and_is_reclaimed_only_when_asked() {
    let parent = tempfile::tempdir().expect("tempdir");
    let now = Timestamp::from_second(1_700_000_000).expect("a timestamp");

    let cache = parent.path().join("rust-mutants-target-aaaa");
    fs::create_dir(&cache).expect("mkdir");
    let mut owner = claim_cache(&cache, now, "rust-mutants-target-owner-v1")
        .expect("a fresh directory is claimable");
    owner.release().expect("release");

    let scratch = parent.path().join("rust-mutants-snap-bbbb");
    fs::create_dir(&scratch).expect("mkdir");
    claim(&scratch, now)
        .expect("a fresh directory is claimable")
        .release()
        .expect("release");

    let swept = sweep(
        parent.path(),
        &["rust-mutants-target-", "rust-mutants-snap-"],
    )
    .expect("sweep");
    assert_eq!(
        swept.removed.len(),
        1,
        "a routine sweep reclaims the scratch and spares the cache: {swept:?}"
    );
    assert_eq!(swept.cached, 1);
    assert!(
        is_directory(&cache).expect("inspect cache"),
        "the cache is what makes a second run fast"
    );
    assert!(!is_directory(&scratch).expect("inspect scratch"));

    let reclaimed = reclaim(parent.path(), &["rust-mutants-target-"]).expect("reclaim");
    assert_eq!(reclaimed.removed.len(), 1, "{reclaimed:?}");
    assert!(
        !is_directory(&cache).expect("inspect reclaimed cache"),
        "asking for the caches to go is what gc means"
    );
}

#[test]
fn a_cache_a_run_is_using_is_not_reclaimed() {
    let parent = tempfile::tempdir().expect("tempdir");
    let now = Timestamp::from_second(1_700_000_000).expect("a timestamp");
    let cache = parent.path().join("rust-mutants-target-cccc");
    fs::create_dir(&cache).expect("mkdir");
    let mut owner = claim_cache(&cache, now, "rust-mutants-target-owner-v1")
        .expect("a fresh directory is claimable");

    let reclaimed = reclaim(parent.path(), &["rust-mutants-target-"]).expect("reclaim");
    assert!(reclaimed.removed.is_empty(), "{reclaimed:?}");
    assert_eq!(reclaimed.live, 1);
    assert!(is_directory(&cache).expect("inspect live cache"));
    owner.release().expect("release");
}

#[test]
fn a_marker_written_before_roles_existed_still_reads() {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::write(
        dir.path().join(MARKER_NAME),
        "{\"schema\":\"rust-mutants-temp-owner-v1\",\"pid\":1,\"started\":\"2026-01-01T00:00:00Z\",\
         \"kept\":false}",
    )
    .expect("write");
    let marker = read_marker(dir.path()).expect("a marker without a role reads");
    assert_eq!(marker.role, Role::Scratch);
    assert!(!marker.kept);
}

#[test]
fn a_cache_whose_tree_is_gone_is_collected_rather_than_kept_for_a_run_that_cannot_come() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let tree = parent.join("tree");
    fs::create_dir(&tree).expect("the tree");
    let living = parent.join("rust-mutants-target-living");
    let orphan = parent.join("rust-mutants-target-orphan");
    fs::create_dir(&living).expect("the living cache");
    fs::create_dir(&orphan).expect("the orphaned cache");
    claim_cache_of(&living, now(), SCHEMA, &tree)
        .expect("claim")
        .release()
        .expect("release living cache");
    claim_cache_of(&orphan, now(), SCHEMA, &parent.join("gone"))
        .expect("claim")
        .release()
        .expect("release orphan cache");

    let swept = sweep(parent, &["rust-mutants-target-"]).expect("sweep");
    assert!(
        exists(&living).expect("inspect living cache"),
        "a cache the next run of that tree can still hit is what a cache is for"
    );
    assert!(
        !exists(&orphan).expect("inspect orphan cache"),
        "a cache keyed to a tree nobody can name again is one no run will ever look up, and \
         sparing it is how a temporary directory grows without bound: {:?}",
        swept.removed
    );
    assert_eq!(
        swept.cached, 1,
        "and the one it spared is counted as spared"
    );
}

#[test]
fn a_cache_that_names_no_tree_is_spared_as_it_always_was() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let dir = parent.join("rust-mutants-target-unkeyed");
    fs::create_dir(&dir).expect("the cache");
    claim_cache(&dir, now(), SCHEMA)
        .expect("claim")
        .release()
        .expect("release cache");

    let swept = sweep(parent, &["rust-mutants-target-"]).expect("sweep");
    assert!(
        exists(&dir).expect("inspect unkeyed cache"),
        "a marker written before caches said what they are keyed to says nothing about whether \
         a run can hit it, and a sweep that guessed would delete a cache somebody is about to use"
    );
    assert_eq!(swept.cached, 1);
}

#[test]
fn the_size_of_a_directory_is_every_regular_file_below_it() {
    use rust_mutants::tempowner::directory_size;

    let root = tempfile::tempdir().expect("a directory");
    let nested = root.path().join("one").join("two");
    fs::create_dir_all(&nested).expect("directories below it");
    fs::write(root.path().join("top"), b"1234567890").expect("a file at the top");
    fs::write(nested.join("deep"), b"12345").expect("a file two levels down");

    assert_eq!(
        directory_size(root.path()).expect("the directory is readable"),
        15,
        "a sweep says how much room it gave back, and a directory it walked only the top \
         of says a number smaller than the room"
    );
    assert_eq!(
        directory_size(&nested).expect("the nested directory is readable"),
        5,
        "and the same question about a directory below it is about that one"
    );
}

#[test]
fn a_directory_with_nothing_in_it_and_one_that_is_not_there_are_both_nothing() {
    use rust_mutants::tempowner::directory_size;

    let root = tempfile::tempdir().expect("a directory");
    assert_eq!(
        directory_size(root.path()).expect("the directory is readable"),
        0,
        "a directory with nothing in it gave back nothing"
    );
    assert_eq!(
        directory_size(&root.path().join("was-never-made"))
            .expect("an absent directory has size zero"),
        0,
        "and a directory that is not there is the same answer rather than a failure: \
         both a sweep and a cache ask this while something else is removing what they \
         are counting, and neither may stop because the answer moved"
    );
}

#[test]
fn a_keep_is_released_only_where_the_directory_vouches_for_it_and_nobody_holds_it() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let kept = make(parent, "rust-mutants-snap-kept");
    claim(&kept, now()).expect("claims").keep().expect("keeps");
    let held = make(parent, "rust-mutants-snap-held");
    claim(&held, now()).expect("claims").keep().expect("keeps");
    let mut holder = acquire(&lock_path(&held))
        .expect("inspects the lock")
        .expect("a kept directory's lock is free until somebody takes it");
    let unmarked = make(parent, "rust-mutants-snap-unmarked");

    assert_eq!(
        release_kept(&kept).expect("released"),
        Released::Removed,
        "a kept directory whose owner is gone goes when somebody asks"
    );
    assert_eq!(
        release_kept(&held).expect("inspected"),
        Released::Live,
        "a directory somebody still holds is theirs, kept or not"
    );
    assert_eq!(
        release_kept(&unmarked).expect("inspected"),
        Released::Unvouched,
        "a path in a ledger is not authority to delete: the directory has to say it was kept"
    );
    assert!(!exists(&kept).expect("inspect kept"));
    assert!(exists(&held).expect("inspect held") && exists(&unmarked).expect("inspect unmarked"));
    holder.release().expect("releases");
}

#[test]
fn every_marker_a_claim_writes_is_the_published_shape_a_collector_reads() {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../schema/temp-owner-v1.json"),
        )
        .expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");
    let temp = tempfile::tempdir().expect("tempdir");
    let scratch = make(temp.path(), "rust-mutants-snap-shape");
    let cache = make(temp.path(), "rust-mutants-target-shape");
    let keyed = make(temp.path(), "rust-mutants-target-keyed");
    let mut owners = vec![
        claim(&scratch, now()).expect("claims a scratch"),
        claim_cache(&cache, now(), SCHEMA).expect("claims a cache"),
        claim_cache_of(&keyed, now(), SCHEMA, temp.path()).expect("claims a keyed cache"),
    ];
    owners
        .first_mut()
        .expect("the scratch")
        .keep()
        .expect("keeps");
    for dir in [&scratch, &cache, &keyed] {
        let written: serde_json::Value = njutest_devkit::strictjson::decode_str(
            &fs::read_to_string(marker_path(dir)).expect("the marker"),
        )
        .expect("the marker is JSON");
        let errors: Vec<String> = validator
            .iter_errors(&written)
            .map(|error| error.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "a collector that is not this program decides what to take by this document, so \
             what a claim writes is exactly schema/temp-owner-v1.json: {written} {errors:?}"
        );
    }
    for mut owner in owners {
        owner.release().expect("releases");
    }
}

#[test]
fn a_fresh_directory_a_collector_is_looking_at_is_still_claimed_once_it_looks_away() {
    let temp = tempfile::tempdir().expect("tempdir");
    let fresh = make(temp.path(), "rust-mutants-snap-fresh");
    let mut collector = acquire(&lock_path(&fresh))
        .expect("inspects the lock")
        .expect("a collector holds a directory for the instant it judges it");
    let (answered, answer) = std::sync::mpsc::sync_channel(1);
    let claiming = fresh.clone();
    let claimer = njutest_devkit::thread::JoinedThread::launch(move || {
        answered
            .send(claim(&claiming, now()).map(|mut owner| owner.release()))
            .expect("says what it got");
    });
    let early = answer.recv_timeout(Duration::from_millis(500));
    assert!(
        early.is_err(),
        "while the collector holds the lock a claim has nothing to answer yet; answering is \
         giving the directory up: {early:?}"
    );
    collector.release().expect("the collector looks away");
    let answer = answer.recv().expect("the claimer answers once it may");
    claimer.join().expect("the claimer returns");
    assert!(
        matches!(answer, Ok(Ok(()))),
        "a directory with no marker yet is one nobody has claimed, so a lock on it is a \
         collector's, held for an instant, and a claim waits it out rather than leaving the \
         directory unowned for good: {answer:?}"
    );
    assert!(
        !read_marker(&fresh).expect("the marker").kept,
        "and the claim is the one on record"
    );
}

#[test]
fn a_cache_claim_says_so_to_every_tool_that_honours_the_cache_tag() {
    let temp = tempfile::tempdir().expect("tempdir");
    let cache = make(temp.path(), "rust-mutants-target-tagged");
    let mut owner = claim_cache(&cache, now(), SCHEMA).expect("claims");
    let tag = fs::read_to_string(cache.join("CACHEDIR.TAG")).expect("the tag");
    assert!(
        tag.starts_with("Signature: 8a477f597d28d172789f06886806bc55"),
        "{tag}"
    );
    owner.release().expect("releases");
}
