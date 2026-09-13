// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The language server on the process's own streams: what an editor that starts `njutest lsp` gets back.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, and an answer that is not where the protocol puts it is the failure this is here to report"
)]

use std::io::Write;
use std::process::{Command, Stdio};

use njutest_cli::app::lsp::{framed, message};
use njutest_cli::config::Contract;
use njutest_cli::report::{Finding, FindingKind, MutantRecord, Position, Report, RunKind};
use serde_json::{Value, json};

/// A workspace whose last run found one surviving mutation in `src/lib.rs`.
fn verified(root: &std::path::Path) -> String {
    let mut report = Report::new(
        "20260909T000000Z-000001",
        RunKind::Full,
        Contract::StandardV1,
    );
    report.mutants.push(MutantRecord {
        id: "b".repeat(64),
        display_id: "bbbbbbbbbbbb".to_owned(),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 1,
            column: 5,
            character_column: 5,
        },
        rule: "gt-to-ge@1".to_owned(),
        outcome: "survived".to_owned(),
        killed_by: None,
        reused: false,
        source_run_id: None,
    });
    report.findings.push(Finding {
        kind: FindingKind::SurvivingMutant,
        subject: "bbbbbbbbbbbb".to_owned(),
        detail: "no test noticed gt-to-ge@1".to_owned(),
        path: None,
        position: None,
    });
    std::fs::create_dir_all(root.join("reports/runs/one")).expect("mkdir");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/lib.rs"), "fn f() {}\n").expect("the file it is in");
    std::fs::write(
        root.join("reports/runs/one/njutest-assurance-report-v1.json"),
        serde_json::to_string(&report).expect("a report"),
    )
    .expect("the report");
    std::fs::write(
        root.join("reports/latest-any.json"),
        json!({ "directory": "reports/runs/one", "run_id": report.run_id }).to_string(),
    )
    .expect("the pointer a reader follows");
    format!("file://{}", root.join("src/lib.rs").display())
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

    let mut child = Command::new(env!("CARGO_BIN_EXE_njutest"))
        .args(["lsp", "--directory", &dir.path().display().to_string()])
        .current_dir(std::env::temp_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the server starts");
    child
        .stdin
        .take()
        .expect("a pipe")
        .write_all(asked.as_bytes())
        .expect("the client writes");
    let output = child.wait_with_output().expect("the server ends");

    let mut said = std::io::Cursor::new(output.stdout);
    let answered: Vec<Value> = std::iter::from_fn(|| message(&mut said)).collect();
    assert!(
        output.status.success(),
        "reading a report is not a verdict: {}",
        String::from_utf8_lossy(&output.stderr)
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
