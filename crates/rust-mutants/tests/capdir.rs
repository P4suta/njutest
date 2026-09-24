// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a capability directory names every operation relative to the directory it holds, and refuses what a name could smuggle in.

#![cfg_attr(
    unix,
    expect(
        clippy::expect_used,
        reason = "a test reports a setup failure by panicking"
    )
)]

use rust_mutants::capdir::Name;

#[test]
fn a_name_is_one_component_on_every_platform_the_store_runs_on() {
    for refused in [
        "",
        ".",
        "..",
        "a/b",
        "a\\b",
        "a:b",
        "a\0b",
        "trailing.",
        "trailing ",
        "con",
        "NUL.txt",
        "com1",
    ] {
        assert!(
            Name::new(refused).is_err(),
            "{refused:?} names something else somewhere"
        );
    }
    for accepted in [
        "runs",
        "20260925t000000z-abcdef",
        ".njutest",
        "index.json",
        "console",
    ] {
        assert!(
            Name::new(accepted).is_ok(),
            "{accepted:?} is one plain component"
        );
    }
}

#[cfg(unix)]
mod posix {
    use rust_mutants::capdir::{Dir, Kind, Name, Privacy};

    fn name(text: &str) -> Name<'_> {
        Name::new(text).expect("a component")
    }

    #[test]
    fn creation_is_exclusive_and_a_rename_never_replaces_unless_asked() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = Dir::open(temp.path()).expect("the directory");
        std::io::Write::write_all(&mut dir.create_file(name("a")).expect("new"), b"first")
            .expect("write");
        assert_eq!(
            dir.create_file(name("a"))
                .map(|_| ())
                .map_err(|error| error.kind()),
            Err(std::io::ErrorKind::AlreadyExists),
            "a name already taken is refused, not opened"
        );
        std::io::Write::write_all(&mut dir.create_file(name("b")).expect("new"), b"second")
            .expect("write");
        assert_eq!(
            dir.rename_noreplace(name("a"), &dir, name("b"))
                .map_err(|error| error.kind()),
            Err(std::io::ErrorKind::AlreadyExists),
            "a rename that would replace is refused"
        );
        assert_eq!(std::fs::read(temp.path().join("b")).expect("b"), b"second");
        let before = dir
            .status_at(name("a"))
            .expect("stat")
            .expect("a is there")
            .identity;
        dir.rename_replace(name("a"), &dir, name("b"))
            .expect("replacing when asked");
        assert_eq!(
            dir.status_at(name("b"))
                .expect("stat")
                .map(|status| status.identity),
            Some(before),
            "the object is the one that moved"
        );
        assert_eq!(dir.status_at(name("a")).expect("stat"), None);
    }

    #[test]
    fn a_link_is_never_followed_and_a_private_directory_is_its_owners() {
        let temp = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("elsewhere");
        std::os::unix::fs::symlink(elsewhere.path(), temp.path().join("link")).expect("link");
        let dir = Dir::open(temp.path()).expect("the directory");
        assert!(
            dir.open_dir(name("link")).is_err(),
            "a link is not opened as a directory"
        );
        assert_eq!(
            dir.status_at(name("link"))
                .expect("stat")
                .map(|status| status.kind),
            Some(Kind::Other),
            "and is seen as what it is"
        );
        let made = dir
            .create_private_dir_exclusive(name("private"))
            .expect("made");
        assert_eq!(made.privacy().expect("privacy"), Privacy::OwnerOnly);
        assert!(
            dir.create_private_dir_exclusive(name("private")).is_err(),
            "exclusive"
        );
        let mut names = dir.entries().expect("entries");
        names.sort();
        assert_eq!(names, ["link", "private"]);
        made.sync().expect("sync");
        drop(made);
        dir.remove_dir(name("private")).expect("removed");
        dir.remove_file(name("link"))
            .expect("the link itself removed");
        assert!(
            std::fs::symlink_metadata(elsewhere.path()).is_ok_and(|metadata| metadata.is_dir()),
            "and not what it pointed at"
        );
    }
}
