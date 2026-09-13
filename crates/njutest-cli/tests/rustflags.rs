// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a build is compiled with, and why the coverage build has to say it in the variable rather than the flag.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use njutest_cli::rustflags::{COVERAGE_FLAG, Configured, SEPARATOR, configured, encoded};

fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(key, value)| (OsString::from(key), OsString::from(value)))
        .collect()
}

fn parts(value: &OsString) -> Vec<String> {
    value
        .to_string_lossy()
        .split(SEPARATOR)
        .map(ToOwned::to_owned)
        .collect()
}

fn write(root: &Path, relative: &str, contents: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
    fs::write(path, contents).expect("the file");
}

#[test]
fn nothing_to_say_leaves_the_variable_unset_so_the_configuration_still_applies() {
    assert_eq!(encoded(&env(&[]), &Configured::default(), &[]), None);
}

#[test]
fn the_coverage_flag_is_appended_to_what_was_already_encoded() {
    let existing = format!("-Dwarnings{SEPARATOR}--cfg{SEPARATOR}fuzzing");
    let value = encoded(
        &env(&[("CARGO_ENCODED_RUSTFLAGS", &existing)]),
        &Configured::default(),
        &[COVERAGE_FLAG],
    )
    .expect("something to say");
    assert_eq!(
        parts(&value),
        ["-Dwarnings", "--cfg", "fuzzing", COVERAGE_FLAG]
    );
}

#[test]
fn a_plain_rustflags_is_split_the_way_cargo_splits_it() {
    let value = encoded(
        &env(&[("RUSTFLAGS", "  -Dwarnings   --cfg fuzzing ")]),
        &Configured::default(),
        &[COVERAGE_FLAG],
    )
    .expect("something to say");
    assert_eq!(
        parts(&value),
        ["-Dwarnings", "--cfg", "fuzzing", COVERAGE_FLAG],
        "on whitespace, and empty pieces are not arguments"
    );
}

#[test]
fn the_encoded_variable_wins_over_the_plain_one_exactly_as_cargo_does() {
    let value = encoded(
        &env(&[
            ("CARGO_ENCODED_RUSTFLAGS", "-Dwarnings"),
            ("RUSTFLAGS", "--cfg ignored"),
        ]),
        &Configured::default(),
        &[],
    )
    .expect("something to say");
    assert_eq!(parts(&value), ["-Dwarnings"]);
}

#[test]
fn what_the_project_configured_is_put_back_because_the_variable_would_replace_it() {
    let value = encoded(
        &env(&[]),
        &Configured {
            build: vec!["-Dwarnings".to_owned()],
            target_specific: false,
            unreadable: false,
        },
        &[COVERAGE_FLAG],
    )
    .expect("something to say");
    assert_eq!(
        parts(&value),
        ["-Dwarnings", COVERAGE_FLAG],
        "otherwise the binaries under verification are not the project's binaries"
    );
}

#[test]
fn an_environment_that_is_already_set_leaves_the_configuration_where_cargo_left_it() {
    let value = encoded(
        &env(&[("RUSTFLAGS", "-Dwarnings")]),
        &Configured {
            build: vec!["--cfg not-used-by-cargo-either".to_owned()],
            target_specific: false,
            unreadable: false,
        },
        &[COVERAGE_FLAG],
    )
    .expect("something to say");
    assert_eq!(
        parts(&value),
        ["-Dwarnings", COVERAGE_FLAG],
        "cargo ignores build.rustflags when the variable is set, and so does this"
    );
}

#[test]
fn build_rustflags_are_joined_with_the_ancestors_first_and_the_closest_file_last() {
    let root = tempfile::tempdir().expect("a temporary root");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"outer\"]\n",
    );
    let inner = root.path().join("workspace");
    write(
        &inner,
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"inner\"]\n",
    );

    let found = configured(&inner, &env(&[]));
    assert_eq!(
        found.build,
        ["--cfg", "outer", "--cfg", "inner"],
        "cargo joins arrays across files and places the higher precedence ones later"
    );
    assert!(!found.target_specific);
}

