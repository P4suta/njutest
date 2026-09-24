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
    use rust_mutants::capdir::{Dir, Entry, Kind, Name, Privacy};

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

    #[test]
    fn owner_only_is_exactly_what_the_owner_needs_and_nothing_more() {
        use std::os::unix::fs::PermissionsExt as _;

        let temp = tempfile::tempdir().expect("tempdir");
        let dir = Dir::open(temp.path()).expect("the directory");
        for (mode, expected) in [
            (0o700, Privacy::OwnerOnly),
            (0o500, Privacy::Loose),
            (0o1700, Privacy::Loose),
            (0o2700, Privacy::Loose),
            (0o750, Privacy::Loose),
            (0o701, Privacy::Loose),
        ] {
            std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode))
                .expect("chmod");
            assert_eq!(
                dir.privacy().expect("privacy"),
                expected,
                "{mode:o}: owner-only is read, write and enter for the owner and no other bit"
            );
            if expected == Privacy::Loose {
                dir.restrict_to_owner().expect("tightened");
                assert_eq!(dir.privacy().expect("privacy"), Privacy::OwnerOnly);
            }
        }
    }

    #[test]
    fn emptying_a_held_directory_removes_what_no_name_could_spell_and_follows_nothing() {
        #[cfg(not(target_os = "macos"))]
        use std::os::unix::ffi::OsStrExt as _;

        let temp = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("elsewhere");
        std::fs::write(elsewhere.path().join("kept"), b"outside").expect("outside");
        let root = temp.path();
        std::fs::create_dir_all(root.join("model/deep")).expect("nested");
        std::fs::write(root.join("model/deep/artifact"), b"a").expect("artifact");
        std::fs::write(root.join("a:b"), b"colon").expect("colon");
        std::fs::write(root.join("con"), b"device").expect("device");
        #[cfg(not(target_os = "macos"))]
        std::fs::write(
            root.join(std::ffi::OsStr::from_bytes(b"not-utf8-\xff")),
            b"bytes",
        )
        .expect("non-UTF-8, which APFS refuses to hold at all");
        std::os::unix::fs::symlink(elsewhere.path(), root.join("link")).expect("link");
        let dir = Dir::open(root).expect("the directory");
        dir.remove_contents().expect("emptied");
        assert_eq!(
            std::fs::read_dir(root).expect("listing").count(),
            0,
            "every entry went, including those a Name refuses and one that is not UTF-8"
        );
        assert_eq!(
            std::fs::read(elsewhere.path().join("kept")).expect("still there"),
            b"outside",
            "and a link was removed as a link, never followed"
        );
    }

    #[test]
    fn an_entry_is_opened_once_and_is_what_the_open_handle_is() {
        let temp = tempfile::tempdir().expect("tempdir");
        let elsewhere = tempfile::tempdir().expect("elsewhere");
        let root = temp.path();
        std::fs::write(root.join("file"), b"bytes").expect("file");
        std::fs::create_dir_all(root.join("dir")).expect("dir");
        std::os::unix::fs::symlink(elsewhere.path(), root.join("link")).expect("link");
        let fifo = std::process::Command::new("mkfifo")
            .arg(root.join("fifo"))
            .status()
            .expect("mkfifo runs");
        assert!(fifo.success(), "a fifo was made");
        let dir = Dir::open(root).expect("the directory");
        assert!(matches!(
            dir.open_entry(name("file")).expect("file"),
            Entry::File(_)
        ));
        assert!(matches!(
            dir.open_entry(name("dir")).expect("dir"),
            Entry::Dir(_)
        ));
        assert!(
            matches!(dir.open_entry(name("link")), Ok(Entry::Other) | Err(_)),
            "a link is never followed into what it names"
        );
        assert!(
            matches!(dir.open_entry(name("fifo")).expect("fifo"), Entry::Other),
            "and a pipe is seen without waiting on it"
        );
    }

    #[test]
    fn emptying_refuses_a_tree_deeper_than_its_bound_rather_than_running_out_of_handles() {
        let temp = tempfile::tempdir().expect("tempdir");
        let mut deep = temp.path().to_path_buf();
        for _level in 0..=rust_mutants::capdir::REMOVAL_DEPTH {
            deep.push("d");
        }
        std::fs::create_dir_all(&deep).expect("a deep tree");
        let dir = Dir::open(temp.path()).expect("the directory");
        let refused = dir.remove_contents();
        assert!(
            matches!(&refused, Err(error) if error.kind() == std::io::ErrorKind::InvalidData),
            "a tree deeper than the bound is a named refusal: {refused:?}"
        );
    }
}
