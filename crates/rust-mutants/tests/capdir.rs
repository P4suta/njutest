// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a capability directory names every operation relative to the directory it holds, and refuses what a name could smuggle in.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::Path;

use rust_mutants::capdir::{Dir, Entry, Kind, Name, Privacy};

fn name(text: &str) -> Name<'_> {
    Name::new(text).expect("a component")
}

#[cfg(windows)]
#[test]
fn windows_owned_records_cover_acl_multi_batch_listing_and_long_renames() {
    let temp = tempfile::tempdir().expect("tempdir");
    let root = Dir::open(temp.path()).expect("directory");
    let dir = root
        .create_private_dir_exclusive(name("owned"))
        .expect("private directory");
    assert_eq!(
        dir.privacy().expect("ACL and process SIDs"),
        Privacy::OwnerOnly
    );
    let mut expected = Vec::new();
    for index in 0..200 {
        let entry = format!("entry-{index:04}-{}", "x".repeat(150));
        let file = dir.create_file(name(&entry)).expect("new file");
        drop(file);
        expected.push(entry);
    }
    let mut listed = dir.entries().expect("more than one OS batch");
    listed.sort();
    assert_eq!(listed, expected);
    let before = expected.first().expect("first entry");
    let after = format!("renamed-{}", "界".repeat(100));
    dir.rename_noreplace(name(before), &dir, name(&after))
        .expect("bounded UTF-16 rename record");
    assert!(dir.status_at(name(before)).expect("old name").is_none());
    assert!(dir.status_at(name(&after)).expect("new name").is_some());
    assert_eq!(
        dir.privacy().expect("ACL still private"),
        Privacy::OwnerOnly
    );
    dir.remove_contents().expect("remove every batch");
    assert!(dir.entries().expect("empty directory").is_empty());
}

/// Makes `at` a link to the directory `target`: a symbolic link on Unix, and on Windows a junction, which any user may make.
fn link(target: &Path, at: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, at).expect("link");
    #[cfg(windows)]
    {
        let made = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(at)
            .arg(target)
            .output()
            .expect("mklink runs");
        assert!(made.status.success(), "a junction was made: {made:?}");
    }
}

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
    link(elsewhere.path(), &temp.path().join("link"));
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
fn emptying_a_held_directory_removes_what_no_name_could_spell_and_follows_nothing() {
    let temp = tempfile::tempdir().expect("tempdir");
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    std::fs::write(elsewhere.path().join("kept"), b"outside").expect("outside");
    let root = temp.path();
    std::fs::create_dir_all(root.join("model/deep")).expect("nested");
    std::fs::write(root.join("model/deep/artifact"), b"a").expect("artifact");
    unspellable(root);
    link(elsewhere.path(), &root.join("link"));
    let dir = Dir::open(root).expect("the directory");
    dir.remove_contents().expect("emptied");
    assert_eq!(
        std::fs::read_dir(root).expect("listing").count(),
        0,
        "every entry went, including those a Name refuses and one that is not Unicode"
    );
    assert_eq!(
        std::fs::read(elsewhere.path().join("kept")).expect("still there"),
        b"outside",
        "and a link was removed as a link, never followed"
    );
}

/// Puts entries in `root` that no [`Name`] spells: a stream marker and a device name, and a name that is not Unicode where the file system holds one.
#[cfg(unix)]
fn unspellable(root: &Path) {
    #[cfg(not(target_os = "macos"))]
    use std::os::unix::ffi::OsStrExt as _;

    std::fs::write(root.join("a:b"), b"colon").expect("colon");
    std::fs::write(root.join("con"), b"device").expect("device");
    #[cfg(not(target_os = "macos"))]
    std::fs::write(
        root.join(std::ffi::OsStr::from_bytes(b"not-utf8-\xff")),
        b"bytes",
    )
    .expect("non-UTF-8, which APFS refuses to hold at all");
}

