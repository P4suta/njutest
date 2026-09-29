// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use proptest::prelude::{ProptestConfig, prop_assert, prop_assert_eq, proptest};

use super::{Reading, Spelling};

/// The spelling of a tree a Windows build rooted at `C:\work\tree`.
fn windows() -> Spelling {
    Spelling::of(r"C:\work\tree")
}

#[test]
fn a_root_spelled_from_a_drive_is_a_windows_tree_and_every_other_root_a_posix_one() {
    for root in [
        r"C:\work\tree",
        r"C:\work\tree\",
        "C:/work/tree",
        r"C:\work\.\tree",
    ] {
        assert_eq!(
            Spelling::of(root),
            Spelling::Windows {
                drive: b'C',
                names: vec!["work".to_owned(), "tree".to_owned()],
            },
            "{root}"
        );
    }
    assert_eq!(
        Spelling::of(r"d:\tree"),
        Spelling::Windows {
            drive: b'd',
            names: vec!["tree".to_owned()],
        }
    );
    for root in [
        "/home/runner/tree",
        "/",
        ".",
        "C:",
        "C:tree",
        r"\\server\share\tree",
        "tree",
    ] {
        assert_eq!(Spelling::of(root), Spelling::Posix, "{root}");
    }
}

#[test]
fn a_windows_path_below_the_trees_root_starts_at_the_root_whatever_separates_its_names() {
    let spelling = windows();
    for (path, names) in [
        (
            r"C:\work\tree\pkg/tests/data.txt",
            vec!["pkg", "tests", "data.txt"],
        ),
        (
            r"C:\work\tree/pkg\tests\data.txt",
            vec!["pkg", "tests", "data.txt"],
        ),
        ("C:/work/tree/top.txt", vec!["top.txt"]),
        (r"C:\work\\tree\.\top.txt", vec!["top.txt"]),
        (r"C:\work\tree\pkg\..\top.txt", vec!["top.txt"]),
        (r"C:\work\..\work\tree\top.txt", vec!["top.txt"]),
        (r"C:\work\tree", Vec::new()),
        (r"C:\work\tree\", Vec::new()),
    ] {
        assert_eq!(spelling.read(path), Reading::Rooted(names), "{path}");
    }
}

#[test]
fn the_drive_letter_is_read_in_either_case_and_every_other_name_exactly() {
    let spelling = windows();
    assert_eq!(
        spelling.read(r"c:\work\tree\top.txt"),
        Reading::Rooted(vec!["top.txt"])
    );
    assert_eq!(
        Spelling::of(r"c:\work\tree").read(r"C:\work\tree\top.txt"),
        Reading::Rooted(vec!["top.txt"])
    );
    for path in [r"C:\Work\tree\top.txt", r"C:\work\TREE\top.txt"] {
        assert_eq!(spelling.read(path), Reading::Elsewhere, "{path}");
    }
}

#[test]
fn a_windows_path_outside_the_trees_root_is_elsewhere() {
    let spelling = windows();
    for path in [
        r"C:\work\tree\..\secret",
        r"C:\work\tree\pkg\..\..\secret",
        r"C:\work\treehouse\top.txt",
        r"C:\work",
        r"C:\",
        r"D:\work\tree\top.txt",
        r"\work\tree\top.txt",
        "/work/tree/top.txt",
        r"\\server\share\tree",
        "C:top.txt",
        "C:",
    ] {
        assert_eq!(spelling.read(path), Reading::Elsewhere, "{path}");
    }
}

#[test]
fn a_relative_windows_path_is_read_as_windows_reads_one() {
    let spelling = windows();
    for (path, names) in [
        (r"tests\data.txt", vec!["tests", "data.txt"]),
        ("tests/data.txt", vec!["tests", "data.txt"]),
        (r"tests\.\\data.txt", vec!["tests", "data.txt"]),
        (r"file.txt\..\other", vec!["other"]),
        (r"..\..\top.txt", vec!["..", "..", "top.txt"]),
        (r"a\..\..\b", vec!["..", "b"]),
        (".", Vec::new()),
    ] {
        assert_eq!(spelling.read(path), Reading::Relative(names), "{path}");
    }
}

#[test]
fn a_posix_path_separates_names_by_slash_alone_and_an_absolute_one_is_elsewhere() {
    let spelling = Spelling::of("/home/runner/tree");
    assert_eq!(
        spelling.read("tests//./data.txt"),
        Reading::Relative(vec!["tests", ".", "data.txt"])
    );
    assert_eq!(
        spelling.read("file.txt/../other"),
        Reading::Relative(vec!["file.txt", "..", "other"])
    );
    assert_eq!(
        spelling.read(r"tests\data.txt"),
        Reading::Relative(vec![r"tests\data.txt"])
    );
    assert_eq!(
        spelling.read(r"C:\work\tree\top.txt"),
        Reading::Relative(vec![r"C:\work\tree\top.txt"])
    );
    for path in ["/home/runner/tree/top.txt", "/etc/passwd", "/"] {
        assert_eq!(spelling.read(path), Reading::Elsewhere, "{path}");
    }
}

#[test]
fn a_trailing_separator_is_the_one_the_tree_is_spelled_with() {
    let posix = Spelling::of("/home/runner/tree");
    assert!(posix.trailing("out/"));
    assert!(!posix.trailing(r"out\"));
    let windows = windows();
    assert!(windows.trailing("out/"));
    assert!(windows.trailing(r"out\"));
    assert!(!windows.trailing("out"));
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn a_windows_reading_walks_only_names_and_climbs_only_from_where_it_starts(
        path in "[CcDw:./\\\\a-z]{0,24}",
    ) {
        match windows().read(&path) {
            Reading::Rooted(names) => {
                prop_assert!(
                    names.iter().all(|name| !name.is_empty()
                        && *name != "."
                        && *name != ".."
                        && !name.contains(['\\', '/'])),
                    "{path:?} walks {names:?} from the root"
                );
            }
            Reading::Relative(names) => {
                let climbs = names.iter().take_while(|name| **name == "..").count();
                prop_assert!(
                    names.iter().skip(climbs).all(|name| !name.is_empty()
                        && *name != "."
                        && *name != ".."
                        && !name.contains(['\\', '/'])),
                    "{path:?} walks {names:?} from where it starts"
                );
            }
            Reading::Elsewhere => {}
        }
    }

    #[test]
    fn a_posix_reading_splits_at_slashes_alone(path in "[C:./\\\\a-z]{0,24}") {
        match Spelling::of("/home/runner/tree").read(&path) {
            Reading::Relative(names) => {
                prop_assert!(!path.starts_with('/'));
                prop_assert!(
                    names.iter().all(|name| !name.is_empty() && !name.contains('/')),
                    "{path:?} walks {names:?}"
                );
                prop_assert_eq!(names.concat(), path.replace('/', ""));
            }
            Reading::Rooted(names) => prop_assert!(false, "{path:?} rooted at {names:?}"),
            Reading::Elsewhere => prop_assert!(path.starts_with('/')),
        }
    }
}
