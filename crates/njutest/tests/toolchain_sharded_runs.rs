// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The sharded runs and the proofaudit specimen committed beside the audits that read them, held to the shape today's runner records for the same fixtures.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::disallowed_methods,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use njutest_devkit::fixture::copy_tree;

/// The variable that records the committed sharded runs again rather than refusing a difference, as `UPDATE_ENGINE_RUNS` does for the engine's.
const UPDATE_RUNS: &str = "UPDATE_SHARDED_RUNS";

/// The variable that records the proofaudit specimen report again from a run of today's runner.
const UPDATE_SPECIMEN: &str = "UPDATE_SPECIMEN_REPORT";

/// Each committed recording, by the directory it is kept in, and whether its runs sealed.
const RECORDINGS: [(&str, bool); 2] = [("sharded-run", true), ("sharded-run-unsealed", false)];

/// The part's own row collections, emptied when a run's complete document becomes the specimen.
const ROWS: [&str; 15] = [
    "resources",
    "candidates",
    "seams",
    "faults",
    "beside",
    "crashes",
    "targets",
    "sources",
    "mutants",
    "findings",
    "limitations",
    "drift",
    "repaired",
    "knobs",
    "concurrency",
];

/// A throwaway copy of `fixture-assured`, the fixture the recordings are runs of, kept for the length of the test.
struct Fixture {
    root: PathBuf,
    _dir: tempfile::TempDir,
}

fn fixture() -> Fixture {
    let dir = tempfile::Builder::new()
        .prefix("njutest-sharded-runs-")
        .tempdir()
        .expect("a temporary directory");
    let root = dir.path().join("fixture-assured");
    copy_tree(
        &njutest_devkit::paths::fixtures_dir().join("fixture-assured"),
        &root,
    );
    njutest_devkit::fixture::pin_contract(&root, "standard-v1");
    Fixture { root, _dir: dir }
}

/// One command, the real binary with the least environment a nested run needs, saying its exit code and both streams.
fn asked(root: &Path, args: &[&str]) -> (u8, String, String) {
    let mut command = njutest_devkit::paths::command(Path::new(env!("CARGO_BIN_EXE_njutest")));
    command.env_clear();
    command.envs(njutest_devkit::paths::environment_for_a_toolchain_run(&[]));
    command.env("NO_COLOR", "1");
    command.envs(njutest_devkit::paths::temporary_directory(
        &njutest_devkit::paths::temp_beside(root).expect("a temporary directory"),
    ));
    command.env(
        "XDG_CACHE_HOME",
        njutest_devkit::paths::cache_beside(root).expect("a cache directory"),
    );
    command.current_dir(root);
    command.args(args);
    let output = command.output().expect("njutest runs");
    let code = output
        .status
        .code()
        .expect("njutest ends inside the exit table");
    (
        u8::try_from(code).expect("an exit code this table holds"),
        njutest_devkit::process::strict_utf8(&output.stdout).into_owned(),
        njutest_devkit::process::strict_utf8(&output.stderr).into_owned(),
    )
}

/// The document a run or a merge left at `path`, parsed.
fn document(path: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(path).expect("a document a recording keeps");
    njutest_devkit::strictjson::decode_str(&text).expect("a recording's document is JSON")
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

/// The shapes of every kind of event one recording file holds, laid at `spelled`.
fn events(path: &Path, spelled: &str) -> BTreeSet<String> {
    let text = std::fs::read_to_string(path).expect("a recording a run keeps");
    let mut found = BTreeSet::new();
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let event: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("a recording's events are JSON");
        let kind = event
            .pointer("/payload/type")
            .and_then(serde_json::Value::as_str)
            .expect("an event names its type")
            .to_owned();
        shape(&event, &format!("{spelled}#{kind}"), &mut found);
    }
    found
}

/// One recorded sharded run: its two shard run directories, its merged document, and the directory its recordings are kept under, by run.
struct Recorded {
    traces: PathBuf,
    shards: Vec<PathBuf>,
    merged: PathBuf,
}

