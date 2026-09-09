// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fuzz targets, against the three places that are supposed to name all of them.

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

/// Every target the fuzz crate declares, and whether its stanza keeps it out of `cargo bench`.
fn declared() -> Vec<(String, bool)> {
    let manifest = read("fuzz/Cargo.toml");
    manifest
        .split("[[bin]]")
        .skip(1)
        .filter_map(|stanza| {
            let name = stanza
                .lines()
                .find_map(|line| line.strip_prefix("name = \""))?
                .split('"')
                .next()?
                .to_owned();
            Some((name, stanza.contains("bench = false")))
        })
        .collect()
}

#[test]
fn every_fuzz_target_is_a_source_file_a_readme_row_and_a_workflow_matrix_entry() {
    let declared = declared();
    let names: BTreeSet<&str> = declared.iter().map(|(name, _)| name.as_str()).collect();
    let sources: BTreeSet<String> = std::fs::read_dir(root().join("fuzz/fuzz_targets"))
        .expect("the fuzz targets")
        .flatten()
        .filter_map(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .strip_suffix(".rs")
                .map(str::to_owned)
        })
        .collect();
    let orphans: Vec<&String> = sources
        .iter()
        .filter(|name| !names.contains(name.as_str()))
        .collect();
    assert!(
        orphans.is_empty(),
        "these targets have a source file and no stanza, so nothing builds them: {orphans:?}"
    );

    let readme = read("fuzz/README.md");
    let workflow = read(".github/workflows/fuzz.yml");
    for (name, _) in &declared {
        assert!(
            sources.contains(name),
            "the manifest declares {name} and no source file holds it"
        );
        assert!(
            readme.contains(&format!("| `{name}` |")),
            "fuzz/README.md does not say what {name} proves"
        );
        assert!(
            workflow.contains(&format!("- {name}\n")),
            "the weekly fuzz job does not run {name}"
        );
    }
}

#[test]
fn no_fuzz_target_is_a_benchmark() {
    let without: Vec<String> = declared()
        .into_iter()
        .filter_map(|(name, benched)| (!benched).then_some(name))
        .collect();
    assert!(
        without.is_empty(),
        "these stanzas leave `bench = false` out, so `cargo bench` builds a fuzz target with a \
         sanitizer it has no toolchain for: {without:?}"
    );
}

#[test]
fn the_readme_says_the_number_of_runs_the_smoke_task_actually_does() {
    let readme = read("fuzz/README.md");
    let mise = read("mise.toml");
    let runs = mise
        .split("-runs=")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .expect("the smoke task names a number of runs");
    assert!(
        readme.contains(&format!("{runs} runs")),
        "fuzz/README.md says something other than {runs} runs, which is what the task does"
    );
}

#[test]
fn every_seed_corpus_belongs_to_a_target_that_still_exists() {
    let named: Vec<String> = declared().into_iter().map(|(name, _bench)| name).collect();
    let seeds = root().join("fuzz/seeds");
    let mut seeded: Vec<String> = std::fs::read_dir(&seeds)
        .expect("the seed corpora")
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    seeded.sort();
    assert!(
        !seeded.is_empty(),
        "a target whose input is a document explores nothing from an empty corpus, and \
         fuzz/corpus is not committed: random bytes are not JSON, so a nightly run of \
         one starts where the last one started. Seeds are what make it a run"
    );
    let orphaned: Vec<&String> = seeded.iter().filter(|name| !named.contains(name)).collect();
    assert!(
        orphaned.is_empty(),
        "these seed corpora name targets the fuzz crate does not define, so nothing \
         will ever read them: {orphaned:?}"
    );
    for name in &seeded {
        let held = std::fs::read_dir(seeds.join(name))
            .expect("one seed corpus")
            .flatten()
            .count();
        assert!(
            held > 0,
            "and {name} has a seed directory with nothing in it, which is the empty \
             corpus this exists to avoid"
        );
    }
}
