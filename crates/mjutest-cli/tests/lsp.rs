// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The language server: what it says about a report, and what it refuses to do about it.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test that reads a fixed answer out of a fixed request is a table, and an index out of range is the failure it is there to report"
)]

use std::io::{BufRead, Cursor, Read, Write};

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
    assert_eq!(
        Encoding::default(),
        Encoding::Utf16,
        "and a server that has not been asked yet counts the same way, because the \
         protocol says a client that named nothing means UTF-16"
    );
}

#[test]
fn a_client_that_counts_only_code_units_is_not_told_the_server_counts_bytes() {
    let asked = Encoding::asked(&json!({
        "params": { "capabilities": { "general": { "positionEncodings": ["utf-16"] } } }
    }));

    assert_eq!(
        asked,
        Encoding::Utf16,
        "what the client offered is what it can read: answering utf-8 to a client that \
         never named it would put every column in the wrong place"
    );
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

/// A stream whose headers can be read and whose body cannot.
struct Cut {
    header: Cursor<Vec<u8>>,
}

impl Read for Cut {
    fn read(&mut self, _buffer: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("the stream broke"))
    }
}

impl BufRead for Cut {
    fn fill_buf(&mut self) -> std::io::Result<&[u8]> {
        self.header.fill_buf()
    }

    fn consume(&mut self, amount: usize) {
        self.header.consume(amount);
    }
}

/// Everything written, and how many times it was pushed out.
#[derive(Default)]
struct Recording {
    said: Vec<u8>,
    flushes: u32,
}

impl Write for Recording {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        self.said.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.flushes = self.flushes.saturating_add(1);
        Ok(())
    }
}

fn answers(said: Vec<u8>) -> Vec<Value> {
    let mut read = Cursor::new(said);
    std::iter::from_fn(|| message(&mut read)).collect()
}

fn served(messages: &[Value], root: &std::path::Path) -> Recording {
    let asked: String = messages.iter().map(framed).collect();
    let mut output = Recording::default();
    let code = serve(&mut Cursor::new(asked.into_bytes()), &mut output, root);
    assert_eq!(
        code, 0,
        "reading a report is not a verdict: this server establishes nothing, so it has \
         nothing to fail about"
    );
    output
}

/// A workspace whose last run found the one mutation `reported` describes.
fn ran(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("reports/runs/one")).expect("mkdir");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/lib.rs"), "\n\n\n\n\n\n\u{1D11E}x = 1;\n")
        .expect("the file a finding is in");
    std::fs::write(
        root.join("reports/runs/one/mjutest-assurance-report-v1.json"),
        serde_json::to_string(&reported()).expect("a report"),
    )
    .expect("the report");
    std::fs::write(
        root.join("reports/latest-any.json"),
        json!({ "directory": "reports/runs/one", "run_id": "one" }).to_string(),
    )
    .expect("the pointer a reader follows");
}

#[test]
fn a_finding_this_cannot_place_does_not_hide_the_ones_after_it() {
    let root = tempfile::tempdir().expect("a directory");
    let mut report = reported();
    report.findings.insert(
        0,
        Finding {
            kind: FindingKind::FailingTest,
            subject: "pkg/lib/pkg".to_owned(),
            detail: "the target failed".to_owned(),
            position: None,
        },
    );

    let shown = diagnostics(&report, root.path(), Encoding::Utf8);

    assert_eq!(
        shown.len(),
        1,
        "a finding with no file to be shown in is one to step over, not one to stop at: \
         everything the run found after it is still something the editor has to show: \
         {shown:?}"
    );
}

#[test]
fn a_line_that_cannot_be_read_is_counted_the_way_the_report_already_counted_it() {
    let root = tempfile::tempdir().expect("a directory");
    let mut report = reported();
    report.mutants[0].position = Position {
        line: 7,
        column: 11,
        character_column: 5,
    };

    let missing = diagnostics(&report, root.path(), Encoding::Utf16);

    std::fs::create_dir_all(root.path().join("src")).expect("mkdir");
    std::fs::write(root.path().join("src/lib.rs"), "one line\n").expect("a short file");
    let short = diagnostics(&report, root.path(), Encoding::Utf16);

    assert_eq!(
        missing[0].diagnostics[0]["range"]["start"]["character"], 4,
        "a file this cannot read is one whose characters it cannot count, and the \
         report's own scalar column is the number that is right wherever a line holds \
         nothing outside the basic plane"
    );
    assert_eq!(
        short[0].diagnostics[0]["range"]["start"]["character"], 4,
        "and a line the file does not have is the same question with the same answer"
    );
}

#[test]
fn a_header_that_is_not_text_is_not_a_message() {
    let mut input = Cursor::new(b"Content-Length: \xff\xfe\r\n\r\n{}".to_vec());

    assert!(
        message(&mut input).is_none(),
        "a header this cannot read is one this cannot act on, and a server that fell \
         over on it would take the editor down with it"
    );
}

#[test]
fn the_end_of_a_stream_is_no_bytes_at_all_and_not_a_short_line() {
    let body = json!({ "jsonrpc": "2.0", "id": 4, "method": "shutdown" }).to_string();
    let mut input = Cursor::new(format!("Content-Length: {}\n\n{body}", body.len()).into_bytes());

    assert_eq!(
        message(&mut input).expect("a message")["id"],
        4,
        "the blank line that ends the headers is recognised by being blank, and the one \
         length that means the stream ended is no bytes at all: a client whose lines end \
         with a bare newline is one this reader still understands"
    );
}

