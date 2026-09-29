// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A rename that puts a file at the path an invocation halts at ends the guest in that call, with nothing after it.

use rust_mutants_sealed::{Invocation, OverlayEntry, OverlayState, SealedError, SealedStop};

use crate::common::{command, invocation, run, runner, uninterrupted};

/// A guest that writes `said` to `notice.partial`, renames it to `notice`, and then makes `after`, all in the tree preopened first.
fn publishes_then_writes_on() -> Vec<u8> {
    command(
        &["path_open", "fd_write", "fd_close", "path_rename"],
        "(data (i32.const 100) \"notice.partial\") (data (i32.const 120) \"notice\") \
         (data (i32.const 140) \"after\") (data (i32.const 160) \"said\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 14) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 160))
         (i32.store (i32.const 76) (i32.const 4))
         (call $expect (call $fd_write (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
         (call $expect (call $fd_close (i32.load (i32.const 64))) (i32.const 0))
         (call $expect (call $path_rename (i32.const 3) (i32.const 100) (i32.const 14) (i32.const 3) (i32.const 120) (i32.const 6)) (i32.const 0))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 140) (i32.const 5) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))",
    )
}

/// The invocation of every test here, halting at `halt`.
fn halting_at(halt: Option<&str>) -> Invocation {
    Invocation {
        halt: halt.map(ToOwned::to_owned),
        ..invocation()
    }
}

/// Every path the overlay holds a file at, with its bytes.
fn files(overlay: &[OverlayEntry]) -> Vec<(&str, &[u8])> {
    overlay
        .iter()
        .filter_map(|entry| match &entry.state {
            OverlayState::File { contents, .. } => Some((entry.path.as_str(), contents.as_slice())),
            OverlayState::Directory { .. } | OverlayState::Removed => None,
        })
        .collect()
}

#[test]
fn a_rename_onto_the_halt_path_ends_the_guest_there_with_nothing_after_it() {
    let transcript = run(
        &publishes_then_writes_on(),
        &halting_at(Some("/sandbox/notice")),
    );
    assert_eq!(
        transcript.stop(),
        SealedStop::Halted,
        "the rename put a file at the halt path"
    );
    assert_eq!(
        files(transcript.overlay()),
        vec![("/sandbox/notice", b"said".as_slice())],
        "the overlay is what it held when the rename returned: the notice is there, and the file \
         the guest would have made next is not"
    );
}

#[test]
fn without_a_halt_or_with_one_elsewhere_the_same_guest_runs_on() {
    for halt in [None, Some("/sandbox/elsewhere")] {
        let transcript = run(&publishes_then_writes_on(), &halting_at(halt));
        assert_eq!(
            transcript.stop(),
            SealedStop::Returned,
            "{halt:?}: no rename put a file at a halt path"
        );
        assert_eq!(
            files(transcript.overlay()),
            vec![
                ("/sandbox/after", b"".as_slice()),
                ("/sandbox/notice", b"said".as_slice())
            ],
            "{halt:?}: the guest went on past the rename"
        );
    }
}

#[test]
fn a_halt_is_part_of_what_an_invocation_is_a_function_of() {
    let halted = run(
        &publishes_then_writes_on(),
        &halting_at(Some("/sandbox/notice")),
    );
    let unhalted = run(&publishes_then_writes_on(), &halting_at(None));
    assert_ne!(
        halted.invocation(),
        unhalted.invocation(),
        "two invocations that differ in where they halt are two invocations"
    );
}

#[test]
fn a_halt_no_tree_holds_a_place_for_is_refused_before_the_guest_starts() {
    for path in [
        "/elsewhere/notice",
        "/sandbox",
        "/sandbox/",
        "/sandbox/../notice",
        "/sandboxed/notice",
    ] {
        let refused = runner()
            .prepare(&publishes_then_writes_on())
            .expect("the command is valid")
            .invoke(&halting_at(Some(path)), &uninterrupted());
        assert!(
            matches!(&refused, Err(SealedError::Halt { path: said }) if said == path),
            "{path}: {refused:?}"
        );
        assert_eq!(
            refused.map(|_| ()).map_err(|error| error.code().code()),
            Err("RS0006"),
            "{path}"
        );
    }
}
