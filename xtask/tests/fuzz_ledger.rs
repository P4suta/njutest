// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The fuzz targets, against the three places that are supposed to name all of them.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own files are not themselves tests"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use njutest_devkit::result::{OptionState, option_state};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(PathBuf::new, Path::to_path_buf)
}

fn read(relative: &str) -> String {
    std::fs::read_to_string(root().join(relative))
        .unwrap_or_else(|error| panic!("{relative}: {error}"))
}

fn directory_entries(directory: &Path) -> Vec<std::fs::DirEntry> {
    std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| {
            entry.unwrap_or_else(|error| panic!("entry under {}: {error}", directory.display()))
        })
        .collect()
}

fn file_type(entry: &std::fs::DirEntry) -> std::fs::FileType {
    entry
        .file_type()
        .unwrap_or_else(|error| panic!("{}: {error}", entry.path().display()))
}

fn file_name(entry: &std::fs::DirEntry) -> String {
    match entry.file_name().into_string() {
        Ok(name) => name,
        Err(name) => panic!(
            "a repository file name is not UTF-8; encoded bytes: {:02x?}",
            name.as_os_str().as_encoded_bytes()
        ),
    }
}

/// Every Rust source under `directory`, however deep.
fn rust_sources(directory: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(next) = pending.pop() {
        for entry in directory_entries(&next) {
            let path = entry.path();
            if file_type(&entry).is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found
}

/// Every crash reproducer kept under `fuzz/regressions`, by target, as the path a test names it by.
fn kept_crashes() -> Vec<(String, Vec<String>)> {
    let kept = root().join("fuzz/regressions");
    match std::fs::symlink_metadata(&kept) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => panic!("{}: {error}", kept.display()),
    }
    let mut found: Vec<(String, Vec<String>)> = directory_entries(&kept)
        .into_iter()
        .map(|entry| {
            let target = file_name(&entry);
            let mut crashes: Vec<String> = directory_entries(&entry.path())
                .iter()
                .map(|crash| format!("fuzz/regressions/{target}/{}", file_name(crash)))
                .collect();
            crashes.sort();
            (target, crashes)
        })
        .collect();
    found.sort();
    found
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
    let sources: BTreeSet<String> = directory_entries(&root().join("fuzz/fuzz_targets"))
        .into_iter()
        .filter_map(|entry| file_name(&entry).strip_suffix(".rs").map(str::to_owned))
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
        .and_then(|rest| rest.split_whitespace().next());
    assert_eq!(
        option_state(runs.as_ref()),
        OptionState::Present,
        "the smoke task names a number of runs"
    );
    let Some(runs) = runs else {
        return;
    };
    assert!(
        readme.contains(&format!("{runs} runs")),
        "fuzz/README.md says something other than {runs} runs, which is what the task does"
    );
}

#[test]
fn every_seed_corpus_belongs_to_a_target_that_still_exists() {
    let named: Vec<String> = declared().into_iter().map(|(name, _bench)| name).collect();
    let seeds = root().join("fuzz/seeds");
    let mut seeded: Vec<String> = directory_entries(&seeds)
        .into_iter()
        .filter(|entry| file_type(entry).is_dir())
        .map(|entry| file_name(&entry))
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
        let held = directory_entries(&seeds.join(name)).len();
        assert!(
            held > 0,
            "and {name} has a seed directory with nothing in it, which is the empty \
             corpus this exists to avoid"
        );
    }
}

#[test]
fn every_kept_crash_belongs_to_a_target_that_still_exists() {
    let named: Vec<String> = declared().into_iter().map(|(name, _bench)| name).collect();
    for (target, crashes) in kept_crashes() {
        assert!(
            named.contains(&target),
            "fuzz/regressions/{target} keeps crashes of a target the fuzz crate does not \
             define, so no run replays them"
        );
        assert!(
            !crashes.is_empty(),
            "fuzz/regressions/{target} is a directory with no crash in it"
        );
    }
}

#[test]
fn every_kept_crash_is_the_input_of_an_ordinary_test() {
    let sources: Vec<String> = ["crates", "xtask"]
        .iter()
        .flat_map(|directory| rust_sources(&root().join(directory)))
        .map(|source| {
            std::fs::read_to_string(&source)
                .unwrap_or_else(|error| panic!("{}: {error}", source.display()))
        })
        .collect();
    let untested: Vec<String> = kept_crashes()
        .into_iter()
        .flat_map(|(_target, crashes)| crashes)
        .filter(|crash| !sources.iter().any(|source| source.contains(crash.as_str())))
        .collect();
    assert!(
        untested.is_empty(),
        "these crashes are kept and no test reads them, so the suite a pull request waits for \
         never asks whether they still crash, and the weekly run that would is not one it \
         waits for: {untested:?}"
    );
}