impl Recorded {
    /// The shapes of the recording, run identities and builds spelled `<run>` and `<build>` so two recordings of the same fixture compare.
    fn shapes(&self) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        shape(&document(&self.merged), "merged.json", &mut found);
        for shard in &self.shards {
            shape(
                &document(&shard.join(njutest::app::reports::DOCUMENT_NAME)),
                "runs/<run>/report",
                &mut found,
            );
            let trace = self.trace_of(shard, Path::new("trace.jsonl").to_path_buf());
            found.extend(events(&trace, "traces/<run>/trace.jsonl"));
            for engine in self.engines_of(shard) {
                found.extend(events(
                    &engine,
                    "traces/<run>/builds/<build>/engine/trace.jsonl",
                ));
            }
        }
        found
    }

    /// One file of `shard`'s recording, at `spelled` under the recordings' directory.
    fn trace_of(&self, shard: &Path, spelled: PathBuf) -> PathBuf {
        self.traces
            .join(
                shard
                    .file_name()
                    .expect("a run directory is named for its run"),
            )
            .join(spelled)
    }

    /// Every engine recording `shard`'s run kept, one per build.
    fn engines_of(&self, shard: &Path) -> Vec<PathBuf> {
        let builds = self.trace_of(shard, Path::new("builds").to_path_buf());
        let mut found: Vec<PathBuf> = std::fs::read_dir(&builds)
            .expect("the run kept its builds' recordings")
            .map(|entry| {
                entry
                    .expect("a build's recording")
                    .path()
                    .join(njutest::app::trace::ENGINE_DIRECTORY)
                    .join("trace.jsonl")
            })
            .collect();
        found.sort();
        found
    }
}

/// Records `fixture` as the committed recording `name` is: two shards and a merge, sealed where `sealed` says.
fn recorded(fixture: &Fixture, sealed: bool) -> Recorded {
    let mut verify = vec!["verify", "--offline", "--locked", "--trace", "--ui=plain"];
    if !sealed {
        verify.push("--no-seal");
    }
    let mut shards = Vec::new();
    for shard in ["1/2", "2/2"] {
        let (code, out, err) = asked(
            &fixture.root,
            &[verify.as_slice(), &["--shard", shard]].concat(),
        );
        assert_eq!(
            code,
            njutest::cli::EXIT_INSUFFICIENT,
            "a part of a catalog assures nothing on its own: {out}{err}"
        );
        shards.push(newest_run(&fixture.root));
    }
    let merged = fixture.root.join("merged.json");
    let (code, out, err) = asked(
        &fixture.root,
        &[
            "merge",
            &shards[0]
                .join(njutest::app::reports::DOCUMENT_NAME)
                .display()
                .to_string(),
            &shards[1]
                .join(njutest::app::reports::DOCUMENT_NAME)
                .display()
                .to_string(),
            "--output",
            &merged.display().to_string(),
        ],
    );
    assert_eq!(
        code,
        if sealed {
            njutest::cli::EXIT_ASSURED
        } else {
            njutest::cli::EXIT_INSUFFICIENT
        },
        "a merge of two parts of one catalog answers what the parts establish: sealed parts \
         establish their verdicts, and parts run with --no-seal establish none, so the whole \
         they make is a lead and not a verdict: {out}{err}\n{}",
        document(&merged)
    );
    Recorded {
        traces: fixture.root.join(".njutest/trace"),
        shards,
        merged,
    }
}

/// The newest run directory under `root`'s reports, which is the run that just answered.
fn newest_run(root: &Path) -> PathBuf {
    std::fs::read_dir(
        root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join("runs"),
    )
    .expect("the run wrote its report")
    .map(|entry| entry.expect("a run directory").path())
    .filter(|path| path.is_dir())
    .max()
    .expect("the run's directory")
}

/// Where the committed recordings are kept.
fn committed(recording: &str) -> PathBuf {
    njutest_devkit::paths::workspace_root()
        .join("xtask/tests/testdata")
        .join(recording)
}

/// The shapes of the committed recording `name`, spelled as a fresh one's are.
fn committed_shapes(name: &str) -> BTreeSet<String> {
    let root = committed(name);
    let shards: Vec<PathBuf> = std::fs::read_dir(root.join("runs"))
        .expect("the committed shard reports")
        .map(|entry| entry.expect("a run directory").path())
        .collect();
    let recorded = Recorded {
        traces: root.join("traces"),
        shards,
        merged: root.join("merged.json"),
    };
    recorded.shapes()
}

/// Replaces the committed recording `name` with `fresh`: its merge, its shard reports and its recordings, and none of what the last recording kept.
fn rewrite(name: &str, fresh: &Recorded) {
    let into = committed(name);
    let runs = into.join("runs");
    std::fs::remove_dir_all(&runs).expect("the committed shard reports go");
    std::fs::create_dir_all(&runs).expect("the runs directory");
    let traces = into.join("traces");
    std::fs::remove_dir_all(&traces).expect("the committed recordings go");
    for shard in &fresh.shards {
        let run = shard.file_name().expect("the run's name");
        std::fs::create_dir_all(runs.join(run)).expect("the committed run directory");
        std::fs::copy(
            shard.join(njutest::app::reports::DOCUMENT_NAME),
            runs.join(run).join(njutest::app::reports::DOCUMENT_NAME),
        )
        .expect("the committed report");
        std::fs::create_dir_all(traces.join(run)).expect("the committed recording directory");
        std::fs::copy(
            fresh.trace_of(shard, Path::new("trace.jsonl").to_path_buf()),
            traces.join(run).join("trace.jsonl"),
        )
        .expect("the committed recording");
        for engine in fresh.engines_of(shard) {
            let build = engine
                .strip_prefix(
                    fresh
                        .traces
                        .join(shard.file_name().expect("the run"))
                        .join("builds"),
                )
                .expect("the engine recording is under the run's builds")
                .components()
                .next()
                .expect("the build's name")
                .as_os_str();
            let to = traces
                .join(run)
                .join("builds")
                .join(build)
                .join(njutest::app::trace::ENGINE_DIRECTORY);
            std::fs::create_dir_all(&to).expect("the committed engine recording directory");
            std::fs::copy(engine, to.join("trace.jsonl")).expect("the committed engine recording");
        }
    }
    std::fs::copy(&fresh.merged, into.join("merged.json")).expect("the committed merge");
}

