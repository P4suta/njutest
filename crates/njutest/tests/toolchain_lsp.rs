// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The language server on the process's own streams: what an editor that starts `njutest lsp` gets back.

#![cfg(unix)]
#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::io::Write;
use std::process::{Command, Stdio};

use njutest::app::lsp::{framed, message};
use njutest::config::Contract;
use njutest::report::{
    BuildReport, Finding, FindingKind, MutantAccounting, MutantRecord, ObserverAccounting,
    Position, RunKind, Timing,
};
use njutest_devkit::process::SupervisedChild;
use rust_mutants::id::RunId;
use serde_json::{Value, json};

fn run_id(value: &str) -> RunId {
    RunId::try_from(value).expect("a canonical run id")
}

/// The file the fixture run read, whose digest its report records.
const MEASURED: &str = "fn f() {}\n";

/// A workspace whose last run found one surviving mutation in `src/lib.rs`.
fn verified(root: &std::path::Path) -> String {
    let mut source = BuildReport::new(
        "20260909t000000z-000001",
        RunKind::Full,
        Contract::StandardV1,
    );
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    source.timing = Timing {
        started: "2026-09-09T00:00:00Z".to_owned(),
        finished: "2026-09-09T00:00:00Z".to_owned(),
        duration_ms: 1,
    };
    source.limitations.push(njutest::report::Limitation::new(
        "git-metadata-unavailable",
        "the LSP fixture is not a git repository",
    ));
    source.mutants.push(MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "b".repeat(64),
        display_id: "b".repeat(20),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 1,
            column: 5,
            character_column: 5,
        },
        rule: "gt-to-ge@1".to_owned(),
        item: "demo".to_owned(),
        original: ">".to_owned(),
        replacement: String::new(),
        outcome: njutest::report::Decided::Survived,
        accepted: false,
        reuse: njutest::report::Reuse(njutest::report::Established::Here),
        blind_in: Vec::new(),
        routing: None,
    });
    source.accounting.mutants = MutantAccounting {
        cataloged: 1,
        executed: 1,
        survived: 1,
        observers: ObserverAccounting {
            unnoticed: 1,
            ..ObserverAccounting::default()
        },
        ..MutantAccounting::default()
    };
    source.findings.push(Finding {
        kind: FindingKind::SurvivingMutant,
        subject: "b".repeat(20),
        detail: "no test noticed gt-to-ge@1".to_owned(),
        origin: njutest::report::FindingOrigin::Global,
        path: None,
        position: None,
    });
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, MEASURED.as_bytes());
    source.sources.insert(
        "src/lib.rs".to_owned(),
        rust_mutants::id::HexDigest::finish(hasher),
    );
    let one = run_id("one");
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let latticed = njutest::report::across::configured(&one, &measurements)
        .expect("one checked complete lattice");
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog LSP fixture cannot be a shard");
    };
    let report = latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion");
    let run = root
        .join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
        .join("runs")
        .join(one.as_str());
    std::fs::create_dir_all(&run).expect("mkdir");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/lib.rs"), MEASURED).expect("the file it is in");
    std::fs::write(
        run.join(njutest::app::reports::DOCUMENT_NAME),
        serde_json::to_string(&report).expect("a report"),
    )
    .expect("the report");
    std::fs::write(
        root.join(njutest::config::DEFAULT_REPORTS_DIRECTORY)
            .join(njutest::app::reports::Index::Any.file()),
        json!({
            "schema": njutest::report::SCHEMA,
            "directory": format!(
                "{}/runs/{}",
                njutest::config::DEFAULT_REPORTS_DIRECTORY,
                one.as_str()
            ),
            "run_id": one.as_str(),
        })
        .to_string(),
    )
    .expect("the pointer a reader follows");
    njutest::app::lsp::uri_of(&root.join("src/lib.rs")).expect("the fixture path is valid UTF-8")
}

#[test]
fn an_editor_that_starts_this_server_is_told_what_the_last_run_found() {
    let dir = tempfile::Builder::new()
        .prefix("njutest-lsp-")
        .tempdir()
        .expect("a temporary directory");
    let uri = verified(dir.path());

    let asked: String = [
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" }),
        json!({ "jsonrpc": "2.0", "method": "exit" }),
    ]
    .iter()
    .map(framed)
    .collect();

    let mut command = Command::new(env!("CARGO_BIN_EXE_njutest"));
    command
        .args(["lsp", "--directory", &dir.path().display().to_string()])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = SupervisedChild::launch(&mut command).expect("the server starts");
    child
        .take_stdin()
        .expect("a pipe")
        .write_all(asked.as_bytes())
        .expect("the client writes");
    let output = child.wait_with_output().expect("the server ends");

    let mut said = std::io::Cursor::new(output.stdout);
    let answered: Vec<Value> = std::iter::from_fn(|| message(&mut said)).collect();
    assert!(
        output.status.success(),
        "reading a report is not a verdict: {}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    assert_eq!(
        answered[0]["result"]["capabilities"]["positionEncoding"], "utf-16",
        "a client that named no encoding is answered in the one the protocol means: \
         {answered:?}"
    );
    assert_eq!(
        answered[1]["params"]["uri"], uri,
        "and the workspace it was pointed at is the one whose report it reads, not the \
         directory it happens to have been started in: {answered:?}"
    );
}
