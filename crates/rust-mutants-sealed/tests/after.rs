// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An invocation can start where another stopped: its trees are the snapshots as the other's overlay left them.

use rust_mutants_sealed::{
    Invocation, OverlayEntry, OverlayState, Preopens, SealedError, SealedStop, Snapshot,
};

use crate::common::{command, invocation, root_starting_in, run, snapshot, tree};

/// A guest that makes `made/note` holding `kept` and removes `seen.txt`.
fn writes() -> Vec<u8> {
    command(
        &["path_create_directory", "path_open", "fd_write", "fd_close", "path_unlink_file"],
        "(data (i32.const 100) \"made\") (data (i32.const 120) \"made/note\") \
         (data (i32.const 140) \"seen.txt\") (data (i32.const 160) \"kept\")",
        "(call $expect (call $path_create_directory (i32.const 3) (i32.const 100) (i32.const 4)) (i32.const 0))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 120) (i32.const 9) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 160))
         (i32.store (i32.const 76) (i32.const 4))
         (call $expect (call $fd_write (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
         (call $expect (call $fd_close (i32.load (i32.const 64))) (i32.const 0))
         (call $expect (call $path_unlink_file (i32.const 3) (i32.const 140) (i32.const 8)) (i32.const 0))",
    )
}

/// A guest that finds `seen.txt` gone and prints what `made/note` holds.
fn reads() -> Vec<u8> {
    command(
        &["path_open", "fd_read", "path_filestat_get"],
        "(data (i32.const 120) \"made/note\") (data (i32.const 140) \"seen.txt\")",
        "(call $expect (call $path_filestat_get (i32.const 3) (i32.const 0) (i32.const 140) (i32.const 8) (i32.const 400)) (i32.const 44))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 120) (i32.const 9) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 200))
         (i32.store (i32.const 76) (i32.const 16))
         (call $expect (call $fd_read (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
         (call $emit (i32.const 200) (i32.load (i32.const 80)))",
    )
}

#[test]
fn an_invocation_started_after_another_reads_what_the_other_left() {
    let first = run(&writes(), &invocation());
    assert_eq!(first.stop(), SealedStop::Returned, "the writer ran whole");
    let next = Invocation {
        preopens: invocation()
            .preopens
            .after(first.overlay())
            .expect("the writer's overlay is one of these trees"),
        ..invocation()
    };
    let second = run(&reads(), &next);
    assert_eq!(
        (second.stop(), second.stdout().bytes()),
        (SealedStop::Returned, b"kept".as_slice()),
        "the reader found the file removed and the one made, as the writer left them"
    );
    let fresh = run(&reads(), &invocation());
    assert_ne!(
        fresh.stop(),
        SealedStop::Returned,
        "the same reader over the snapshot as it was finds nothing of the writer's"
    );
}

#[test]
fn a_snapshot_after_an_overlay_is_the_snapshot_built_with_what_it_left() {
    let made = OverlayState::Directory {
        accessed: 0,
        modified: 0,
    };
    let note = OverlayState::File {
        contents: b"kept".to_vec(),
        accessed: 0,
        modified: 0,
    };
    let left = [
        ("made", &made),
        ("made/note", &note),
        ("seen.txt", &OverlayState::Removed),
    ];
    let after = snapshot()
        .after(left)
        .expect("each change is below a directory the snapshot holds");
    let again = snapshot()
        .after(left)
        .expect("each change is below a directory the snapshot holds");
    assert_eq!(
        after.digest(),
        again.digest(),
        "the same changes over the same tree leave the same tree"
    );
    assert_eq!(
        after.digest(),
        after
            .after([])
            .expect("a tree its own overlay leaves alone is held")
            .digest(),
        "one tree, one digest"
    );
    let built = Snapshot::builder()
        .directory("empty")
        .and_then(|built| built.file("made/note", b"kept".to_vec()))
        .and_then(rust_mutants_sealed::SnapshotBuilder::build)
        .expect("the snapshot is valid");
    assert_ne!(
        after.digest(),
        built.digest(),
        "the times the overlay set are part of what the tree holds, which a tree built without any does not"
    );
    let retimed = OverlayState::File {
        contents: b"kept".to_vec(),
        accessed: 5,
        modified: 6,
    };
    let later = snapshot()
        .after([("made", &made), ("made/note", &retimed)])
        .expect("each change is below a directory the snapshot holds");
    assert_ne!(
        later.digest(),
        after.digest(),
        "a time the overlay set is part of what the tree holds"
    );
}

#[test]
fn an_overlay_that_removed_the_working_directory_is_no_state_an_invocation_starts_in() {
    let preopens = Preopens::new(vec![
        tree("/sandbox", snapshot()),
        root_starting_in("/sandbox", "empty"),
    ])
    .expect("the working directory is a directory of the tree");
    let removed = OverlayEntry {
        path: "/sandbox/empty".to_owned(),
        state: OverlayState::Removed,
    };
    let refused = preopens.after(&[removed]);
    assert_eq!(
        refused.map(|_| ()).map_err(|error| error.code().code()),
        Err("RS0005"),
        "the directory the guest runs in is gone, so the invocation cannot be given it"
    );
}

#[test]
fn an_overlay_entry_below_no_tree_or_no_directory_is_refused() {
    let lost = OverlayEntry {
        path: "/elsewhere/note".to_owned(),
        state: OverlayState::Removed,
    };
    assert!(matches!(
        invocation().preopens.after(&[lost]),
        Err(SealedError::SnapshotPath { .. })
    ));
    let orphan = OverlayEntry {
        path: "/sandbox/nowhere/note".to_owned(),
        state: OverlayState::Removed,
    };
    let refused = invocation().preopens.after(&[orphan]);
    assert_eq!(
        refused.map(|_| ()).map_err(|error| error.code().code()),
        Err("RS0003"),
        "a change inside a directory the tree does not hold is no change an overlay makes"
    );
}
