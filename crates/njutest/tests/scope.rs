// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `[project] exclude` narrows, and what it may not: a file left out of the mutations is still compiled, still run, and still part of what the run is.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::path::Path;

use njutest::assure::identity::{Asked, Machine, inputs, of};
use njutest::config::Config;
use njutest::evidence::digest::Mode;
use njutest::evidence::key::Common;
use njutest_devkit::repo::Repo;

const fn machine() -> Machine<'static> {
    Machine {
        toolchain: "rustc 1.98.0",
        platform: "x86_64-unknown-linux-gnu",
        engine: "engine",
    }
}

fn tree_of(root: &Path, config: &Config) -> String {
    let machine = machine();
    let asked = Asked {
        root,
        config,
        machine: &machine,
        vars: &rust_mutants::vars::Variables::empty(),
        elsewhere: &[],
    };
    inputs(&asked, Mode::Full, &[], None)
        .expect("the tree reads")
        .tree
}

fn excluding(pattern: &str) -> Config {
    let mut config = Config::default();
    config.project.exclude = vec![pattern.to_owned()];
    config
}

fn common() -> Common {
    Common {
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        engine: "engine".to_owned(),
        environment: Vec::new(),
        contract: "standard-v1".to_owned(),
        test_args: Vec::new(),
        build: rust_mutants::cargo::BuildConfig::default().selection(),
        timeout_ms: 1,
        steps: 1,
        versions: Vec::new(),
        corpus: String::new(),
    }
}

#[test]
fn a_file_the_configuration_excludes_from_the_mutations_is_still_part_of_what_the_run_is() {
    let repo = Repo::new();
    repo.package("demo")
        .lib("pub fn f(a: i32) -> i32 { a + 1 }\n");
    repo.write("src/generated.rs", "pub const N: i32 = 1;\n");
    let config = excluding("src/generated.rs");

    let before = tree_of(repo.root(), &config);
    repo.write("src/generated.rs", "pub const N: i32 = 2;\n");
    let after = tree_of(repo.root(), &config);

    assert_ne!(
        before, after,
        "the exclusion says which files are mutated, not which are compiled: a run that \
         still builds and runs the file is a run the file can change the answer of, and an \
         identity that did not move would hand the next run an answer measured against \
         other bytes"
    );
}

#[test]
fn a_plan_compiles_what_the_configuration_says_to_compile() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    repo.write(
        ".njutest.toml",
        "version = 1\n\n[execution]\nfeatures = [\"imperial\"]\nall_features = true\nno_default_features = true\n",
    );
    let arguments = njutest::cli::Plan {
        directory: None,
        packages: Vec::new(),
        why: false,
        offline: true,
        locked: true,
    };

    let selection =
        njutest::app::plan::compiled(repo.root(), &arguments).expect("the configuration reads");

    assert_eq!(selection.features, ["imperial"]);
    assert!(selection.all_features);
    assert!(
        !selection.default_features,
        "the file says the default features are off, and the field says whether they are \
         on: a plan that read the negation the other way would describe a build cargo \
         would never do"
    );
}

#[test]
fn only_compiler_inputs_and_explicitly_named_variables_enter_the_run_identity() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    let mut config = Config::default();
    config.execution.environment = vec!["CUSTOM_INPUT".to_owned()];
    let vars = vec![
        (OsString::from("ORDINARY"), OsString::from("ignored")),
        (OsString::from("RUSTFLAGS"), OsString::from("-Copt-level=2")),
        (
            OsString::from("CARGO_PROFILE_DEV_OPT_LEVEL"),
            OsString::from("1"),
        ),
        (
            OsString::from("CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER"),
            OsString::from("clang"),
        ),
        (OsString::from("CUSTOM_INPUT"), OsString::from("present")),
    ];
    let machine = machine();
    let asked = Asked {
        root: repo.root(),
        config: &config,
        machine: &machine,
        vars: &vars.into_iter().collect::<rust_mutants::vars::Variables>(),
        elsewhere: &[],
    };

    let read = inputs(&asked, Mode::Full, &[], None).expect("the tree reads");
    assert_eq!(
        read.environment,
        [
            ("CARGO_PROFILE_DEV_OPT_LEVEL".to_owned(), "1".to_owned()),
            (
                "CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER".to_owned(),
                "clang".to_owned()
            ),
            ("CUSTOM_INPUT".to_owned(), "present".to_owned()),
            ("RUSTFLAGS".to_owned(), "-Copt-level=2".to_owned()),
        ],
        "the variables enter in name order, so one environment the platform lists in another \
         order is one identity"
    );
    assert!(
        of(&asked, Mode::Full, common(), None)
            .expect("the identity reads")
            .is_known()
    );
}

#[cfg(unix)]
#[test]
fn a_variable_name_that_is_not_text_is_not_guessed_into_the_identity() {
    use std::os::unix::ffi::OsStringExt as _;

    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    let config = Config::default();
    let vars = vec![
        (
            OsString::from_vec(vec![b'R', 0xff]),
            OsString::from("hidden"),
        ),
        (OsString::from("CC"), OsString::from("clang")),
    ];
    let machine = machine();
    let asked = Asked {
        root: repo.root(),
        config: &config,
        machine: &machine,
        vars: &vars.into_iter().collect::<rust_mutants::vars::Variables>(),
        elsewhere: &[],
    };

    assert_eq!(
        inputs(&asked, Mode::Full, &[], None)
            .expect("the tree reads")
            .environment,
        [("CC".to_owned(), "clang".to_owned())]
    );
}

#[test]
fn neither_identity_entry_point_panics_when_the_tree_or_lockfile_cannot_be_read() {
    let container = tempfile::tempdir().expect("tempdir");
    let root_is_a_file = container.path().join("not-a-tree");
    std::fs::write(&root_is_a_file, "not a directory").expect("the file");

    let lock_is_a_directory = container.path().join("tree");
    std::fs::create_dir_all(lock_is_a_directory.join("Cargo.lock"))
        .expect("a directory where a lockfile belongs");

    for root in [&root_is_a_file, &lock_is_a_directory] {
        let config = Config::default();
        let machine = machine();
        let asked = Asked {
            root,
            config: &config,
            machine: &machine,
            vars: &rust_mutants::vars::Variables::empty(),
            elsewhere: &[],
        };
        assert!(
            inputs(&asked, Mode::Full, &[], None).is_err(),
            "an unreadable tree has no input identity"
        );
        assert!(
            of(&asked, Mode::Full, common(), None).is_err(),
            "an unreadable tree has no run identity"
        );
    }
}
