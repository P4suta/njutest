// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The seeds whose readers live on this side of the workspace.
//!
//! `crates/njutest-cli/tests/fuzz_seeds.rs` puts every other committed seed to
//! the reader its target uses, and names these three as checked here: the
//! runner may not depend on the engine's command line, and the engine's own
//! recording is read by the engine. A seed a reader refuses is not a seed —
//! a fuzzer given one starts outside the format exactly as it would with
//! nothing.

#![expect(
    clippy::panic,
    reason = "a test reports a setup failure by panicking, and a seed the reader refuses names itself in the message"
)]

use std::path::{Path, PathBuf};

fn seeds(target: &str) -> Vec<PathBuf> {
    let directory = njutest_devkit::paths::workspace_root()
        .join("fuzz/seeds")
        .join(target);
    let held: Vec<PathBuf> = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{target}: {error}"))
        .flatten()
        .map(|entry| entry.path())
        .collect();
    assert!(!held.is_empty(), "{target} has no seeds");
    held
}

#[test]
fn every_run_report_seed_is_a_document_the_reader_takes() {
    for seed in seeds("run_report") {
        let text = std::fs::read_to_string(&seed)
            .unwrap_or_else(|error| panic!("{}: {error}", seed.display()));
        serde_json::from_str::<rust_mutants_cli::report::run::RunDocument>(&text)
            .unwrap_or_else(|error| panic!("{}: {error}", seed.display()));
    }
}

#[test]
fn every_engine_configuration_seed_is_one_the_reader_takes() {
    for seed in seeds("engine_config") {
        let text = std::fs::read_to_string(&seed)
            .unwrap_or_else(|error| panic!("{}: {error}", seed.display()));
        rust_mutants_cli::config::Config::parse(&text, Path::new(".rust-mutants.toml"))
            .unwrap_or_else(|error| panic!("{}: {error}", seed.display()));
    }
}

#[test]
fn every_recording_seed_is_one_the_reader_takes() {
    for seed in seeds("trace_reader") {
        let data =
            std::fs::read(&seed).unwrap_or_else(|error| panic!("{}: {error}", seed.display()));
        let events = rust_mutants::trace::read_events(data.as_slice())
            .unwrap_or_else(|error| panic!("{}: {error}", seed.display()));
        assert!(
            !events.is_empty(),
            "{} reads back as no events at all, which is what an empty file does",
            seed.display()
        );
    }
}
