// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Content-addressed sealed transcripts across roots and runs, with the host's work counted in the trace.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use njutest_devkit::fixture::copy_tree;

/// A fixture rooted separately from its cache and the temporary directory its runs share.
struct Placed {
    root: PathBuf,
    cache: PathBuf,
}

/// One copy of fixture-simple at `name`, with its own cache.
fn placed(temp: &Path, name: &str) -> Placed {
    let root = temp.join(name);
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-simple"),
        &root,
    );
    Placed {
        root,
        cache: temp.join("cache").join(name),
    }
}

/// The host's actual work, read from the recording rather than inferred from the verdict.
#[derive(Debug, PartialEq, Eq)]
struct Spent {
    compiles: u64,
    instances: u64,
    answered: u64,
}

/// Every mutant execution's transcript and the host's accounting for the run that made them.
struct Observed {
    transcripts: BTreeMap<(String, String, String), String>,
    spent: Spent,
}

/// A whole real run, with or without the cache, under the least environment a nested run needs.
fn ran(placed: &Placed, temp: &Path, no_cache: bool) -> Observed {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_rust-mutants")));
    command.env_clear();
    command.envs(njutest_devkit::paths::environment_for_a_toolchain_run(&[]));
    command.env("NO_COLOR", "1");
    command.envs(njutest_devkit::paths::temporary_directory(temp));
    command.env("XDG_CACHE_HOME", &placed.cache);
    command.args(["run", "--root", njutest_devkit::paths::utf8(&placed.root)]);
    command.args(["--tier", "all", "--offline", "--locked", "--trace"]);
    if no_cache {
        command.arg("--no-cache");
    }
    let output = command.output().expect("rust-mutants runs");
    assert_eq!(
        output.status.code(),
        Some(1),
        "fixture-simple's twelve kills and one survivor are established sealed:\n{}{}",
        njutest_devkit::process::strict_utf8(&output.stdout),
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let run = njutest_devkit::fixture::newest_run(
        &rust_mutants_cli::app::stored::Store::read(&placed.root).root(),
    );
    let recording = std::fs::read_to_string(run.join("trace/trace.jsonl")).expect("the recording");
    let mut transcripts = BTreeMap::new();
    let mut spent = None;
    for line in recording.lines().filter(|line| !line.trim().is_empty()) {
        let event: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("an event is JSON");
        match event
            .pointer("/payload/type")
            .and_then(serde_json::Value::as_str)
        {
            Some("sealed-exec") => {
                let sealed = event.pointer("/payload/sealed").expect("the execution");
                let read = |field: &str| {
                    sealed
                        .get(field)
                        .and_then(serde_json::Value::as_str)
                        .expect("every sealed execution names its transcript and inputs")
                        .to_owned()
                };
                let old = transcripts.insert(
                    (read("mutant"), read("target"), read("test")),
                    read("transcript"),
                );
                assert!(old.is_none(), "one execution is recorded once");
            }
            Some("run-end") => {
                let sealed = event
                    .pointer("/payload/run/sealed")
                    .expect("a run that seals counts compiles, instances and cache answers");
                let read = |field: &str| {
                    sealed
                        .get(field)
                        .and_then(serde_json::Value::as_u64)
                        .expect("the host counts this work")
                };
                spent = Some(Spent {
                    compiles: read("compiles"),
                    instances: read("instances"),
                    answered: read("answered"),
                });
            }
            Some(_) | None => {}
        }
    }
    assert_eq!(
        transcripts.len(),
        13,
        "every README verdict has one sealed execution"
    );
    Observed {
        transcripts,
        spent: spent.expect("the recording ends"),
    }
}

#[test]
fn one_execution_of_one_tree_means_the_same_from_any_root() {
    let temp = tempfile::Builder::new()
        .prefix("njutest-sealed-memory-")
        .tempdir()
        .expect("a temporary directory");
    let first = placed(temp.path(), "first");
    let second = placed(temp.path(), "second");
    assert_ne!(
        first.root, second.root,
        "the comparison uses two distinct roots"
    );
    let here = ran(&first, temp.path(), true);
    let there = ran(&second, temp.path(), true);
    assert_eq!(
        here.transcripts, there.transcripts,
        "the same module bytes and canonical guest paths give identical transcripts from two roots"
    );
    assert_eq!(
        here.spent, there.spent,
        "both roots execute the same work without caching"
    );
    assert!(here.spent.compiles > 0 && here.spent.instances > 13);
    assert_eq!(
        here.spent.answered, 0,
        "--no-cache executes every invocation"
    );
}

#[test]
fn a_later_run_of_one_tree_is_answered_from_what_it_remembered() {
    let temp = tempfile::Builder::new()
        .prefix("njutest-sealed-memory-")
        .tempdir()
        .expect("a temporary directory");
    let fixture = placed(temp.path(), "first");
    let first = ran(&fixture, temp.path(), false);
    assert!(first.spent.compiles > 0 && first.spent.instances > 13);
    assert_eq!(first.spent.answered, 0, "a cold store answers nothing");
    let cache = fixture.cache.join("rust-mutants/sealed-transcripts-v1");
    assert!(
        std::fs::metadata(&cache)
            .expect("the transcript store")
            .is_dir(),
        "transcripts live at the documented cache path"
    );
    assert!(
        std::fs::read_dir(&cache).expect("the store").all(|entry| {
            entry
                .expect("a record")
                .path()
                .extension()
                .is_some_and(|extension| extension == "json")
        }),
        "the store holds whole transcript records directly, without another layout directory"
    );
    let second = ran(&fixture, temp.path(), false);
    assert_eq!(second.transcripts, first.transcripts);
    assert_eq!(second.spent.compiles, first.spent.compiles);
    assert_eq!(
        second.spent.instances, 0,
        "every warm invocation is memoized"
    );
    assert_eq!(
        second.spent.answered, first.spent.instances,
        "the hit rate is 100% of the first run's listing, controls and mutant executions"
    );
    let before = njutest_devkit::fixture::fingerprint(&cache);
    let fresh = ran(&fixture, temp.path(), true);
    assert_eq!(fresh.transcripts, first.transcripts);
    assert_eq!(
        fresh.spent, first.spent,
        "--no-cache starts every instance again"
    );
    assert_eq!(
        njutest_devkit::fixture::fingerprint(&cache),
        before,
        "--no-cache leaves the transcript store untouched"
    );
}
