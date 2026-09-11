// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What `[project] exclude` narrows, and what it may not: a file left out of the mutations is still compiled, still run, and still part of what the run is.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::path::Path;

use mjutest_cli::assure::identity::{Asked, Machine, inputs};
use mjutest_cli::config::Config;
use mjutest_cli::evidence::digest::Mode;
use mjutest_devkit::repo::Repo;

const fn machine() -> Machine<'static> {
    Machine {
        toolchain: "rustc 1.98.0",
        platform: "x86_64-unknown-linux-gnu",
    }
}

fn tree_of(root: &Path, config: &Config) -> String {
    let machine = machine();
    let asked = Asked {
        root,
        config,
        machine: &machine,
        vars: &[],
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
