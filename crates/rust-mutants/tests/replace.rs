// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Replacing a file that a later run reads back, so that a reader holds one whole version of it.

use sha2::{Digest as _, Sha256};

#[test]
fn a_replacement_that_cannot_be_staged_names_where_the_bytes_were_going() {
    let dir = tempfile::tempdir();
    assert!(dir.is_ok(), "tempdir: {dir:?}");
    let Ok(dir) = dir else { return };
    let destination = dir.path().join("e".repeat(250));
    let failure = rust_mutants::replace::file(&destination, b"an answer a later run would read");
    assert!(
        failure.is_err(),
        "a destination whose own name leaves no room for one beside it: {failure:?}"
    );
    let Err(failure) = failure else { return };
    let staged = failure.path.to_str();
    assert!(staged.is_some(), "the temporary path is exact UTF-8");
    let Some(staged) = staged else { return };
    let destination_missing = std::fs::metadata(&destination);
    assert!(
        failure.path != destination
            && staged.ends_with(".writing")
            && matches!(destination_missing, Err(ref error) if error.kind() == std::io::ErrorKind::NotFound),
        "the bytes go somewhere else first, so a replacement that never got that far \
         names where they were going rather than where they would have ended up: a \
         person told the destination refused goes looking at a path that is not the \
         problem, and finds nothing there, because nothing was written. It named {}",
        failure.path.display()
    );
}

#[test]
fn a_replacement_that_cannot_be_moved_into_place_leaves_nothing_of_itself_behind() {
    let dir = tempfile::tempdir();
    assert!(dir.is_ok(), "tempdir: {dir:?}");
    let Ok(dir) = dir else { return };
    let destination = dir.path().join("entry.json");
    let created = std::fs::create_dir_all(&destination);
    assert!(
        created.is_ok(),
        "a directory where the entry goes: {created:?}"
    );
    let failure = rust_mutants::replace::file(&destination, b"an answer a later run would read");
    assert!(
        failure.is_err(),
        "a destination that is a directory: {failure:?}"
    );
    let Err(failure) = failure else { return };
    let entries = std::fs::read_dir(dir.path());
    assert!(entries.is_ok(), "the directory: {entries:?}");
    let Ok(entries) = entries else { return };
    let mut left = Vec::new();
    for entry in entries {
        assert!(entry.is_ok(), "directory entry: {entry:?}");
        let Ok(entry) = entry else { return };
        let name = entry.file_name().into_string();
        assert!(name.is_ok(), "the fixture writes exact UTF-8 names");
        let Ok(name) = name else { return };
        if name != "entry.json" {
            left.push(name);
        }
    }
    assert!(
        failure.path == destination && left.is_empty(),
        "a replacement that could not be made names the destination, and takes its \
         staged bytes with it: a store that keeps what it could not deliver fills with \
         files no reader will ever look at, and a person listing it cannot tell what it \
         holds from what it failed to write. It named {} and left {left:?}",
        failure.path.display()
    );
}

/// How many records the writer puts through one entry while the reader reads it.
const ROUNDS: u64 = 400;

/// How many failing tests one record names, so that writing one is not a single small write.
const NAMED: u64 = 300;

fn digest(byte: u8) -> rust_mutants::id::HexDigest {
    let mut hasher = Sha256::new();
    hasher.update([byte]);
    rust_mutants::id::HexDigest::finish(hasher)
}

/// What one earlier run established about one mutant, as round `round` of a writer that keeps replacing it.
fn record(round: u64) -> rust_mutants::outcomes::Record {
    rust_mutants::outcomes::Record {
        schema: rust_mutants::outcomes::SCHEMA.to_owned(),
        mutant: digest(b'a'),
        outcome: rust_mutants::outcomes::CacheOutcome::Survived,
        target: "demo/lib/demo".to_owned(),
        tests_run: Some(1),
        failed_tests: (0..NAMED)
            .map(|one| format!("cases::round_{round}::test_{one:04}"))
            .collect(),
        run_id: format!("run-{round}"),
        keyed: keyed(),
    }
}

/// What a sample record is keyed on beyond its mutant.
fn keyed() -> rust_mutants::outcomes::Keyed {
    rust_mutants::outcomes::Keyed {
        closure: "c".repeat(64),
        manifests: "m".repeat(64),
        toolchain: "cargo 1.98.0 rustc 1.98.0 aarch64-apple-darwin".to_owned(),
        args: Vec::new(),
        timeout: "auto".to_owned(),
        steps: 0,
        build: Vec::new(),
        engine: "e".to_owned(),
        runner: None,
        declared: rust_mutants::outcomes::Declared::of(&std::collections::BTreeSet::new(), &[]),
    }
}

