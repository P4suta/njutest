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
fn a_path_dependency_inside_the_fixture_is_allowed_and_every_other_kind_is_not() {
    let dir = tempfile::tempdir().expect("tempdir");
    good_fixture(dir.path());
    write(
        dir.path(),
        "Cargo.toml",
        &format!(
            "{HEADER}[workspace]\nmembers = [\"crates/derive\"]\n\n[package]\nname = \"fixture-macros\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-macros-derive = {{ path = \"crates/derive\" }}\n"
        ),
    );
    assert_eq!(check_fixture(dir.path()), Vec::<String>::new());

    for (spelling, why) in [
        ("serde = \"1\"", "serde"),
        ("serde = { version = \"1\" }", "serde"),
        ("out = { path = \"../elsewhere\" }", "out"),
        ("out = { path = \"/abs\" }", "out"),
        ("g = { git = \"https://example.invalid/g\" }", "g"),
    ] {
        write(
            dir.path(),
            "Cargo.toml",
            &format!(
                "{HEADER}[workspace]\n\n[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{spelling}\n"
            ),
        );
        assert_eq!(
            check_fixture(dir.path()),
            [format!(
                "Cargo.toml: dependency {why:?} is not a path inside the fixture; fixtures build offline against no registry"
            )],
            "{spelling}"
        );
    }

    // Every dependency table is checked, target-specific ones included.
    write(
        dir.path(),
        "Cargo.toml",
        &format!(
            "{HEADER}[workspace]\n\n[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dev-dependencies]\na = \"1\"\n\n[build-dependencies]\nb = \"1\"\n\n[target.'cfg(unix)'.dependencies]\nc = \"1\"\n"
        ),
    );
    let problems = check_fixture(dir.path());
    assert_eq!(problems.len(), 3, "{problems:?}");
    assert!(
        problems.iter().all(|p| p.ends_with("against no registry")),
        "{problems:?}"
    );
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
            "Cargo.toml needs an empty [workspace] table to stay independent of the root workspace",
            "Cargo.toml: dependency \"serde\" is not a path inside the fixture; fixtures build offline against no registry",
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
