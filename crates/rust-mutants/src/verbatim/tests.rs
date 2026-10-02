// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;
use std::path::Path;

use super::{Kept, VerbatimError, keep, padded};

const TWICE: &str = "pub fn twice(value: i32) -> i32 {\n    value * 2\n}\n";

fn tree(files: &[(&str, &str)]) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a tree");
    for (path, text) in files {
        let path = root.path().join(path);
        std::fs::create_dir_all(path.parent().expect("a directory")).expect("the directory");
        std::fs::write(path, text).expect("the file");
    }
    root
}

fn sources(paths: &[&str]) -> BTreeMap<String, Option<String>> {
    paths
        .iter()
        .map(|path| ((*path).to_owned(), Some(String::new())))
        .collect()
}

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).expect("the file")
}

fn lines(text: &str) -> Vec<usize> {
    text.match_indices('\n').map(|(at, _)| at).collect()
}

#[test]
fn a_literal_include_reads_a_copy_as_it_was_copied_and_the_reader_keeps_every_position() {
    let lib = "pub mod twice;\npub const SOURCE: &str = include_str!(\"twice.rs\"); pub fn one() -> i32 { 1 }\n";
    let root = tree(&[("src/lib.rs", lib), ("src/twice.rs", TWICE)]);
    let kept = keep(root.path(), &sources(&["src/lib.rs", "src/twice.rs"])).expect("kept");
    assert_eq!(
        kept,
        [Kept {
            reader: "src/lib.rs".to_owned(),
            line: 2,
            read: "src/twice.rs".to_owned(),
            copy: "src/.0".to_owned(),
        }]
    );
    assert_eq!(read(root.path(), "src/.0"), TWICE);
    let now = read(root.path(), "src/lib.rs");
    assert_eq!(
        now,
        "pub mod twice;\npub const SOURCE: &str = include_str!(\".0\"      ); pub fn one() -> i32 { 1 }\n"
    );
    assert_eq!((now.len(), lines(&now)), (lib.len(), lines(lib)));
    assert_eq!(read(root.path(), "src/twice.rs"), TWICE);
}

#[test]
fn every_way_of_naming_a_source_of_the_tree_is_pointed_and_nothing_else_is() {
    let lib = concat!(
        "#![doc = include_str!(\"../README.md\")]\n",
        "#[doc = include_str!(\"twice.rs\")]\n",
        "pub const BYTES: &[u8] = include_bytes!(concat!(\n",
        "    env!(\"CARGO_MANIFEST_DIR\"),\n",
        "    \"/src/twice.rs\"\n",
        "));\n",
        "pub const GONE: &str = include_str!(\"gone.rs\");\n",
        "pub const OUTSIDE: &str = include_str!(\"../../outside.rs\");\n",
        "pub const TEXT: &str = include_str!(\"notes.txt\");\n",
    );
    let test = "const SOURCE: &str = core::include_str!(\"../src/twice.rs\");\n";
    let root = tree(&[
        ("README.md", "# a crate\n"),
        ("src/lib.rs", lib),
        ("src/twice.rs", TWICE),
        ("src/notes.txt", "notes\n"),
        ("src/.0", "a file of the tree's own\n"),
        ("tests/source.rs", test),
    ]);
    let kept = keep(
        root.path(),
        &sources(&["src/lib.rs", "src/twice.rs", "tests/source.rs"]),
    )
    .expect("kept");
    let said: Vec<(&str, usize, &str, &str)> = kept
        .iter()
        .map(|one| {
            (
                one.reader.as_str(),
                one.line,
                one.read.as_str(),
                one.copy.as_str(),
            )
        })
        .collect();
    assert_eq!(
        said,
        [
            ("src/lib.rs", 2, "src/twice.rs", "src/.1"),
            ("src/lib.rs", 3, "src/twice.rs", "src/.1"),
            ("tests/source.rs", 1, "src/twice.rs", "tests/.0"),
        ]
    );
    assert_eq!(read(root.path(), "src/.0"), "a file of the tree's own\n");
    assert_eq!(read(root.path(), "src/.1"), TWICE);
    assert_eq!(read(root.path(), "tests/.0"), TWICE);
    let now = read(root.path(), "src/lib.rs");
    assert_eq!((now.len(), lines(&now)), (lib.len(), lines(lib)));
    for untouched in [
        "#![doc = include_str!(\"../README.md\")]\n",
        "pub const GONE: &str = include_str!(\"gone.rs\");\n",
        "pub const OUTSIDE: &str = include_str!(\"../../outside.rs\");\n",
        "pub const TEXT: &str = include_str!(\"notes.txt\");\n",
    ] {
        assert!(now.contains(untouched), "{untouched:?} in {now:?}");
    }
    assert!(
        now.contains("#[doc = include_str!(\".1\"      )]\n"),
        "{now}"
    );
    let spread = format!(
        "include_bytes!(\".1\"    \n{}\n{}\n );\n",
        " ".repeat("    env!(\"CARGO_MANIFEST_DIR\"),".len()),
        " ".repeat("    \"/src/twice.rs\"".len())
    );
    assert!(now.contains(&spread), "{spread:?} in {now:?}");
    let test_now = read(root.path(), "tests/source.rs");
    assert_eq!(
        test_now,
        "const SOURCE: &str = core::include_str!(\".0\"             );\n"
    );
}

#[test]
fn a_tree_that_names_no_source_as_text_is_left_as_it_was() {
    let lib = "pub fn one() -> i32 { 1 }\n";
    let root = tree(&[("src/lib.rs", lib)]);
    assert_eq!(
        keep(root.path(), &sources(&["src/lib.rs"])).expect("kept"),
        []
    );
    assert_eq!(read(root.path(), "src/lib.rs"), lib);
}

#[test]
fn a_reader_that_is_not_rust_tokens_is_refused_rather_than_left_reading_what_the_run_rewrites() {
    let root = tree(&[
        (
            "src/lib.rs",
            "pub const S: &str = include_str!(\"twice.rs\"); \"unclosed\n",
        ),
        ("src/twice.rs", TWICE),
    ]);
    let refused = keep(root.path(), &sources(&["src/lib.rs"]));
    assert!(
        matches!(refused, Err(VerbatimError::Reading { ref path, .. }) if path == "src/lib.rs"),
        "{refused:?}"
    );
}

#[test]
fn a_name_that_does_not_fit_where_the_argument_stood_is_no_room() {
    assert_eq!(padded("\"a.rs\"", "\".0\""), Some("\".0\"  ".to_owned()));
    assert_eq!(padded("\"a.rs\"", "\".100\""), Some("\".100\"".to_owned()));
    assert_eq!(padded("\"a.rs\"", "\".1000\""), None);
    assert_eq!(
        padded("concat!(\n\"x\")", "\".0\""),
        Some("\".0\"    \n    ".to_owned())
    );
    assert_eq!(padded("con\ncat!()", "\".0\""), None);
    assert_eq!(padded("\"é.rs\"", "\".0\""), Some("\".0\"   ".to_owned()));
    assert_eq!(padded("\"éé\"", "\".0\""), Some("\".0\"  ".to_owned()));
}