#[test]
fn an_answer_a_reader_takes_while_a_run_replaces_it_is_one_whole_answer() {
    let dir = tempfile::tempdir();
    assert!(dir.is_ok(), "tempdir: {dir:?}");
    let Ok(dir) = dir else { return };
    let store = rust_mutants::outcomes::Store::new(dir.path());
    let key = record(0).key();
    let mutant = digest(b'a');
    let stored = store.put(&record(0));
    assert!(stored.is_ok(), "initial record: {stored:?}");
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writing = {
        let store = store.clone();
        let done = std::sync::Arc::clone(&done);
        njutest_devkit::thread::JoinedThread::launch(move || {
            for round in 1..=ROUNDS {
                let stored = store.put(&record(round));
                assert!(stored.is_ok(), "replace record: {stored:?}");
                if stored.is_err() {
                    return;
                }
            }
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        })
    };
    let mut reads: u64 = 0;
    let mut missed: u64 = 0;
    while !done.load(std::sync::atomic::Ordering::SeqCst) {
        assert_ne!(reads, u64::MAX, "the bounded fixture read count fits");
        let Some(next_reads) = reads.checked_add(1) else {
            return;
        };
        reads = next_reads;
        let read = store.get(&key, &mutant);
        assert!(read.is_ok(), "read complete record: {read:?}");
        let Ok(read) = read else { return };
        if read.is_none() {
            assert_ne!(missed, u64::MAX, "the bounded fixture miss count fits");
            let Some(next_missed) = missed.checked_add(1) else {
                return;
            };
            missed = next_missed;
        }
    }
    let joined = writing.join();
    assert!(joined.is_ok(), "the writer: {joined:?}");
    assert_eq!(
        missed, 0,
        "a cache is the one place a wrong answer may never come from, so an answer this \
         reader could not read is no answer and the run does the work again. That is the \
         right direction and the wrong cost: a store replaced in place is unreadable for \
         most of the time somebody is writing it, and a warm cache that answers nothing \
         is a cache nobody can tell from a cold one. {reads} reads, {missed} of them \
         with nothing to read"
    );
}

#[test]
fn a_destination_whose_directory_cannot_be_made_names_the_directory() {
    let dir = tempfile::tempdir();
    assert!(dir.is_ok(), "tempdir: {dir:?}");
    let Ok(dir) = dir else { return };
    let occupied = dir.path().join("occupied");
    let written = std::fs::write(&occupied, "a file where a directory goes");
    assert!(written.is_ok(), "the file: {written:?}");
    let failure = rust_mutants::replace::file(&occupied.join("entry.json"), b"an answer");
    assert!(
        failure.is_err(),
        "a directory that cannot be made: {failure:?}"
    );
    let Err(failure) = failure else { return };
    assert_eq!(
        failure.path, occupied,
        "a store takes its shape as it fills, so the run that fills it makes the \
         directory; when it cannot, every later step fails for the same reason and \
         names something else, and the path is the only thing that says which step this \
         was"
    );
}

#[test]
fn a_bare_name_is_staged_beside_itself_and_never_at_the_root() {
    assert_eq!(
        rust_mutants::replace::directory_of(std::path::Path::new("entry.json")),
        std::path::Path::new("."),
        "a bare name has a parent and it is the empty path, which names nothing a \
         directory can be made at: answering with it would stage the bytes at the \
         filesystem root on one platform and refuse on another, and neither is beside \
         the destination"
    );
    assert_eq!(
        rust_mutants::replace::directory_of(std::path::Path::new("/store/entry.json")),
        std::path::Path::new("/store"),
        "while a name with a directory is staged in that directory, so the rename that \
         follows stays on the filesystem the store is on"
    );
    assert_eq!(
        rust_mutants::replace::directory_of(std::path::Path::new("/")),
        std::path::Path::new("."),
        "and a path with no parent at all is the other way of naming no directory"
    );
}

#[test]
fn a_staged_name_says_whose_it_is_and_is_never_the_destination() {
    let staged = rust_mutants::replace::staging(std::path::Path::new("/store/entry.json"));
    assert!(
        staged.starts_with(".entry.json.")
            && staged.ends_with(".writing")
            && staged.contains(&std::process::id().to_string()),
        "the bytes are staged beside the destination under a name that says which writer \
         put them there, so two replacements that could overlap never share a path and a \
         reader listing the store can tell a half-written file from an entry: {staged}"
    );
    let nameless = rust_mutants::replace::staging(std::path::Path::new("/"));
    assert!(
        nameless.starts_with(".entry.") && nameless.ends_with(".writing"),
        "and a destination with no file name of its own is staged under one this module \
         chose rather than under nothing at all: {nameless}"
    );
}
