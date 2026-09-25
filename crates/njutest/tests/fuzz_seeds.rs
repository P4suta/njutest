// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every committed seed is one its target's reader accepts.

#![expect(
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking and reads the repository's own files"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use rust_mutants::rule::{Registry, Tier};

static REGISTRY: Registry = Registry::canonical();

/// Whether one seed is a document its reader takes.
type Reader = fn(&[u8]) -> bool;

/// Every seed corpus this suite can put to a reader, with the reader the target uses.
const READERS: [(&str, Reader); 16] = [
    ("annotations", |data| {
        let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, Tier::All);
        rust_mutants::syntax::discover_file("src/lib.rs", data, &selection).is_ok()
    }),
    ("cargo_config", |data| {
        std::str::from_utf8(data)
            .is_ok_and(|text| !rust_mutants::cargo::config::read(text).unreadable)
    }),
    ("cargo_messages", |data| {
        rust_mutants::cargo::parse_messages(data).is_ok_and(|found| !found.is_empty())
    }),
    ("cargo_metadata", |data| {
        rust_mutants::cargo::Metadata::parse(data).is_ok()
    }),
    ("carried_answers", |data| {
        std::str::from_utf8(data).is_ok_and(|text| {
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .all(|line| njutest::report::json::parse(line).is_ok())
        })
    }),
    ("config", |data| {
        std::str::from_utf8(data).is_ok_and(|text| {
            njutest::config::Config::parse(text, Path::new(".njutest.toml")).is_ok()
        })
    }),
    ("coverage_export", |data| {
        njutest::coverage::parse_export(data).is_ok_and(|files| !files.is_empty())
    }),
    ("depinfo", |data| {
        std::str::from_utf8(data).is_ok_and(|text| {
            rust_mutants::cargo::parse_dep_info(text).is_ok_and(|paths| !paths.is_empty())
        })
    }),
    ("discover_file", |data| {
        let selection = rust_mutants::syntax::Selection::tier(&REGISTRY, Tier::All);
        rust_mutants::syntax::discover_file("src/lib.rs", data, &selection).is_ok()
    }),
    ("engine_coverage_export", |data| {
        rust_mutants::coverage::parse_export(data).is_ok_and(|files| !files.is_empty())
    }),
    ("libtest_lines", |data| {
        rust_mutants::execute::parse_lines(data).is_ok_and(|lines| !lines.is_empty())
    }),
    ("libtest_summary", |data| {
        rust_mutants::execute::parse_summary(data).is_ok_and(|summary| summary.is_some())
    }),
    ("model_result", |data| {
        njutest::testkit::model_result(data) != njutest::testkit::ModelResultClass::Undecided
    }),
    ("offered_candidates", |data| {
        std::str::from_utf8(data).is_ok_and(|text| {
            njutest::repair::take(text, Path::new("/w"), &njutest::repair::allowed(&[])).is_ok()
        })
    }),
    ("report_document", |data| {
        std::str::from_utf8(data).is_ok_and(|text| njutest::report::json::parse(text).is_ok())
    }),
    ("touch_log", |data| {
        std::str::from_utf8(data).is_ok_and(|text| {
            rust_mutants::touch::read(
                text,
                &"0".repeat(64),
                rust_mutants::touch::Bounds {
                    mutants: 4096,
                    items: 4096,
                },
            )
            .is_ok()
        })
    }),
];

/// The seed directories this suite cannot reach, and why.
const ELSEWHERE: [&str; 3] = ["run_report", "engine_config", "trace_reader"];

fn seeds() -> PathBuf {
    njutest_devkit::paths::workspace_root().join("fuzz/seeds")
}

fn corpora() -> BTreeSet<String> {
    std::fs::read_dir(seeds())
        .expect("the seed corpora")
        .map(|entry| entry.expect("every seed-corpus entry is readable"))
        .filter(|entry| entry.path().is_dir())
        .map(|entry| {
            entry
                .file_name()
                .to_str()
                .expect("test protocol paths are UTF-8")
                .to_owned()
        })
        .collect()
}

/// The seeds spell every path with a leading slash, which is absolute where the fuzzer runs and not where absolute means a drive, and no one spelling is both.
/// What a seed is for is giving a fuzz target a document it takes, and the targets are built and run on unix.
#[cfg(unix)]
#[test]
fn every_seed_is_a_document_the_reader_it_is_for_accepts() {
    for (target, reader) in READERS {
        let directory = seeds().join(target);
        let held: Vec<PathBuf> = std::fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("{target}: {error}"))
            .map(|entry| entry.unwrap_or_else(|error| panic!("{target}: {error}")))
            .map(|entry| entry.path())
            .collect();
        assert!(!held.is_empty(), "{target} has no seeds");
        for seed in held {
            let data = std::fs::read(&seed).expect("a seed");
            assert!(
                reader(&data),
                "{} is not a document its reader takes, so a fuzzer given it starts \
                 outside the format exactly as it would with nothing: a seed the reader \
                 refuses is not a seed",
                seed.display()
            );
        }
    }
}

#[test]
fn every_seed_corpus_is_one_something_checks() {
    let checked: BTreeSet<String> = READERS
        .iter()
        .map(|(target, _reader)| (*target).to_owned())
        .chain(ELSEWHERE.iter().map(|target| (*target).to_owned()))
        .collect();
    let unchecked: Vec<String> = corpora().difference(&checked).cloned().collect();
    assert!(
        unchecked.is_empty(),
        "these seed corpora are committed and nothing puts them to a reader, so nobody \
         would notice them going stale: {unchecked:?}"
    );
}
