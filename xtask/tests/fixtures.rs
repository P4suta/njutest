// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fixture conventions.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::path::Path;

use xtask::fixtures::check_fixture;

const HEADER: &str = "# SPDX-FileCopyrightText: 2026 mjutest contributors\n# SPDX-License-Identifier: MIT OR Apache-2.0\n";
const RS_HEADER: &str = "// SPDX-FileCopyrightText: 2026 mjutest contributors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n";

fn write(dir: &Path, relative: &str, text: &str) {
    let path = dir.join(relative);
    fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
    fs::write(path, text).expect("write");
}

fn good_fixture(dir: &Path) {
    write(
        dir,
        "Cargo.toml",
        &format!(
            "{HEADER}[workspace]\n\n[package]\nname = \"fixture-simple\"\nversion = \"0.1.0\"\nedition = \"2024\"\n"
        ),
    );
    write(dir, "Cargo.lock", "# generated\nversion = 4\n");
    write(dir, "src/lib.rs", &format!("{RS_HEADER}pub fn f() {{}}\n"));
}

#[test]
fn a_conforming_fixture_has_no_problems() {
    let dir = tempfile::tempdir().expect("tempdir");
    good_fixture(dir.path());
    assert_eq!(check_fixture(dir.path()), Vec::<String>::new());
}

#[test]
fn every_broken_convention_is_named() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(
        dir.path(),
        "Cargo.toml",
        "[package]\nname = \"x\"\nversion = \"0.1.0\"\n\n[dependencies]\nserde = \"1\"\n",
    );
    write(dir.path(), "src/lib.rs", "pub fn f() {}\n");
    let problems = check_fixture(dir.path());
    assert_eq!(
        problems,
        [
            "Cargo.lock is missing (commit it: fixtures build with --locked --offline)",
            "Cargo.toml declares [dependencies]; fixtures have no dependencies",
            "Cargo.toml needs an empty [workspace] table to stay independent of the root workspace",
            "Cargo.toml: missing the SPDX header",
            "src/lib.rs: missing the SPDX header",
        ]
    );
}

#[test]
fn a_missing_manifest_is_reported_and_the_build_directory_is_ignored() {
    let dir = tempfile::tempdir().expect("tempdir");
    write(dir.path(), "target/debug/build/x.rs", "no header here");
    let problems = check_fixture(dir.path());
    assert_eq!(
        problems,
        [
            "Cargo.lock is missing (commit it: fixtures build with --locked --offline)",
            "Cargo.toml is missing"
        ]
    );
}
