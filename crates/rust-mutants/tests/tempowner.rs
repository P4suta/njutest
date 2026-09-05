// SPDX-FileCopyrightText: 2026 mjutest contributors
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
    ClaimError, LEGACY_MAX_AGE, LOCK_NAME, MARKER_NAME, MarkerError, Role, SCHEMA, acquire, claim,
    claim_cache, lock_path, marker_path, read_marker, reclaim, sweep, sweep_with,
};

/// The real clock: the legacy rule compares against real modification times.
fn now() -> Timestamp {
    Timestamp::now()
}

fn make(parent: &Path, name: &str) -> std::path::PathBuf {
    let dir = parent.join(name);
    fs::create_dir_all(&dir).expect("mkdir");
    fs::write(dir.join("payload.bin"), vec![0u8; 1024]).expect("payload");
    dir
}

#[test]
fn claim_writes_the_marker_and_holds_the_lock_until_release() {
    let temp = tempfile::tempdir().expect("tempdir");
    let dir = make(temp.path(), "rust-mutants-snap-0001");
    let at = now();
    let mut owner = claim(&dir, at).expect("claims");
    assert_eq!(owner.dir(), dir);
    assert!(lock_path(&dir).ends_with(LOCK_NAME) && lock_path(&dir).is_file());
    assert!(marker_path(&dir).ends_with(MARKER_NAME) && marker_path(&dir).is_file());

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
        let _held = acquire(&path).expect("opens").expect("free");
        assert!(acquire(&path).expect("opens").is_none());
    }
    assert!(acquire(&path).expect("opens").is_some());
}

fn age(dir: &Path, by: Duration) {
    let past = SystemTime::now().checked_sub(by).expect("in range");
    fs::File::open(dir)
        .expect("open")
        .set_modified(past)
        .expect("set mtime");
}

#[test]
fn a_missing_parent_is_a_machine_on_which_nothing_has_run_yet() {
    let temp = tempfile::tempdir().expect("tempdir");
    let result = sweep(&temp.path().join("nowhere"), &["rust-mutants-snap-"], now()).expect("ok");
    assert!(result.removed.is_empty() && result.failures.is_empty());
}

#[test]
fn the_sweep_removes_only_abandoned_directories_wearing_a_prefix() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let abandoned = make(parent, "rust-mutants-snap-dead");
    drop(claim(&abandoned, now()).expect("claims").release());
    let live = make(parent, "rust-mutants-snap-live");
    let mut live_owner = claim(&live, now()).expect("claims");
    let kept = make(parent, "rust-mutants-snap-kept");
    claim(&kept, now()).expect("claims").keep().expect("keeps");
    let young = make(parent, "rust-mutants-snap-young");
    let old = make(parent, "rust-mutants-snap-old");
    age(&old, LEGACY_MAX_AGE + Duration::from_secs(60));
    let unrelated = make(parent, "somebody-else");
    let unrelated_old = make(parent, "somebody-else-old");
    age(&unrelated_old, LEGACY_MAX_AGE * 2);
    let file_with_prefix = parent.join("rust-mutants-snap-file");
    fs::write(&file_with_prefix, b"not a directory").expect("write");
    let other_prefix = make(parent, "rust-mutants-api-dead");
    drop(claim(&other_prefix, now()).expect("claims").release());

    let result = sweep(parent, &["rust-mutants-snap-", "rust-mutants-api-"], now()).expect("ok");

    let mut removed = result.removed.clone();
    removed.sort();
    assert_eq!(
        removed,
        [other_prefix.clone(), abandoned.clone(), old.clone()]
    );
    assert!(
        result.removed_bytes >= 3 * 1024 && result.removed_bytes < 3 * 1024 + 1024,
        "{}",
        result.removed_bytes
    );
    assert_eq!(result.live, 1);
    assert_eq!(result.kept, 1);
    assert!(result.failures.is_empty(), "{:?}", result.failures);
    assert!(!abandoned.exists() && !old.exists() && !other_prefix.exists());
    assert!(
        live.exists()
            && kept.exists()
            && young.exists()
            && unrelated.exists()
            && unrelated_old.exists()
    );
    assert!(file_with_prefix.exists());
    live_owner.release().expect("releases");
}

#[test]
fn a_directory_that_refuses_to_go_does_not_stop_the_sweep_of_the_others() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let stubborn = make(parent, "rust-mutants-snap-stubborn");
    drop(claim(&stubborn, now()).expect("claims").release());
    let willing = make(parent, "rust-mutants-snap-willing");
    drop(claim(&willing, now()).expect("claims").release());
    let remove = |dir: &Path| -> std::io::Result<()> {
        if dir.ends_with("rust-mutants-snap-stubborn") {
            Err(std::io::Error::other("stuck"))
        } else {
            fs::remove_dir_all(dir)
        }
    };
    let result = sweep_with(parent, &["rust-mutants-snap-"], now(), &remove).expect("ok");
    assert_eq!(result.removed, std::slice::from_ref(&willing));
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.failures[0].dir, stubborn);
    assert!(stubborn.exists() && !willing.exists());
}

#[test]
fn a_half_written_marker_does_not_make_a_dead_directory_immortal() {
    let temp = tempfile::tempdir().expect("tempdir");
    let parent = temp.path();
    let dir = make(parent, "rust-mutants-snap-half");
    fs::write(marker_path(&dir), b"{\"schema\":\"rust-mut").expect("write");
    let result = sweep(parent, &["rust-mutants-snap-"], now()).expect("ok");
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
        now,
    )
    .expect("sweep");
    assert_eq!(
        swept.removed.len(),
        1,
        "a routine sweep reclaims the scratch and spares the cache: {swept:?}"
    );
    assert_eq!(swept.cached, 1);
    assert!(cache.is_dir(), "the cache is what makes a second run fast");
    assert!(!scratch.is_dir());

    let reclaimed = reclaim(parent.path(), &["rust-mutants-target-"], now).expect("reclaim");
    assert_eq!(reclaimed.removed.len(), 1, "{reclaimed:?}");
    assert!(
        !cache.is_dir(),
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

    let reclaimed = reclaim(parent.path(), &["rust-mutants-target-"], now).expect("reclaim");
    assert!(reclaimed.removed.is_empty(), "{reclaimed:?}");
    assert_eq!(reclaimed.live, 1);
    assert!(cache.is_dir());
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
