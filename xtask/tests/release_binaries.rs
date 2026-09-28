// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The binaries the workspace declares, against what decides what a person can install: the one command that bundles them, and where binstall fetches the bundle.

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

/// Every crate of the workspace a person can install, as the path to it from the root.
fn shipped() -> Vec<String> {
    let found: Vec<String> = njutest_devkit::census::members(&root())
        .into_iter()
        .filter(|member| member.published && !member.binaries.is_empty())
        .map(|member| match member.directory.strip_prefix(root()) {
            Ok(at) => at.display().to_string(),
            Err(error) => panic!("{} is not under the workspace: {error}", member.name),
        })
        .collect();
    assert!(
        !found.is_empty(),
        "a release publishes what the workspace declares, and reading that from cargo is \
         what makes the crate somebody adds next arrive in these checks on its own"
    );
    found
}

/// Every crate of the workspace that declares one, and the binaries it declares.
fn declared() -> Vec<(String, BTreeSet<String>)> {
    let mut found = Vec::new();
    for member in shipped() {
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
        found.push((member, names));
    }
    found
}

/// The body of the release workflow's job `name`, up to the next job.
fn release_job(name: &str) -> String {
    let workflow = read(".github/workflows/release.yml");
    let header = format!("\n  {name}:\n");
    let at = workflow
        .find(&header)
        .unwrap_or_else(|| panic!("release.yml has no job {name}"));
    let body = workflow
        .get(at.saturating_add(header.len())..)
        .unwrap_or_default();
    let end = body
        .lines()
        .take_while(|line| line.is_empty() || line.starts_with("   "))
        .map(|line| line.len().saturating_add(1))
        .sum::<usize>()
        .min(body.len());
    body.get(..end).unwrap_or_default().to_owned()
}

#[test]
fn the_release_decides_what_it_holds_only_by_the_command_that_reads_the_manifests() {
    let workflow = read(".github/workflows/release.yml");
    let hand_kept: Vec<&str> = [
        "for binary in",
        "target/release",
        "cargo build",
        "cp LICENSE",
        "tar --create",
        "shasum",
    ]
    .into_iter()
    .filter(|spelling| workflow.contains(spelling))
    .collect();
    assert!(
        hand_kept.is_empty(),
        "release.yml decides by itself what the release holds ({hand_kept:?}): which \
         binaries, where each goes, and what goes beside them are what the manifests \
         declare and `cargo xtask bundle` reads, and a second list in the workflow is \
         one that drifts from them"
    );
    let artifacts = release_job("artifacts");
    for held in [
        "TARGET: ${{ matrix.target }}",
        "run: cargo xtask bundle --target \"${TARGET}\" --out dist",
        "path: dist/*.tar.gz*",
    ] {
        assert!(
            artifacts.contains(held),
            "the artifacts job bundles each target with the one command and uploads the \
             archive and its checksum it wrote ({held:?}): {artifacts}"
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
fn binstall_fetches_the_archive_from_the_release_its_tag_names() {
    let check = release_job("check");
    assert!(
        check.contains("if [ \"v${version}\" != \"${TAG}\" ]; then"),
        "the release refuses a tag that does not name the manifests' version: {check}"
    );
    let publish = release_job("publish");
    assert!(
        publish.contains("gh release create \"${TAG}\"") && publish.contains("dist/*"),
        "and publishes what it built to the release that tag names: {publish}"
    );
    for member in shipped() {
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
        let url = stanza
            .lines()
            .find_map(|line| line.strip_prefix("pkg-url = \""))
            .unwrap_or_else(|| panic!("{member} binstall stanza names no pkg-url"));
        assert!(
            url.starts_with("{ repo }/releases/download/v{ version }/"),
            "{member} tells binstall to fetch from somewhere other than the release its \
             version's tag publishes, so the install fails on a URL nobody will think to \
             check: {url}"
        );
    }
}

#[test]
fn every_benchmark_the_workspace_declares_is_one_the_task_runs() {
    let mut declared: BTreeSet<(String, String)> = BTreeSet::new();
    for member in njutest_devkit::census::members(&root()) {
        let manifest = std::fs::read_to_string(member.directory.join("Cargo.toml"))
            .unwrap_or_else(|error| panic!("{}: {error}", member.name));
        let package = member.name.as_str();
        for stanza in manifest.split("[[bench]]").skip(1) {
            let Some(name) = stanza
                .lines()
                .find_map(|line| line.strip_prefix("name = \""))
                .and_then(|value| value.split('"').next())
            else {
                continue;
            };
            assert!(
                declared.insert((package.to_owned(), name.to_owned())),
                "{package}/{name} is declared only once"
            );
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