/// Puts entries in `root` that no [`Name`] spells: a device name, a trailing dot and space, a lone surrogate, and a read-only file, made through the verbatim path Windows spells nothing out of.
#[cfg(windows)]
fn unspellable(root: &Path) {
    use std::os::windows::ffi::OsStringExt as _;

    let verbatim = std::fs::canonicalize(root).expect("the verbatim spelling");
    for (entry, bytes) in [
        ("con", b"device".as_slice()),
        ("trailing.", b"dot"),
        ("trailing ", b"space"),
    ] {
        std::fs::write(verbatim.join(entry), bytes).expect("an entry Win32 would respell");
    }
    std::fs::write(
        verbatim.join(std::ffi::OsString::from_wide(&[0xd800, 0x78])),
        b"surrogate",
    )
    .expect("a name that is not UTF-16");
    let read_only = verbatim.join("read-only");
    std::fs::write(&read_only, b"locked").expect("read-only");
    let mut permissions = std::fs::metadata(&read_only)
        .expect("metadata")
        .permissions();
    permissions.set_readonly(true);
    std::fs::set_permissions(&read_only, permissions).expect("made read-only");
}

#[test]
fn an_entry_is_opened_once_and_is_what_the_open_handle_is() {
    let temp = tempfile::tempdir().expect("tempdir");
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let root = temp.path();
    std::fs::write(root.join("file"), b"bytes").expect("file");
    std::fs::create_dir_all(root.join("dir")).expect("dir");
    link(elsewhere.path(), &root.join("link"));
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
    #[cfg(unix)]
    {
        let fifo = std::process::Command::new("mkfifo")
            .arg(root.join("fifo"))
            .status()
            .expect("mkfifo runs");
        assert!(fifo.success(), "a fifo was made");
        assert!(
            matches!(dir.open_entry(name("fifo")).expect("fifo"), Entry::Other),
            "and a pipe is seen without waiting on it"
        );
    }
}

#[test]
fn a_directory_named_by_a_path_is_opened_as_one_on_every_platform() {
    let temp = tempfile::tempdir().expect("tempdir");
    let opened = rust_mutants::capdir::open_file_at(temp.path());
    assert!(
        opened.is_ok(),
        "a directory named by a path opens, as a file does: {opened:?}"
    );
    let Ok(opened) = opened else { return };
    assert_eq!(
        rust_mutants::capdir::file_status(&opened)
            .expect("an opened directory's status")
            .kind,
        Kind::Directory
    );
}

