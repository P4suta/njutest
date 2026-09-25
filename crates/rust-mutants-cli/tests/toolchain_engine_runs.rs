// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The engine runs committed beside the engine audit's tests, held to the shape today's engine records for the same fixtures.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use njutest_devkit::fixture::Fixture;

/// The variable that records the committed runs again rather than refusing a difference, as `UPDATE_GOLDEN` does for a golden.
const UPDATE: &str = "UPDATE_ENGINE_RUNS";

/// Each committed run, by the directory it is kept in, and the fixture it is a run of.
const SAMPLES: [(&str, &str); 3] = [
    ("engine-run-simple", "fixture-simple"),
    ("engine-run-rejected", "fixture-rejectable"),
    ("engine-run-unreached", "fixture-unreached"),
];

/// Every document a run left at the top of `directory`, by name, which is every document a committed run keeps: a list written here would miss the next one the engine learns to write.
fn documents(directory: &Path) -> BTreeSet<String> {
    std::fs::read_dir(directory)
        .expect("the run's directory")
        .map(|entry| {
            entry
                .expect("an entry of the run's directory")
                .file_name()
                .into_string()
                .expect("a document is named in UTF-8")
        })
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .collect()
}

/// The recording a run keeps, beside its documents.
const RECORDING: &str = "trace/trace.jsonl";

/// Where the committed runs are kept.
fn committed(name: &str) -> PathBuf {
    njutest_devkit::paths::workspace_root()
        .join("xtask/tests/testdata")
        .join(name)
}

/// A run of `fixture` as the committed ones are recorded: every tier, offline, locked, with its recording, under the least of this environment a nested run needs.
fn recorded(fixture: &Fixture) -> PathBuf {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env_clear();
    command.envs(njutest_devkit::paths::environment_for_a_toolchain_run(&[]));
    command.env("NO_COLOR", "1");
    command.env("TMPDIR", fixture.temp());
    command.env("XDG_CACHE_HOME", fixture.cache());
    command.arg("run");
    command.args(["--root", njutest_devkit::paths::utf8(fixture.root())]);
    command.args(["--tier", "all", "--offline", "--locked", "--trace"]);
    let output = command.output().expect("rust-mutants runs");
    assert!(
        matches!(output.status.code(), Some(0..=2)),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
    )
}

/// Every path in `value`, each ending in the kind of what is there, with every element of an array at one path.
fn shape(value: &serde_json::Value, at: &str, into: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::Object(fields) => {
            into.insert(format!("{at}:object"));
            for (name, field) in fields {
                shape(field, &format!("{at}/{name}"), into);
            }
        }
        serde_json::Value::Array(items) => {
            into.insert(format!("{at}:array"));
            for item in items {
                shape(item, &format!("{at}/[]"), into);
            }
        }
        serde_json::Value::Null => {
            into.insert(format!("{at}:null"));
        }
        serde_json::Value::Bool(_) => {
            into.insert(format!("{at}:bool"));
        }
        serde_json::Value::Number(_) => {
            into.insert(format!("{at}:number"));
        }
        serde_json::Value::String(_) => {
            into.insert(format!("{at}:string"));
        }
    }
}

/// The shape of every document and of every kind of event the run in `directory` kept.
fn shapes(directory: &Path) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for document in documents(directory) {
        found.insert(format!("{document}:kept"));
        let text = std::fs::read_to_string(directory.join(&document)).expect("a document");
        let value: serde_json::Value =
            njutest_devkit::strictjson::decode_str(&text).expect("a document is JSON");
        shape(&value, &document, &mut found);
    }
    let recording = std::fs::read_to_string(directory.join(RECORDING)).expect("the recording");
    for line in recording.lines().filter(|line| !line.trim().is_empty()) {
        let event: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("an event is JSON");
        let kind = event
            .pointer("/payload/type")
            .and_then(serde_json::Value::as_str)
            .expect("an event names its type")
            .to_owned();
        shape(&event, &format!("{RECORDING}#{kind}"), &mut found);
    }
    found
}

/// Replaces the committed run `name` with the one in `fresh`, its documents and its recording, and none of the output it kept; a document the engine no longer writes goes.
fn rewrite(name: &str, fresh: &Path) {
    let into = committed(name);
    let written = documents(fresh);
    for gone in documents(&into).difference(&written) {
        std::fs::remove_file(into.join(gone)).expect("a document the engine no longer writes");
    }
    for document in written.iter().map(String::as_str).chain([RECORDING]) {
        let to = into.join(document);
        std::fs::create_dir_all(to.parent().expect("a parent")).expect("the directory");
        std::fs::copy(fresh.join(document), &to).expect("the committed copy");
    }
}

#[test]
fn every_committed_engine_run_has_the_shape_todays_engine_records() {
    let updating = std::env::var_os(UPDATE).is_some();
    let mut stale = Vec::new();
    for (name, fixture) in SAMPLES {
        let fixture = Fixture::copy(fixture);
        let fresh = recorded(&fixture);
        if updating {
            rewrite(name, &fresh);
            continue;
        }
        let (kept, today) = (shapes(&committed(name)), shapes(&fresh));
        if kept != today {
            stale.push(format!(
                "{name}\n  only in the committed run:\n    {}\n  only in today's:\n    {}",
                kept.difference(&today)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n    "),
                today
                    .difference(&kept)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "a committed engine run the engine no longer records is a sample the audit re-decides \
         for nobody: read the difference, then record them again with {UPDATE}=1:\n{}",
        stale.join("\n")
    );
}
