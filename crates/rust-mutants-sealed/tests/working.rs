// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The root directory and the directory a guest starts in: which tree an absolute path reaches, how far a path climbs, and how the host enters the start through the guest's own `chdir`.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::fmt::Write as _;

use rust_mutants_sealed::{
    OverlayState, Preopens, Refusal, RefusalReason, SealedError, SealedStop, Snapshot, Transcript,
    WasiFunction,
};

use crate::common::{
    command, invocation, root, root_starting_in, run, runner, tree, uninterrupted,
};

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

/// What `asked` comes to, with the tree preopened at `path` as descriptor 3 and the root directory as descriptor 4.
fn through(path: &str, asked: &[Asked<'_>]) -> Transcript {
    let mut invoked = invocation();
    invoked.preopens =
        Preopens::new(vec![tree(path, package_tree()), root()]).expect("the tree and the root");
    run(&opening(asked), &invoked)
}

/// The refusals of `transcript`, as (function, reason, count).
fn refused(transcript: &Transcript) -> Vec<Refusal> {
    transcript.refusals().to_vec()
}

#[test]
fn the_root_directory_is_named_slash_to_the_guest() {
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
    invoked.preopens = Preopens::new(vec![tree("/work/tree", package_tree()), root()])
        .expect("the tree and the root");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"\x01\0\0\0/");
}

#[test]
fn an_absolute_path_given_to_the_root_reaches_the_tree_its_names_begin_with_and_no_other() {
    let transcript = through(
        "/work/tree",
        &[
            Asked {
                fd: 4,
                path: "work/tree/pkg/data.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "work//tree/./pkg/sub/inner.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "work/tree/top.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: "work/treehouse/top.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: "etc/passwd",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: "pkg/data.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: "work/tree/../tree/top.txt",
                errno: 76,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"datainnertop");
    assert_eq!(
        refused(&transcript),
        [Refusal {
            function: WasiFunction::PathOpen,
            reason: RefusalReason::Escape,
            count: 4,
        }],
        "a path that names no place in the tree is refused and recorded, and so is one whose `..` \
         would climb the machine's directories, which natively decide where it lands"
    );
}

#[test]
fn a_tree_reached_by_its_own_descriptor_is_climbed_no_higher_than_its_root() {
    let transcript = through(
        "/work/tree",
        &[
            Asked {
                fd: 3,
                path: "pkg/sub/../data.txt",
                errno: 0,
            },
            Asked {
                fd: 3,
                path: "../top.txt",
                errno: 76,
            },
            Asked {
                fd: 3,
                path: "/work/tree/top.txt",
                errno: 76,
            },
            Asked {
                fd: 3,
                path: r"pkg\data.txt",
                errno: 44,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"data");
    assert_eq!(
        refused(&transcript),
        [Refusal {
            function: WasiFunction::PathOpen,
            reason: RefusalReason::Escape,
            count: 2,
        }]
    );
}

#[test]
fn a_windows_path_given_to_the_root_reaches_the_tree_however_it_goes_on() {
    let transcript = through(
        r"C:\work\tree",
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
                path: r"C:\work\tree\pkg/C:\work\tree\pkg\sub\inner.txt",
                errno: 0,
            },
            Asked {
                fd: 4,
                path: r"C:\work\tree\pkg/..\pkg\data.txt",
                errno: 0,
            },
        ],
    );
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.stdout().bytes(),
        b"datainnertoptoptopinnerdata",
        "a path from a drive's root joined onto the directory the guest started in is read from \
         that drive's root, as Windows reads an absolute path joined onto another"
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
        &[
            Asked {
                fd: 4,
                path: r"C:\work\tree\..\secret",
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
                fd: 4,
                path: r"C:\work\tree\pkg/D:\elsewhere\top.txt",
                errno: 76,
            },
            Asked {
                fd: 4,
                path: "pkg/data.txt",
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
            count: 8,
        }]
    );
}

#[test]
fn of_two_trees_a_path_reaches_the_one_whose_root_is_deepest() {
    let inner = Snapshot::builder()
        .file("top.txt", b"inner-top".to_vec())
        .and_then(rust_mutants_sealed::SnapshotBuilder::build)
        .expect("the inner tree is valid");
    let bytes = opening(&[
        Asked {
            fd: 5,
            path: "work/tree/pkg/top.txt",
            errno: 0,
        },
        Asked {
            fd: 5,
            path: "work/tree/top.txt",
            errno: 0,
        },
    ]);
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![
        tree("/work/tree", package_tree()),
        tree("/work/tree/pkg", inner),
        root(),
    ])
    .expect("two trees and the root");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(transcript.stdout().bytes(), b"inner-toptop");
}

