// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Replacing a file that a later run reads back, so that a reader holds one whole version of it.

#[test]
fn a_replacement_that_cannot_be_staged_names_where_the_bytes_were_going() {
    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("e".repeat(250));
    let failure = rust_mutants::replace::file(&destination, b"an answer a later run would read")
        .expect_err("a destination whose own name leaves no room for one beside it");
    assert!(
        failure.path != destination
            && failure.path.to_string_lossy().ends_with(".writing")
            && !destination.exists(),
        "the bytes go somewhere else first, so a replacement that never got that far \
         names where they were going rather than where they would have ended up: a \
         person told the destination refused goes looking at a path that is not the \
         problem, and finds nothing there, because nothing was written. It named {}",
        failure.path.display()
    );
}

#[test]
fn a_replacement_that_cannot_be_moved_into_place_leaves_nothing_of_itself_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let destination = dir.path().join("entry.json");
    std::fs::create_dir_all(&destination).expect("a directory where the entry goes");
    let failure = rust_mutants::replace::file(&destination, b"an answer a later run would read")
        .expect_err("a destination that is a directory");
    let left: Vec<String> = std::fs::read_dir(dir.path())
        .expect("the directory")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != "entry.json")
        .collect();
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

/// What one earlier run established about one mutant, as round `round` of a writer that keeps replacing it.
fn record(round: u64) -> rust_mutants::outcomes::Record {
    rust_mutants::outcomes::Record {
        schema: rust_mutants::outcomes::SCHEMA.to_owned(),
        mutant: "m1".to_owned(),
        outcome: "survived".to_owned(),
        target: "demo/lib/demo".to_owned(),
        tests_run: Some(1),
        failed_tests: (0..NAMED)
            .map(|one| format!("cases::round_{round}::test_{one:04}"))
            .collect(),
        run_id: format!("run-{round}"),
    }
}

#[test]
fn an_answer_a_reader_takes_while_a_run_replaces_it_is_one_whole_answer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = rust_mutants::outcomes::Store::new(dir.path());
    store.put("k1", &record(0));
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writing = {
        let store = store.clone();
        let done = std::sync::Arc::clone(&done);
        std::thread::spawn(move || {
            for round in 1..=ROUNDS {
                store.put("k1", &record(round));
            }
            done.store(true, std::sync::atomic::Ordering::SeqCst);
        })
    };
    let mut reads: u64 = 0;
    let mut missed: u64 = 0;
    while !done.load(std::sync::atomic::Ordering::SeqCst) {
        reads = reads.saturating_add(1);
        if store.get("k1", "m1").is_none() {
            missed = missed.saturating_add(1);
        }
    }
    writing.join().expect("the writer");
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
    let dir = tempfile::tempdir().expect("tempdir");
    let occupied = dir.path().join("occupied");
    std::fs::write(&occupied, "a file where a directory goes").expect("the file");
    let failure = rust_mutants::replace::file(&occupied.join("entry.json"), b"an answer")
        .expect_err("a directory that cannot be made");
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
