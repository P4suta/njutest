// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the cargo configuration files say a build compiles with.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::cargo::config::{Configured, SEPARATOR, configured, encoded, home, read};
use tempfile::TempDir;

fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    std::fs::create_dir_all(path.parent().expect("a parent")).expect("the directory");
    std::fs::write(&path, text).expect("the file");
}

fn env(pairs: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), OsString::from(value)))
        .collect()
}

#[test]
fn build_rustflags_are_joined_ancestors_first_then_deeper_files() {
    let root = TempDir::new().expect("a directory");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"outer\"]\n",
    );
    write(
        root.path(),
        "crate/.cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"inner\"]\n",
    );
    assert_eq!(
        configured(&root.path().join("crate"), None).build,
        ["--cfg", "outer", "--cfg", "inner"],
        "cargo joins the arrays rather than letting one win, and puts the higher precedence last"
    );
}

#[test]
fn config_dot_toml_wins_over_config_in_one_directory() {
    let root = TempDir::new().expect("a directory");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"new\"]\n",
    );
    write(
        root.path(),
        ".cargo/config",
        "[build]\nrustflags = [\"--cfg\", \"old\"]\n",
    );
    assert_eq!(configured(root.path(), None).build, ["--cfg", "new"]);
}

#[test]
fn cargo_home_is_read_last_and_its_flags_come_first() {
    let root = TempDir::new().expect("a directory");
    let cargo_home = TempDir::new().expect("a directory");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"tree\"]\n",
    );
    write(
        cargo_home.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"home\"]\n",
    );
    assert_eq!(
        configured(root.path(), Some(cargo_home.path())).build,
        ["--cfg", "home", "--cfg", "tree"],
        "the home directory is the lowest precedence cargo has, so its arguments come first"
    );
}

#[test]
fn a_target_table_with_rustflags_is_reported_and_not_merged() {
    let root = TempDir::new().expect("a directory");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = [\"--cfg\", \"tree\"]\n\n[target.x86_64-unknown-linux-gnu]\nrustflags = [\"-Clink-arg=-fuse-ld=lld\"]\n",
    );
    let found = configured(root.path(), None);
    assert!(
        found.target_specific,
        "which target tables apply is cargo's decision about the target being built"
    );
    assert_eq!(found.build, ["--cfg", "tree"]);
}

#[test]
fn an_unparsable_file_is_a_limitation_not_a_guess() {
    let root = TempDir::new().expect("a directory");
    write(root.path(), ".cargo/config.toml", "[build\nrustflags = ]\n");
    let found = configured(root.path(), None);
    assert!(found.unreadable);
    assert!(
        found.build.is_empty(),
        "a file nobody could parse says nothing about flags, and nothing is what is reported"
    );
}

#[test]
fn a_string_value_is_split_on_whitespace() {
    let root = TempDir::new().expect("a directory");
    write(
        root.path(),
        ".cargo/config.toml",
        "[build]\nrustflags = \"--cfg one   --cfg two\"\n",
    );
    assert_eq!(
        configured(root.path(), None).build,
        ["--cfg", "one", "--cfg", "two"]
    );
}

#[test]
fn a_tree_that_configures_nothing_configures_nothing() {
    let root = TempDir::new().expect("a directory");
    assert_eq!(configured(root.path(), None), Configured::default());
}

#[test]
fn the_home_cargo_keeps_its_configuration_in_is_the_one_the_environment_spells() {
    assert_eq!(
        home(&env(&[("CARGO_HOME", "/elsewhere"), ("HOME", "/home/one")])),
        Some(PathBuf::from("/elsewhere"))
    );
    assert_eq!(
        home(&env(&[("HOME", "/home/one")])),
        Some(PathBuf::from("/home/one/.cargo")),
        "an unset CARGO_HOME is the documented default, not the absence of a home"
    );
    assert_eq!(home(&env(&[])), None);
}

#[test]
fn the_environment_replaces_the_configuration_rather_than_joining_it() {
    let found = Configured {
        build: vec!["--cfg".to_owned(), "tree".to_owned()],
        ..Configured::default()
    };
    let encoded_flags = encoded(&env(&[("RUSTFLAGS", "--cfg caller")]), &found, &["-Cx"]);
    assert_eq!(
        encoded_flags,
        Some(OsString::from(
            ["--cfg", "caller", "-Cx"].join(&SEPARATOR.to_string())
        )),
        "cargo reads the environment instead of build.rustflags, so a run that puts flags back \
         must not put back the ones cargo would not have used"
    );
}

#[test]
fn nothing_configured_and_nothing_asked_for_leaves_the_variable_unset() {
    assert_eq!(encoded(&env(&[]), &Configured::default(), &[]), None);
}

#[test]
fn a_flag_that_carries_the_separator_could_not_be_passed_on_as_itself() {
    let found = read("[build]\nrustflags = [\"--cfg\\u001Fsmuggled\"]\n");
    assert!(
        found.unreadable && found.build.is_empty(),
        "a flag holding what the encoded variable separates arguments with would reach the \
         compiler as two flags, so a run says it could not read the file: {found:?}"
    );
}

#[test]
fn one_file_is_read_on_its_own_terms() {
    assert_eq!(
        read("[build]\nrustflags = [\"--cfg\", \"one\"]\n"),
        Configured {
            build: vec!["--cfg".to_owned(), "one".to_owned()],
            target_specific: false,
            unreadable: false,
        }
    );
    assert!(read("not toml [[[").unreadable);
}