#[test]
fn a_file_made_through_the_root_is_one_the_tree_holds() {
    let bytes = command(
        &["path_open", "fd_write", "fd_read"],
        "(data (i32.const 100) \"work/tree/pkg/made.txt\")
         (data (i32.const 200) \"pkg/made.txt\")
         (data (i32.const 300) \"made\")",
        "(call $expect (call $path_open (i32.const 4) (i32.const 0) (i32.const 100) (i32.const 22) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 0))
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
    invoked.preopens = Preopens::new(vec![tree("/work/tree", package_tree()), root()])
        .expect("the tree and the root");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.stdout().bytes(),
        b"made",
        "the tree reads what a path through the root wrote"
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

#[test]
fn the_root_itself_is_no_place_in_a_tree_and_is_refused() {
    let bytes = command(
        &["fd_readdir", "fd_filestat_get"],
        "",
        "(call $expect (call $fd_readdir (i32.const 4) (i32.const 1000) (i32.const 100) (i64.const 0) (i32.const 64)) (i32.const 76))
         (call $expect (call $fd_filestat_get (i32.const 4) (i32.const 1000)) (i32.const 76))",
    );
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![tree("/work/tree", package_tree()), root()])
        .expect("the tree and the root");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    let reasons: Vec<(WasiFunction, RefusalReason)> = refused(&transcript)
        .iter()
        .map(|refusal| (refusal.function, refusal.reason))
        .collect();
    assert_eq!(
        reasons,
        [
            (WasiFunction::FdFilestatGet, RefusalReason::Escape),
            (WasiFunction::FdReaddir, RefusalReason::Escape),
        ],
        "listing the machine's root, or reading what it is, names no place in a tree"
    );
}

#[test]
fn a_name_its_directory_holds_only_in_another_case_is_refused_and_recorded() {
    for (path, asked) in [
        (
            "/work/tree",
            ["work/tree/pkg/DATA.txt", "work/tree/Pkg/sub/inner.txt"],
        ),
        (
            r"C:\work\tree",
            [
                r"C:\work\tree\pkg\DATA.txt",
                r"C:\work\tree\Pkg\sub\inner.txt",
            ],
        ),
    ] {
        let transcript = through(
            path,
            &[
                Asked {
                    fd: 4,
                    path: asked[0],
                    errno: 76,
                },
                Asked {
                    fd: 4,
                    path: asked[1],
                    errno: 76,
                },
                Asked {
                    fd: 3,
                    path: "TOP.TXT",
                    errno: 76,
                },
                Asked {
                    fd: 3,
                    path: "pkg/data.txt",
                    errno: 0,
                },
                Asked {
                    fd: 3,
                    path: "pkg/missing.txt",
                    errno: 44,
                },
            ],
        );
        assert_eq!(transcript.stop(), SealedStop::Returned, "{path}");
        assert_eq!(transcript.stdout().bytes(), b"data", "{path}");
        let refusals: Vec<(WasiFunction, &str, u64)> = refused(&transcript)
            .iter()
            .map(|refusal| (refusal.function, refusal.reason.name(), refusal.count))
            .collect();
        assert_eq!(
            refusals,
            [(WasiFunction::PathOpen, "case-only", 3)],
            "a lookup a case-insensitive file system would answer from another name is refused, \
             under {path}"
        );
    }
}

#[test]
fn a_file_made_under_a_name_its_directory_holds_in_another_case_is_refused_and_not_made() {
    let bytes = command(
        &["path_open"],
        "(data (i32.const 100) \"pkg/Data.txt\")",
        "(call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 100) (i32.const 12) (i32.const 1) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 64)) (i32.const 76))",
    );
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![tree("/work/tree", package_tree()), root()])
        .expect("the tree and the root");
    let transcript = run(&bytes, &invoked);
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert!(
        transcript.overlay().is_empty(),
        "nothing is made beside the name it differs from only in case: {:?}",
        transcript.overlay()
    );
    let refusals: Vec<&str> = refused(&transcript)
        .iter()
        .map(|refusal| refusal.reason.name())
        .collect();
    assert_eq!(refusals, ["case-only"]);
}

/// A command that exports a `malloc` handing out memory at 4096 and a `chdir` that prints the path it is given and answers `answer`, and whose `_start` prints `started`.
fn entering(answer: &str) -> Vec<u8> {
    command(
        &[],
        &format!(
            "(data (i32.const 3000) \"started\")
             (func (export \"malloc\") (param i32) (result i32) (i32.const 4096))
             (func (export \"chdir\") (param $at i32) (result i32)
               (local $len i32)
               (block $done (loop $scan
                 (br_if $done (i32.eqz (i32.load8_u (i32.add (local.get $at) (local.get $len)))))
                 (local.set $len (i32.add (local.get $len) (i32.const 1)))
                 (br $scan)))
               (call $emit (local.get $at) (local.get $len))
               {answer})"
        ),
        "(call $emit (i32.const 3000) (i32.const 7))",
    )
}

#[test]
fn a_guest_starts_in_its_directory_through_its_own_chdir_before_start() {
    for (path, directory, entered) in [
        ("/work/tree", "pkg/sub", "/work/tree/pkg/sub"),
        ("/work/tree", "", "/work/tree"),
        (r"C:\work\tree", "pkg/sub", r"C:\work\tree\pkg\sub"),
    ] {
        let mut invoked = invocation();
        invoked.preopens = Preopens::new(vec![
            tree(path, package_tree()),
            root_starting_in(path, directory),
        ])
        .expect("the tree and the root");
        let transcript = run(&entering("(i32.const 0)"), &invoked);
        assert_eq!(
            transcript.stop(),
            SealedStop::Returned,
            "{path} {directory}"
        );
        assert_eq!(
            transcript.stdout().bytes(),
            format!("{entered}started").as_bytes(),
            "the host hands the guest's own chdir the directory as the tree's build spells a \
             path, before `_start`"
        );
    }
}

#[test]
fn a_chdir_that_stops_the_guest_is_the_guests_stop_and_one_that_refuses_is_an_error() {
    let mut invoked = invocation();
    invoked.preopens = Preopens::new(vec![
        tree("/work/tree", package_tree()),
        root_starting_in("/work/tree", "pkg"),
    ])
    .expect("the tree and the root");
    let trapped = run(&entering("(unreachable)"), &invoked);
    assert_eq!(
        trapped.stop(),
        SealedStop::Trapped {
            kind: rust_mutants_sealed::TrapKind::Unreachable
        },
        "the guest's own chdir ran the guest's own code, so how it stopped is an answer about \
         the guest"
    );
    assert_eq!(trapped.stdout().bytes(), b"/work/tree/pkg");
    let refused = runner()
        .prepare(&entering("(i32.const -1)"))
        .expect("the command is valid")
        .invoke(&invoked, &uninterrupted());
    assert!(
        matches!(&refused, Err(SealedError::StartRefused { path }) if path == "/work/tree/pkg"),
        "a chdir that refuses the directory leaves the guest somewhere it was not asked to \
         start, which is no answer about it: {refused:?}"
    );
    let unexported = runner()
        .prepare(&command(&[], "", ""))
        .expect("the command is valid")
        .invoke(&invoked, &uninterrupted());
    match unexported {
        Err(error @ SealedError::StartUnexported { export: "malloc" }) => {
            assert_eq!(error.code().code(), "RS1006");
        }
        other => panic!("a module without the exports cannot be started in a directory: {other:?}"),
    }
}
