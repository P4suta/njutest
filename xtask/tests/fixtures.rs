// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fixture conventions.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::fs;
use std::path::Path;

use xtask::fixtures::check_fixture;

const HEADER: &str = "# SPDX-FileCopyrightText: 2026 njutest contributors\n# SPDX-License-Identifier: MIT OR Apache-2.0\n";
const RS_HEADER: &str = "// SPDX-FileCopyrightText: 2026 njutest contributors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n";

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
    write(
        dir,
        "README.md",
        "# fixture-simple\n\nWhat it is for.\n\n```fates\nsrc/lib.rs:2:1 gt-to-ge killed\n```\n",
    );
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
            "README.md is missing (it is where a fixture says what it is for)",
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
            "Cargo.toml is missing",
            "README.md is missing (it is where a fixture says what it is for)",
        ]
    );
}

#[test]
fn a_readme_that_states_no_fates_is_a_fixture_a_change_can_quietly_re_decide() {
    let dir = tempfile::tempdir().expect("tempdir");
    good_fixture(dir.path());
    write(dir.path(), "README.md", "# x\n\nWhat it is for.\n");
    let problems = check_fixture(dir.path());
    assert_eq!(problems.len(), 1, "{problems:?}");
    assert!(
        problems
            .first()
            .is_some_and(|problem| problem.contains("```fates")),
        "{problems:?}"
    );
}

#[test]
fn a_sibling_fixture_library_is_the_one_path_allowed_to_climb() {
    let dir = tempfile::tempdir().expect("tempdir");
    good_fixture(dir.path());
    write(
        dir.path(),
        "Cargo.toml",
        &format!(
            "{HEADER}[workspace]\n\n[package]\nname = \"fixture-outside-dep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nfixture-outside-dep-lib = {{ path = \"../fixture-outside-dep-lib\" }}\n"
        ),
    );
    assert_eq!(
        check_fixture(dir.path()),
        Vec::<String>::new(),
        "a fixture that exists to read from outside itself reads from another fixture, so the \
         suite still builds from what this repository holds and still builds offline"
    );

    for (spelling, why) in [
        (
            "out = { path = \"../elsewhere\" }",
            "a sibling that is not a fixture",
        ),
        (
            "out = { path = \"../../fixture-far\" }",
            "a path that climbs further than beside",
        ),
        (
            "out = { path = \"../fixture-a/../../fixture-b\" }",
            "a path that climbs on its way",
        ),
    ] {
        write(
            dir.path(),
            "Cargo.toml",
            &format!(
                "{HEADER}[workspace]\n\n[package]\nname = \"x\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\n{spelling}\n"
            ),
        );
        let problems = check_fixture(dir.path());
        assert!(
            problems
                .iter()
                .any(|problem| problem.contains("not a path inside the fixture")),
            "{why}: {problems:?}"
        );
    }
}