#[test]
fn a_rustflags_written_as_one_string_is_split_like_the_variable() {
    let root = tempfile::tempdir().expect("a temporary root");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = \"-Dwarnings --cfg fuzzing\"\n",
    );
    assert_eq!(
        configured(root.path(), &env(&[])).build,
        ["-Dwarnings", "--cfg", "fuzzing"]
    );
}

#[test]
fn target_specific_flags_are_noticed_and_not_merged_because_which_ones_apply_is_cargos_to_decide() {
    let root = tempfile::tempdir().expect("a temporary root");
    write(
        root.path(),
        ".cargo/config.toml",
        "[target.'cfg(unix)']\nrustflags = [\"--cfg\", \"unixish\"]\n",
    );
    let found = configured(root.path(), &env(&[]));
    assert!(found.build.is_empty());
    assert!(
        found.target_specific,
        "so a run can state the limitation rather than guess the answer"
    );
}

#[test]
fn a_configuration_that_is_not_toml_is_not_a_reason_to_stop() {
    let root = tempfile::tempdir().expect("a temporary root");
    write(root.path(), ".cargo/config.toml", "this is not toml [[[");
    let found = configured(root.path(), &env(&[]));
    assert!(found.build.is_empty());
    assert!(
        found.unreadable,
        "but a run says it could not read it, rather than reporting flags it never saw"
    );
}

#[test]
fn the_old_extensionless_name_is_read_too() {
    let root = tempfile::tempdir().expect("a temporary root");
    write(
        root.path(),
        ".cargo/config",
        "[build]\nrustflags = [\"--cfg\", \"old\"]\n",
    );
    assert_eq!(configured(root.path(), &env(&[])).build, ["--cfg", "old"]);
}

#[test]
fn a_target_table_costs_the_instrumented_build_the_flags_it_cannot_merge() {
    use njutest_cli::build::configured_limitations;
    use njutest_cli::rustflags::Configured;

    assert_eq!(
        configured_limitations(&Configured {
            target_specific: true,
            ..Configured::default()
        }),
        [njutest_cli::limitation::TARGET_RUSTFLAGS_NOT_MERGED],
        "which `target.*` table applies is cargo's decision about the target being built \
         rather than this build's, so the flags are left out and the run says so"
    );
}

#[test]
fn a_configuration_nothing_could_read_costs_the_build_everything_it_asked_for() {
    use njutest_cli::build::configured_limitations;
    use njutest_cli::rustflags::Configured;

    assert_eq!(
        configured_limitations(&Configured {
            unreadable: true,
            ..Configured::default()
        }),
        [rust_mutants::limitation::CARGO_CONFIGURATION_UNREADABLE],
        "a file this release could not read faithfully says nothing about what it asks \
         for, so nothing of it is written back into the instrumented build"
    );
}

#[test]
fn a_configuration_that_is_both_costs_both_and_says_both() {
    use njutest_cli::build::configured_limitations;
    use njutest_cli::rustflags::Configured;

    assert_eq!(
        configured_limitations(&Configured {
            target_specific: true,
            unreadable: true,
            build: Vec::new(),
        }),
        [
            njutest_cli::limitation::TARGET_RUSTFLAGS_NOT_MERGED,
            rust_mutants::limitation::CARGO_CONFIGURATION_UNREADABLE,
        ],
        "a reader told only one of them would go looking for the wrong file, and the \
         coverage a run routes by was taken from a build that differs from the project's \
         in both ways rather than in one"
    );
}

#[test]
fn flags_the_project_sets_for_every_target_cost_the_build_nothing() {
    use njutest_cli::build::configured_limitations;
    use njutest_cli::rustflags::Configured;

    assert!(
        configured_limitations(&Configured {
            build: vec!["--cfg".to_owned(), "tree".to_owned()],
            ..Configured::default()
        })
        .is_empty(),
        "those are written back in one place along with the instrumentation's own, so \
         there is nothing the build could not honour and nothing to say"
    );
    assert!(
        configured_limitations(&Configured::default()).is_empty(),
        "and a tree that configures nothing says nothing"
    );
}
