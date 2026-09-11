// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The binaries the workspace declares, against the three places that decide what a person can install.
//!
//! A binary that is built and not bundled is one `cargo binstall` looks for in
//! an archive that does not carry it, and the failure arrives at the person
//! installing rather than at the release that made it. So the manifests, the
//! archive the release workflow builds, and the version check it makes are
//! held to each other here, where a change to one of them is refused before
//! it is tagged.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own files are not themselves tests"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// Every crate of the workspace that declares one, and the binaries it declares.
fn declared() -> Vec<(String, BTreeSet<String>)> {
    let mut found = Vec::new();
    for member in ["crates/mjutest-cli", "crates/rust-mutants-cli"] {
        let manifest = read(&format!("{member}/Cargo.toml"));
        let names: BTreeSet<String> = manifest
            .split("[[bin]]")
            .skip(1)
            .filter_map(|stanza| {
                Some(
                    stanza
                        .lines()
                        .find_map(|line| line.strip_prefix("name = \""))?
                        .split('"')
                        .next()?
                        .to_owned(),
                )
            })
            .collect();
        found.push((member.to_owned(), names));
    }
    found
}

/// The binaries one `for binary in ...; do` of the release workflow names.
fn listed(after: &str) -> BTreeSet<String> {
    let workflow = read(".github/workflows/release.yml");
    let at = workflow
        .find(after)
        .unwrap_or_else(|| panic!("release.yml no longer holds {after:?}"));
    let rest = workflow.get(at..).unwrap_or_default();
    let start = rest
        .find("for binary in ")
        .unwrap_or_else(|| panic!("no binary list after {after:?}"));
    let line = rest
        .get(start..)
        .and_then(|from| from.split('\n').next())
        .unwrap_or_default();
    line.trim_start_matches("for binary in ")
        .trim_end_matches("; do")
        .split_whitespace()
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn every_binary_the_workspace_declares_is_one_the_release_bundles_and_checks() {
    let every: BTreeSet<String> = declared()
        .into_iter()
        .flat_map(|(_member, names)| names)
        .collect();
    assert!(
        every.len() >= 4,
        "two products, each with its own name and the name cargo looks for: {every:?}"
    );

    for (what, after) in [
        (
            "the version check",
            "The tag, the manifests, and the binaries",
        ),
        ("the archive", "Bundle them"),
    ] {
        assert_eq!(
            listed(after),
            every,
            "{what} names a different set of binaries than the manifests declare: a \
             binary that is built and not bundled is one `cargo binstall` looks for in \
             an archive that does not carry it"
        );
    }
}

#[test]
fn both_products_answer_to_the_name_cargo_looks_for() {
    for (member, names) in declared() {
        let own = names
            .iter()
            .find(|name| !name.starts_with("cargo-"))
            .unwrap_or_else(|| panic!("{member} declares a binary of its own"));
        assert!(
            names.contains(&format!("cargo-{own}")),
            "a person who installed {own} types `cargo {own}` before they type {own}, \
             and cargo finds a subcommand by the name `cargo-{own}`: {member} declares \
             {names:?}"
        );
    }
}

#[test]
fn what_binstall_looks_in_is_the_archive_the_release_builds() {
    let archive = {
        let workflow = read(".github/workflows/release.yml");
        let at = workflow
            .find("name=\"mjutest-${VERSION}-${TARGET}\"")
            .map(|_found| "mjutest-{ version }-{ target }");
        at.unwrap_or_else(|| panic!("release.yml no longer names the archive it builds"))
    };
    for member in ["crates/mjutest-cli", "crates/rust-mutants-cli"] {
        let manifest = read(&format!("{member}/Cargo.toml"));
        let stanza = manifest
            .split("[package.metadata.binstall]")
            .nth(1)
            .unwrap_or_else(|| {
                panic!(
                    "{member} has no [package.metadata.binstall], so `cargo binstall` \
                     compiles the workspace instead of taking what the release published"
                )
            });
        let directory = stanza
            .lines()
            .find_map(|line| line.strip_prefix("bin-dir = \""))
            .and_then(|value| value.split('/').next())
            .unwrap_or_else(|| panic!("{member} binstall stanza names no bin-dir"));
        assert_eq!(
            directory, archive,
            "{member} tells binstall to look in a directory the release does not build, \
             so the install fails on a URL nobody will think to check"
        );
        assert!(
            stanza.contains(&format!("/{archive}.tar.gz\"")),
            "and at an archive it does build: {stanza}"
        );
    }
}

#[test]
fn every_benchmark_the_workspace_declares_is_one_the_task_runs() {
    let mut declared: BTreeSet<(String, String)> = BTreeSet::new();
    for member in [
        "crates/mjutest-cli",
        "crates/rust-mutants",
        "crates/mjutest",
    ] {
        let manifest = read(&format!("{member}/Cargo.toml"));
        let package = member
            .rsplit('/')
            .next()
            .unwrap_or_else(|| panic!("{member} names a package"));
        for stanza in manifest.split("[[bench]]").skip(1) {
            let Some(name) = stanza
                .lines()
                .find_map(|line| line.strip_prefix("name = \""))
                .and_then(|value| value.split('"').next())
            else {
                continue;
            };
            let _added = declared.insert((package.to_owned(), name.to_owned()));
        }
    }
    assert!(
        declared.len() >= 4,
        "the workspace declares benchmarks: {declared:?}"
    );

    let task = read("mise.toml");
    let block = task
        .find("[tasks.bench]")
        .and_then(|at| task.get(at..))
        .unwrap_or_else(|| panic!("mise.toml no longer has a bench task"));
    let block = block
        .find("\n[tasks.")
        .map_or(block, |end| block.get(..end).unwrap_or(block));
    let unrun: Vec<&(String, String)> = declared
        .iter()
        .filter(|(package, name)| !block.contains(&format!("-p {package} --bench {name}")))
        .collect();
    assert!(
        unrun.is_empty(),
        "a benchmark nobody runs is a number nobody reads: the stage it measures can get \
         slower every release and the file it lives in still compiles. {unrun:?} is \
         declared and `mise run bench` never asks for it"
    );
}
