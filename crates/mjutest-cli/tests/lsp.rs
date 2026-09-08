// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The language server: what it says about a report, and what it refuses to do about it.

#![expect(
    clippy::indexing_slicing,
    reason = "a test that reads a fixed answer out of a fixed request is a table, and an index out of range is the failure it is there to report"
)]

use std::io::Cursor;

use mjutest_cli::app::lsp::{Encoding, diagnostics, framed, message, serve};
use mjutest_cli::config::Contract;
use mjutest_cli::report::{
    Finding, FindingKind, MutantRecord, Position, Report, RunKind, TargetRecord, TargetStatus,
};
use serde_json::{Value, json};

fn reported() -> Report {
    let mut report = Report::new(
        "20260908T000000Z-000001",
        RunKind::Full,
        Contract::StandardV1,
    );
    report.targets.push(TargetRecord {
        id: "one".to_owned(),
        name: "pkg/lib/pkg".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 1,
        message: None,
    });
    report.mutants.push(MutantRecord {
        id: "a".repeat(64),
        display_id: "aaaaaaaaaaaa".to_owned(),
        path: "src/lib.rs".to_owned(),
        position: Position {
            line: 7,
            column: 9,
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
        subject: "aaaaaaaaaaaa".to_owned(),
        detail: "no test noticed gt-to-ge@1".to_owned(),
        position: None,
    });
    report
}

#[test]
fn a_finding_is_shown_where_the_mutation_it_names_is() {
    let root = tempfile::tempdir().expect("a directory");
    let shown = diagnostics(&reported(), root.path(), Encoding::Utf8);

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].path, "src/lib.rs");
    let at = &shown[0].diagnostics[0]["range"]["start"];
    assert_eq!(
        (at["line"].as_u64(), at["character"].as_u64()),
        (Some(6), Some(8)),
        "a report counts lines and columns from one and the protocol counts from zero: \
         a finding shown one line down is a finding about the wrong code"
    );
    assert_eq!(shown[0].diagnostics[0]["data"]["mutant"], "aaaaaaaaaaaa");
}

#[test]
fn a_finding_about_something_that_is_not_in_a_file_is_not_shown_in_one() {
    let mut report = reported();
    report.findings.push(Finding {
        kind: FindingKind::FailingTest,
        subject: "pkg/lib/pkg".to_owned(),
        detail: "the target failed".to_owned(),
        position: None,
    });
    let root = tempfile::tempdir().expect("a directory");

    let shown = diagnostics(&report, root.path(), Encoding::Utf8);
    assert_eq!(
        shown.iter().map(|one| one.diagnostics.len()).sum::<usize>(),
        1,
        "a failing target names no place in a file, and guessing one would put a \
         finding on code that has nothing to do with it"
    );
}

#[test]
fn a_column_is_counted_the_way_the_client_says_it_counts() {
    let root = tempfile::tempdir().expect("a directory");
    std::fs::create_dir_all(root.path().join("src")).expect("mkdir");
    std::fs::write(
        root.path().join("src/lib.rs"),
        "\n\n\n\n\n\n\u{1D11E}\u{1D11E}x = 1;\n",
    )
    .expect("a line whose characters are not one code unit each");

    let mut report = reported();
    report.mutants[0].position = Position {
        line: 7,
        column: 11,
        character_column: 5,
    };

    let utf8 = diagnostics(&report, root.path(), Encoding::Utf8);
    let utf16 = diagnostics(&report, root.path(), Encoding::Utf16);

    assert_eq!(
        utf8[0].diagnostics[0]["range"]["start"]["character"], 10,
        "the `=` of `\u{1D11E}\u{1D11E}x = 1;` is ten bytes in"
    );
    assert_eq!(
        utf16[0].diagnostics[0]["range"]["start"]["character"], 6,
        "and six UTF-16 code units in, because the two clefs are two units each. \
         Neither number is the other, and neither is the report's own scalar column of \
         four, so the line has to be read rather than assumed"
    );
}

#[test]
fn a_client_that_counts_bytes_is_told_the_server_counts_bytes() {
    let asked = Encoding::asked(&json!({
        "params": { "capabilities": { "general": { "positionEncodings": ["utf-8", "utf-16"] } } }
    }));
    assert_eq!(asked, Encoding::Utf8);
}

#[test]
fn a_client_that_says_nothing_gets_what_the_protocol_says_it_means() {
    assert_eq!(Encoding::asked(&json!({})), Encoding::Utf16);
    assert_eq!(Encoding::Utf16.name(), "utf-16");
}

#[test]
fn initialize_is_answered_and_the_stream_ending_ends_the_server() {
    let asked = framed(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }));
    let mut input = Cursor::new(asked.into_bytes());
    let mut output: Vec<u8> = Vec::new();
    let root = tempfile::tempdir().expect("a directory");

    let code = serve(&mut input, &mut output, root.path());

    assert_eq!(code, 0);
    let mut read = Cursor::new(output);
    let answer = message(&mut read).expect("one answer");
    assert_eq!(answer["id"], 1);
    assert_eq!(
        answer["result"]["capabilities"]["codeActionProvider"],
        Value::Bool(true)
    );
    assert!(
        message(&mut read).is_none(),
        "a run that has written no report has nothing to publish, and saying nothing is \
         not the same as saying there is nothing wrong"
    );
}

#[test]
fn a_code_action_hands_back_the_acceptance_to_record_and_edits_nothing() {
    let request = framed(&json!({
        "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction",
        "params": { "context": { "diagnostics": [
            { "data": { "mutant": "aaaaaaaaaaaa" }, "message": "no test noticed it" }
        ] } }
    }));
    let mut input = Cursor::new(request.into_bytes());
    let mut output: Vec<u8> = Vec::new();
    let root = tempfile::tempdir().expect("a directory");

    let _code = serve(&mut input, &mut output, root.path());

    let mut read = Cursor::new(output);
    let answer = message(&mut read).expect("one answer");
    let offered = answer["result"].as_array().expect("actions");
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0]["command"]["command"], "mjutest.accept");
    assert_eq!(offered[0]["command"]["arguments"][0], "aaaaaaaaaaaa");
    assert!(
        offered[0].get("edit").is_none(),
        "a run is read-only and `fix --apply` is the one thing that writes, so this \
         hands back what to type rather than typing it: {offered:?}"
    );
}

#[test]
fn a_header_this_reader_does_not_know_is_not_a_reason_to_stop() {
    let body = json!({ "jsonrpc": "2.0", "id": 3, "method": "shutdown" }).to_string();
    let framed = format!(
        "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let mut input = Cursor::new(framed.into_bytes());
    let mut output: Vec<u8> = Vec::new();
    let root = tempfile::tempdir().expect("a directory");

    let _code = serve(&mut input, &mut output, root.path());

    let mut read = Cursor::new(output);
    assert_eq!(
        message(&mut read).expect("an answer")["id"],
        3,
        "the protocol allows headers this reader has not heard of, and stopping at one \
         would stop at a client doing nothing wrong"
    );
}