#[test]
fn every_committed_sharded_run_is_one_todays_runner_records() {
    let updating = std::env::var_os(UPDATE_RUNS).is_some();
    let mut stale = Vec::new();
    for (recording, sealed) in RECORDINGS {
        let fixture = fixture();
        let fresh = recorded(&fixture, sealed);
        if updating {
            rewrite(recording, &fresh);
            continue;
        }
        let committed = committed_shapes(recording);
        let today = fresh.shapes();
        if committed != today {
            stale.push(format!(
                "{recording}\n  only in the committed recording:\n    {}\n  only in today's:\n    {}",
                committed
                    .difference(&today)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n    "),
                today
                    .difference(&committed)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n    ")
            ));
        }
    }
    assert!(
        stale.is_empty(),
        "a committed recording the runner no longer records is evidence an audit re-decides \
         for nobody: read the difference, then record them again with {UPDATE_RUNS}=1:\n{}",
        stale.join("\n")
    );
}

/// Holds the committed proofaudit specimen to the shape a complete run of today's runner writes, recording it again with `UPDATE_SPECIMEN_REPORT=1`.
#[test]
fn the_proofaudit_specimen_report_is_a_shape_a_run_writes() {
    let fixture = fixture();
    let (code, out, err) = asked(
        &fixture.root,
        &["verify", "--offline", "--locked", "--ui=plain"],
    );
    assert_eq!(code, 0, "a whole run of the fixture assures: {out}{err}");
    let run = newest_run(&fixture.root);
    let produced = document(&run.join(njutest::app::reports::DOCUMENT_NAME));
    let path =
        njutest_devkit::paths::workspace_root().join("xtask/src/proofaudit/specimen-report.json");
    if std::env::var_os(UPDATE_SPECIMEN).is_some() {
        std::fs::write(
            &path,
            serde_json::to_string_pretty(&emptied(&produced)).expect("the specimen serialises"),
        )
        .expect("the committed specimen");
        return;
    }
    let committed = document(&path);
    let (mut held, mut wrote) = (BTreeSet::new(), BTreeSet::new());
    shape(&committed, "specimen", &mut held);
    shape(&produced, "specimen", &mut wrote);
    let missing: Vec<String> = held.difference(&wrote).cloned().collect();
    assert!(
        missing.is_empty(),
        "every shape the proofaudit specimen fills is one a run still writes, or a completed \
         specimen says what no producer says: read the difference, then record it again with \
         {UPDATE_SPECIMEN}=1:\n{missing:?}"
    );
}

/// The specimen report a run's complete document becomes: every collection of rows it concluded emptied, keeping the envelope and the part's own columns as the run wrote them.
fn emptied(produced: &serde_json::Value) -> serde_json::Value {
    let mut specimen = produced.clone();
    let report = specimen
        .get_mut("report")
        .and_then(serde_json::Value::as_object_mut)
        .expect("a complete report");
    report.insert(
        "global_findings".to_owned(),
        serde_json::Value::Array(Vec::new()),
    );
    let builds = report
        .get_mut("builds")
        .and_then(serde_json::Value::as_array_mut)
        .expect("a complete report's builds");
    for build in builds {
        let parts = build
            .get_mut("parts")
            .and_then(serde_json::Value::as_array_mut)
            .expect("a complete build's parts");
        for part in parts {
            let fields = part.as_object_mut().expect("a complete catalog part");
            let unlisted: Vec<String> = fields
                .iter()
                .filter(|(_, field)| field.is_array())
                .map(|(name, _)| name.clone())
                .filter(|name| !ROWS.contains(&name.as_str()))
                .collect();
            assert!(
                unlisted.is_empty(),
                "a part holds a collection of rows the specimen does not know to empty, so a \
                 specimen recorded with {UPDATE_SPECIMEN} would keep a run's rows in it: {unlisted:?}"
            );
            for name in ROWS {
                fields.insert((*name).to_owned(), serde_json::Value::Array(Vec::new()));
            }
        }
    }
    specimen
}