#[test]
fn a_body_shorter_than_the_length_it_was_given_is_not_a_message() {
    let body = json!({ "jsonrpc": "2.0", "id": 5, "method": "shutdown" }).to_string();
    let mut input = Cursor::new(
        format!(
            "Content-Length: {}\r\n\r\n{body}",
            body.len().saturating_add(8)
        )
        .into_bytes(),
    );

    assert!(
        message(&mut input).is_none(),
        "the bytes that arrived parse, and they are still not the message the client \
         framed: a reader that answered this would answer half of what somebody sent \
         and leave the other half to be read as the next message"
    );
}

#[test]
fn a_stream_that_breaks_while_the_body_is_read_is_not_a_message() {
    let mut input = Cut {
        header: Cursor::new(b"Content-Length: 12\r\n\r\n".to_vec()),
    };

    assert!(
        message(&mut input).is_none(),
        "a body this could not read is not one it can act on"
    );
}

#[test]
fn a_diagnostic_that_names_no_mutation_does_not_hide_the_ones_after_it() {
    let root = tempfile::tempdir().expect("a directory");
    let output = served(
        &[json!({
            "jsonrpc": "2.0", "id": 6, "method": "textDocument/codeAction",
            "params": { "context": { "diagnostics": [
                { "message": "something else put this here" },
                { "data": { "mutant": "aaaaaaaaaaaa" } }
            ] } }
        })],
        root.path(),
    );

    let answered = answers(output.said);
    let offered = answered[0]["result"].as_array().expect("actions");
    assert_eq!(
        offered.len(),
        1,
        "an editor shows diagnostics from more than one tool at a time, and one this \
         server did not write is one to step over rather than one to stop at: {offered:?}"
    );
}

#[test]
fn a_request_this_server_does_not_answer_is_still_answered_and_a_notification_is_not() {
    let root = tempfile::tempdir().expect("a directory");

    let asked = served(
        &[json!({ "jsonrpc": "2.0", "id": 7, "method": "textDocument/hover" })],
        root.path(),
    );
    let told = served(
        &[json!({ "jsonrpc": "2.0", "method": "$/setTrace" })],
        root.path(),
    );

    assert_eq!(
        answers(asked.said).len(),
        1,
        "a client that asked for something carries an id and waits for it: leaving a \
         request unanswered hangs the editor on a method this server simply does not do"
    );
    assert!(
        answers(told.said).is_empty(),
        "and a notification carries no id because nobody is waiting: answering one puts \
         a reply on the wire that the client has nothing to match it to"
    );
}

#[test]
fn exit_is_the_last_message_this_server_reads() {
    let root = tempfile::tempdir().expect("a directory");

    let output = served(
        &[
            json!({ "jsonrpc": "2.0", "method": "exit" }),
            json!({ "jsonrpc": "2.0", "id": 8, "method": "shutdown" }),
        ],
        root.path(),
    );

    assert!(
        answers(output.said).is_empty(),
        "a client that said exit has stopped listening, and a server still reading its \
         stream is one that outlives the editor that started it"
    );
}

#[test]
fn what_the_last_run_found_is_published_when_a_file_is_opened_or_saved() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());

    let output = served(
        &[
            json!({ "jsonrpc": "2.0", "id": 9, "method": "initialize", "params": {
                "capabilities": { "general": { "positionEncodings": ["utf-8"] } }
            } }),
            json!({ "jsonrpc": "2.0", "method": "textDocument/didSave" }),
        ],
        root.path(),
    );

    let said = answers(output.said);
    let published: Vec<&Value> = said
        .iter()
        .filter(|one| one["method"] == "textDocument/publishDiagnostics")
        .collect();
    assert_eq!(
        published.len(),
        1,
        "a person who opens a file is asking what is known about it, and what is known \
         is the report the last run wrote: {said:?}"
    );
    assert_eq!(
        published[0]["params"]["uri"],
        format!("file://{}", root.path().join("src/lib.rs").display()),
        "the file a finding is in is the one it is shown in"
    );
    assert_eq!(
        published[0]["params"]["diagnostics"][0]["range"]["start"]["character"], 8,
        "and the columns are counted the way this client said it counts: the `1` of \
         `\u{1D11E}x = 1;` is eight bytes in and five code units in, so a server that \
         forgot what was negotiated would put it three characters off: {said:?}"
    );
}

#[test]
fn a_run_that_has_not_happened_is_not_a_file_with_nothing_wrong_in_it() {
    let root = tempfile::tempdir().expect("a directory");
    let never = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    std::fs::create_dir_all(root.path().join("reports")).expect("mkdir");
    std::fs::write(root.path().join("reports/latest-any.json"), "{ not json")
        .expect("a pointer nobody can follow");
    let unreadable = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    std::fs::write(
        root.path().join("reports/latest-any.json"),
        json!({ "directory": "reports/runs/gone" }).to_string(),
    )
    .expect("a pointer to a run whose report is not there");
    let missing = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    assert!(
        answers(never.said).is_empty()
            && answers(unreadable.said).is_empty()
            && answers(missing.said).is_empty(),
        "publishing an empty list would clear the editor's markers, which reads as a \
         file that was measured and found clean: a workspace nobody has verified, a \
         pointer nobody can follow, and a run whose report is not there all say nothing \
         at all"
    );
}

#[test]
fn every_answer_is_pushed_out_rather_than_left_in_a_buffer() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());

    let answered = served(
        &[json!({ "jsonrpc": "2.0", "id": 10, "method": "shutdown" })],
        root.path(),
    );
    let published = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    assert!(
        answered.flushes > 0 && published.flushes > 0,
        "a client reads this over a pipe and waits for the bytes: an answer sitting in \
         a buffer is an editor waiting for a server that has already answered ({} \
         answered, {} published)",
        answered.flushes,
        published.flushes
    );
}
