// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The `[patch]` table, which `cargo metadata` reports as though it were the graph all along.

use std::path::PathBuf;

use rust_mutants::cargo::manifest::{Patch, read_patches};

#[test]
fn patch_entries_with_a_path_are_read_from_the_root_manifest() {
    let text = r#"
[workspace]
members = ["crates/a"]

[patch.crates-io]
serde = { path = "../serde" }
regex = { path = "vendor/regex" }
other = { git = "https://example.invalid/other" }

[patch."https://example.invalid/registry"]
mine = { path = "../../mine" }
"#;
    assert_eq!(
        read_patches(text),
        vec![
            Patch {
                source: "crates-io".to_owned(),
                name: "regex".to_owned(),
                path: PathBuf::from("vendor/regex"),
            },
            Patch {
                source: "crates-io".to_owned(),
                name: "serde".to_owned(),
                path: PathBuf::from("../serde"),
            },
            Patch {
                source: "https://example.invalid/registry".to_owned(),
                name: "mine".to_owned(),
                path: PathBuf::from("../../mine"),
            },
        ],
        "every entry that names a directory, and no entry that names a source a copy still holds"
    );
}

#[test]
fn a_manifest_with_no_patch_table_has_no_patches() {
    assert!(read_patches("[package]\nname = \"a\"\n").is_empty());
    assert!(read_patches("").is_empty());
}

#[test]
fn a_manifest_that_does_not_parse_reports_nothing_rather_than_half_of_it() {
    assert!(
        read_patches("[patch.crates-io]\nserde = { path = ").is_empty(),
        "the build says what is wrong with a manifest in its own words; guessing at half a \
         document is worse than saying nothing"
    );
}