#[test]
fn a_file_named_by_a_path_is_opened_without_following_a_link_and_flushed_through_any_handle() {
    let temp = tempfile::tempdir().expect("tempdir");
    let elsewhere = tempfile::tempdir().expect("elsewhere");
    let root = temp.path();
    std::fs::write(root.join("file"), b"bytes").expect("file");
    link(elsewhere.path(), &root.join("link"));
    let opened = rust_mutants::capdir::open_file_at(&root.join("file")).expect("a plain file");
    rust_mutants::capdir::sync_file(&opened).expect("a handle opened for reading still flushes");
    assert!(
        rust_mutants::capdir::open_file_at(&root.join("link")).is_err(),
        "a link at the end of a path is not followed"
    );
    let dir = Dir::open(root).expect("the directory");
    let held = dir.open_file(name("file")).expect("held for reading");
    rust_mutants::capdir::sync_file(&held).expect("and flushed through the same kind of handle");
    dir.sync().expect("the directory flushes too");
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

#[cfg(unix)]
#[test]
fn owner_only_is_exactly_what_the_owner_needs_and_nothing_more() {
    use std::os::unix::fs::PermissionsExt as _;

    let temp = tempfile::tempdir().expect("tempdir");
    let dir = Dir::open(temp.path()).expect("the directory");
    for mode in [0o700, 0o500, 0o1700, 0o2700, 0o750, 0o701] {
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(mode))
            .expect("chmod");
        let applied = std::fs::metadata(temp.path())
            .expect("the directory remains")
            .permissions()
            .mode()
            & 0o7777;
        assert_eq!(applied & 0o777, mode & 0o777);
        let expected = if applied == 0o700 {
            Privacy::OwnerOnly
        } else {
            Privacy::Loose
        };
        assert_eq!(
            dir.privacy().expect("privacy"),
            expected,
            "requested {mode:o}, applied {applied:o}: owner-only is read, write and enter for the owner and no other bit"
        );
        if expected == Privacy::Loose {
            dir.restrict_to_owner().expect("tightened");
            assert_eq!(dir.privacy().expect("privacy"), Privacy::OwnerOnly);
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::os::windows::fs::OpenOptionsExt as _;

    use rust_mutants::capdir::{Dir, Privacy};
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    use super::name;

    fn icacls(path: &std::path::Path, grant: &str) {
        let granted = std::process::Command::new("icacls")
            .arg(path)
            .args(["/grant", grant])
            .output()
            .expect("icacls runs");
        assert!(granted.status.success(), "{grant} was granted: {granted:?}");
    }

    #[test]
    fn owner_only_is_a_protected_list_of_the_owner_the_system_and_the_administrators() {
        let temp = tempfile::tempdir().expect("tempdir");
        let dir = Dir::open(temp.path()).expect("the directory");
        assert_eq!(
            dir.privacy().expect("privacy"),
            Privacy::Loose,
            "a directory that inherits from its parent is not private, whoever the parent admits"
        );
        dir.restrict_to_owner().expect("tightened");
        assert_eq!(dir.privacy().expect("privacy"), Privacy::OwnerOnly);
        let made = dir
            .create_private_dir_exclusive(name("private"))
            .expect("made");
        assert_eq!(made.privacy().expect("privacy"), Privacy::OwnerOnly);
        icacls(&temp.path().join("private"), "*S-1-1-0:(R)");
        assert_eq!(
            made.privacy().expect("privacy"),
            Privacy::Loose,
            "a grant to everyone is loose"
        );
        made.restrict_to_owner().expect("tightened again");
        assert_eq!(made.privacy().expect("privacy"), Privacy::OwnerOnly);
    }

    #[test]
    fn a_removal_another_process_holds_the_entry_against_is_refused_with_the_cause_named() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("held"), b"bytes").expect("file");
        let dir = Dir::open(temp.path()).expect("the directory");
        let holder = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(temp.path().join("held"))
            .expect("held without sharing its removal");
        let refused = dir.remove_file(name("held"));
        assert!(
            matches!(&refused, Err(error) if error.to_string().contains("without sharing it")),
            "a removal that cannot happen says why, rather than succeeding silently: {refused:?}"
        );
        drop(holder);
        dir.remove_file(name("held"))
            .expect("the same removal once nobody holds it");
    }

    #[test]
    fn a_removed_or_replaced_entry_is_gone_by_name_while_another_handle_still_holds_it() {
        let temp = tempfile::tempdir().expect("tempdir");
        std::fs::write(temp.path().join("open"), b"bytes").expect("file");
        std::fs::write(temp.path().join("index"), b"old").expect("index");
        std::fs::write(temp.path().join("next"), b"new").expect("next");
        let dir = Dir::open(temp.path()).expect("the directory");
        let reader = std::fs::File::open(temp.path().join("open")).expect("a reader");
        dir.remove_file(name("open"))
            .expect("removed while a reader holds it");
        assert_eq!(
            dir.status_at(name("open")).expect("stat"),
            None,
            "the name is gone at once, not when the last reader lets go"
        );
        let old = std::fs::File::open(temp.path().join("index")).expect("an index reader");
        dir.rename_replace(name("next"), &dir, name("index"))
            .expect("replaced while a reader holds the old one");
        assert_eq!(
            std::fs::read(temp.path().join("index")).expect("the index"),
            b"new"
        );
        drop(reader);
        drop(old);
    }
}
