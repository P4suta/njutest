// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The working directory: where a relative path starts, how far it climbs, and the spellings of its tree's root it answers.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::fmt::Write as _;

use rust_mutants_sealed::{
    OverlayState, Preopens, Refusal, RefusalReason, SealedStop, Snapshot, Transcript, WasiFunction,
};

use crate::common::{command, invocation, run, tree, working};

/// A tree holding a file at its root and a package with a file and a directory of its own.
fn package_tree() -> Snapshot {
    Snapshot::builder()
        .file("top.txt", b"top".to_vec())
        .and_then(|built| built.file("pkg/data.txt", b"data".to_vec()))
        .and_then(|built| built.file("pkg/sub/inner.txt", b"inner".to_vec()))
        .and_then(rust_mutants_sealed::SnapshotBuilder::build)
        .expect("the tree is valid")
}

/// One path the guest asks for through a descriptor, and the error number it is answered with.
struct Asked<'path> {
    fd: u32,
    path: &'path str,
    errno: u32,
}

/// A command that opens each asked path through its descriptor, expects its answer, and prints each file it opened.
fn opening(asked: &[Asked<'_>]) -> Vec<u8> {
    let mut data = String::new();
    let mut body = String::new();
    for (index, one) in asked.iter().enumerate() {
        let at = index
            .checked_add(1)
            .and_then(|place| place.checked_mul(100))
            .expect("a few paths fit the first page");
        let escaped = one.path.replace('\\', "\\\\");
        writeln!(data, "(data (i32.const {at}) \"{escaped}\")").expect("writing to a String");
        let open = format!(
            "(call $path_open (i32.const {fd}) (i32.const 0) (i32.const {at}) (i32.const {len}) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64))",
            fd = one.fd,
            len = one.path.len(),
        );
        writeln!(
            body,
            "(call $expect {open} (i32.const {errno}))",
            errno = one.errno
        )
        .expect("writing to a String");
        if one.errno == 0 {
            body.push_str(
                "(i32.store (i32.const 72) (i32.const 8000))
                 (i32.store (i32.const 76) (i32.const 64))
                 (call $expect (call $fd_read (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
                 (call $emit (i32.const 8000) (i32.load (i32.const 80)))
                 (call $expect (call $fd_close (i32.load (i32.const 64))) (i32.const 0))\n",
            );
        }
    }
    command(&["path_open", "fd_read", "fd_close"], &data, &body)
}

/// What `asked` comes to, with the tree preopened at `root` and its working directory at `directory`.
fn through(root: &str, directory: &str, asked: &[Asked<'_>]) -> Transcript {
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![tree(root, package_tree()), working(root, directory)])
        .expect("the tree and its working directory");
    run(&opening(asked), &invoked)
}

/// The refusals of `transcript`, as (function, reason, count).
fn refused(transcript: &Transcript) -> Vec<Refusal> {
    transcript.refusals().to_vec()
}

#[test]
fn the_working_directory_is_named_dot_to_the_guest() {
    let bytes = command(
        &["fd_prestat_get", "fd_prestat_dir_name"],
        "",
        "(call $expect (call $fd_prestat_get (i32.const 4) (i32.const 64)) (i32.const 0))
         (call $expect (call $fd_prestat_get (i32.const 5) (i32.const 64)) (i32.const 8))
         (call $expect (call $fd_prestat_dir_name (i32.const 4) (i32.const 200) (i32.const 1)) (i32.const 0))
         (call $emit (i32.const 68) (i32.const 4))
         (call $emit (i32.const 200) (i32.const 1))",
    );
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![
        tree("/work/tree", package_tree()),
        working("/work/tree", "pkg"),
    ])
    .expect("the tree and its working directory");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"\x01\0\0\0.");
}

#[test]
fn a_relative_path_starts_at_the_working_directory_and_climbs_no_higher_than_the_trees_root() {
    let transcript = through(
        "/work/tree",
        "pkg",
        &[
            Asked {
                fd: 4,
                path: "data.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "sub/../sub/inner.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "../top.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "../pkg/./data.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "../../outside",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: "/work/tree/top.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: r"sub\inner.txt",
                errno: 44,
            },
            Asked {
                fd: 3,
                path: "../top.txt",
                errno: 76,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"datainnertopdata");
    assert_eq!(
        refused(&transcript),
        [Refusal {
            function: WasiFunction::PathOpen,
            reason: RefusalReason::Escape,
            count: 3,
        }]
    );
}

#[test]
fn the_working_directory_at_the_trees_root_climbs_nowhere() {
    let transcript = through(
        "/work/tree",
        "",
        &[
            Asked {
                fd: 4,
                path: "top.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "pkg/data.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "../top.txt",
                errno: 76,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"topdata");
}

#[test]
fn a_windows_path_below_the_trees_root_starts_at_the_root_however_its_names_are_separated() {
    let transcript = through(
        r"C:\work\tree",
        "pkg",
        &[
            Asked {
                fd: 4,
                path: r"C:\work\tree\pkg/data.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: r"C:\work\tree/pkg\sub\inner.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "c:/work/tree/top.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: r"C:\Work\TREE\top.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: r"C:\work\tree\pkg\..\top.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: r"sub\inner.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: r"..\top.txt",
                errno: 0,
            },
            Asked {
                fd: 3,
                path: r"pkg\data.txt",
                errno: 0,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.stdout().bytes(),
        b"datainnertoptoptopinnertopdata"
    );
    assert!(
        transcript.refusals().is_empty(),
        "{:?}",
        transcript.refusals()
    );
}

#[test]
fn a_windows_path_outside_the_trees_root_is_refused_as_an_escape() {
    let transcript = through(
        r"C:\work\tree",
        "pkg",
        &[
            Asked {
                fd: 4,
                path: r"C:\work\tree\..\secret",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: r"..\..\secret",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: r"C:\work\treehouse\top.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: r"D:\work\tree\top.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: r"\work\tree\top.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: "C:top.txt",
                errno: 76,
            },
            Asked {
                fd: 3,
                path: r"C:\work\tree\top.txt",
                errno: 76,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        refused(&transcript),
        [Refusal {
            function: WasiFunction::PathOpen,
            reason: RefusalReason::Escape,
            count: 7,
        }]
    );
}

#[test]
fn a_file_made_through_the_working_directory_is_one_the_tree_holds() {
    let bytes = command(
        &["path_open", "fd_write", "fd_read"],
        "(data (i32.const 100) \"made.txt\")
         (data (i32.const 200) \"pkg/made.txt\")
         (data (i32.const 300) \"made\")",
        "(call $expect (call $path_open (i32.const 4) (i32.const 0) (i32.const 100) (i32.const 8) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 300))
         (i32.store (i32.const 76) (i32.const 4))
         (call $expect (call $fd_write (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 200) (i32.const 12) (i32.const 0) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
         (i32.store (i32.const 72) (i32.const 8000))
         (i32.store (i32.const 76) (i32.const 64))
         (call $expect (call $fd_read (i32.load (i32.const 64)) (i32.const 72) (i32.const 1) (i32.const 80)) (i32.const 0))
         (call $emit (i32.const 8000) (i32.load (i32.const 80)))",
    );
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![
        tree("/work/tree", package_tree()),
        working("/work/tree", "pkg"),
    ])
    .expect("the tree and its working directory");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.stdout().bytes(),
        b"made",
        "the tree reads what its working directory wrote"
    );
    let [made] = transcript.overlay() else {
        panic!("one entry: {:?}", transcript.overlay())
    };
    assert_eq!(made.path, "/work/tree/pkg/made.txt");
    assert!(
        matches!(&made.state, OverlayState::File { contents, .. } if contents == b"made"),
        "{made:?}"
    );
}
