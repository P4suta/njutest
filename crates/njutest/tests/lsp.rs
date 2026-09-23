// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The language server: what it says about a report, and what it refuses to do about it.
//!
//! Every test here needs a published report, and `Store::keep` answers `NJ6004` on Windows because publication is rooted at a POSIX directory capability. `docs/limitations.md` says so; these say it by not existing there.

#![cfg(unix)]
#![expect(
    clippy::assigning_clones,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::too_many_lines,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::io::{BufRead, Cursor, Read, Write};

use njutest::app::lsp::{Encoding, diagnostics, framed, message, serve};
use njutest::cli::EXIT_ASSURED;
use njutest::config::Contract;
use njutest::report::{
    BuildReport, Finding, FindingKind, Limitation, MutantAccounting, MutantRecord,
    ObserverAccounting, Position, Report, RunKind, TargetRecord, TargetStatus,
};
use rust_mutants::id::RunId;
use serde_json::{Value, json};

fn run_id(value: &str) -> RunId {
    RunId::try_from(value).expect("a canonical run id")
}

/// The on-disk spelling used only to assemble hostile LSP fixtures.
///
/// Production LSP readers use a held `StoredRun` capability and never reopen these display paths after validation.
fn fixture_report_root(root: &std::path::Path) -> std::path::PathBuf {
    root.join(
        njutest::config::Config::default()
            .reports
            .directory
            .as_path(),
    )
}

fn fixture_runs(root: &std::path::Path) -> std::path::PathBuf {
    fixture_report_root(root).join("runs")
}

fn fixture_run(root: &std::path::Path, run: &RunId) -> std::path::PathBuf {
    fixture_runs(root).join(run.as_str())
}

fn fixture_index(
    root: &std::path::Path,
    index: njutest::app::reports::Index,
) -> std::path::PathBuf {
    fixture_report_root(root).join(index.file())
}

fn fixture_run_name(run: &RunId) -> String {
    format!(
        "{}/runs/{}",
        njutest::config::DEFAULT_REPORTS_DIRECTORY,
        run.as_str()
    )
}

/// The file the fixture run measured, with its mutation on line 7.
const CLEF: &str = "\n\n\n\n\n\n\u{1D11E}x = 1;\n";

fn reported() -> Report {
    reported_at(
        Position {
            line: 7,
            column: 9,
            character_column: 5,
        },
        false,
        CLEF,
    )
}

/// The SHA-256 of `text`, as a run records the file it read.
fn digest_of(text: &str) -> rust_mutants::id::HexDigest {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, text.as_bytes());
    rust_mutants::id::HexDigest::finish(hasher)
}

/// A workspace holding `text` at `src/lib.rs`.
fn holding(root: &std::path::Path, text: &str) {
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    std::fs::write(root.join("src/lib.rs"), text).expect("the file a finding is in");
}

/// A run that measured `measured` at `src/lib.rs` and found the mutation at `position` survived.
fn reported_at(position: Position, unplaced_finding: bool, measured: &str) -> Report {
    let mut source = BuildReport::new("source-one", RunKind::Full, Contract::StandardV1);
    source.repository.root_name = "workspace".to_owned();
    source.repository.workspace_digest = "b".repeat(64);
    source.repository.configuration_digest = "c".repeat(64);
    source.toolchain.rustc = "rustc 1.98.0".to_owned();
    source.scope.configured_builds = vec![njutest::config::DEFAULT_CONFIGURATION.to_owned()];
    source.timing.started = "2026-09-08T00:00:00Z".to_owned();
    source.timing.finished = "2026-09-08T00:00:00Z".to_owned();
    source.timing.duration_ms = 1;
    source.targets.push(TargetRecord {
        id: "one".to_owned(),
        name: "pkg/lib/pkg".to_owned(),
        package: "pkg".to_owned(),
        status: TargetStatus::Passed,
        duration_ms: 1,
        message: None,
    });
    source.count_targets().expect("one exact target row");
    source.mutants.push(MutantRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "a".repeat(64),
        display_id: "aaaaaaaaaaaaaaaaaaaa".to_owned(),
        path: "src/lib.rs".to_owned(),
        position,
        rule: "gt-to-ge".to_owned(),
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
        subject: "aaaaaaaaaaaaaaaaaaaa".to_owned(),
        detail: "no test noticed gt-to-ge".to_owned(),
        origin: njutest::report::FindingOrigin::Global,
        path: None,
        position: None,
    });
    if unplaced_finding {
        source.findings.push(Finding {
            kind: FindingKind::FailingTest,
            subject: "pkg/lib/pkg".to_owned(),
            detail: "the target failed".to_owned(),
            origin: njutest::report::FindingOrigin::Global,
            path: None,
            position: None,
        });
    }
    source.limitations.push(Limitation::new(
        "git-metadata-unavailable",
        "the LSP fixture is not a git repository",
    ));
    source
        .sources
        .insert("src/lib.rs".to_owned(), digest_of(measured));
    source.verdict = source.concluded();
    let measurements = njutest::report::across::BuildMeasurements::checked(vec![(
        njutest::config::DEFAULT_CONFIGURATION.to_owned(),
        rust_mutants::cargo::BuildConfig::default().selection(),
        source,
    )])
    .expect("one checked build measurement");
    let final_run = run_id("one");
    let latticed = njutest::report::across::configured(&final_run, &measurements)
        .expect("one checked complete lattice");
    let njutest::report::LatticedDocument::Complete(latticed) = latticed else {
        panic!("the whole-catalog LSP fixture cannot be a shard");
    };
    latticed
        .complete_without_models()
        .expect("standard-v1 needs no model completion")
}

#[test]
fn a_finding_is_shown_where_the_mutation_it_names_is() {
    let root = tempfile::tempdir().expect("a directory");
    holding(root.path(), CLEF);
    let shown = diagnostics(&reported(), root.path(), Encoding::Utf8)
        .expect("the checked report has representable diagnostics");

    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].path, "src/lib.rs");
    let at = &shown[0].diagnostics[0]["range"]["start"];
    assert_eq!(
        (at["line"].as_u64(), at["character"].as_u64()),
        (Some(6), Some(8)),
        "a report counts lines and columns from one and the protocol counts from zero: \
         a finding shown one line down is a finding about the wrong code"
    );
    assert_eq!(
        shown[0].diagnostics[0]["data"]["mutant"], "src/lib.rs:demo:gt-to-ge@7",
        "an editor's quick fix is a command somebody runs, so the name in it is the name \
         every other surface of this run prints: a digest here and a locator in the \
         terminal is one mutation with two names"
    );
    assert_eq!(
        shown[0].diagnostics[0]["data"]["id"],
        "aaaaaaaaaaaaaaaaaaaa"
    );
}

#[test]
fn a_finding_about_something_that_is_not_in_a_file_is_not_shown_in_one() {
    let report = reported_at(
        Position {
            line: 7,
            column: 9,
            character_column: 5,
        },
        true,
        CLEF,
    );
    let root = tempfile::tempdir().expect("a directory");
    holding(root.path(), CLEF);

    let shown = diagnostics(&report, root.path(), Encoding::Utf8)
        .expect("the checked report has representable diagnostics");
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
    let clefs = "\n\n\n\n\n\n\u{1D11E}\u{1D11E}x = 1;\n";
    std::fs::write(root.path().join("src/lib.rs"), clefs)
        .expect("a line whose characters are not one code unit each");

    let report = reported_at(
        Position {
            line: 7,
            column: 11,
            character_column: 5,
        },
        false,
        clefs,
    );

    let utf8 = diagnostics(&report, root.path(), Encoding::Utf8)
        .expect("the checked report has representable UTF-8 diagnostics");
    let utf16 = diagnostics(&report, root.path(), Encoding::Utf16)
        .expect("the checked report has representable UTF-16 diagnostics");

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
            { "data": { "mutant": "aaaaaaaaaaaaaaaaaaaa" }, "message": "no test noticed it" }
        ] } }
    }));
    let mut input = Cursor::new(request.into_bytes());
    let mut output: Vec<u8> = Vec::new();
    let root = tempfile::tempdir().expect("a directory");

    let code = serve(&mut input, &mut output, root.path());
    assert_eq!(code, EXIT_ASSURED, "a valid code-action request is served");

    let mut read = Cursor::new(output);
    let answer = message(&mut read).expect("one answer");
    let offered = answer["result"].as_array().expect("actions");
    assert_eq!(offered.len(), 1);
    assert_eq!(offered[0]["command"]["command"], "njutest.accept");
    assert_eq!(
        offered[0]["command"]["arguments"][0],
        "aaaaaaaaaaaaaaaaaaaa"
    );
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

    let code = serve(&mut input, &mut output, root.path());
    assert_eq!(code, EXIT_ASSURED, "an allowed extra header is served");

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
    let one = run_id("one");
    std::fs::create_dir_all(fixture_run(root, &one)).expect("mkdir");
    std::fs::create_dir_all(root.join("src")).expect("mkdir");
    holding(root, CLEF);
    std::fs::write(
        fixture_run(root, &one).join(njutest::app::reports::DOCUMENT_NAME),
        serde_json::to_string(&reported()).expect("a report"),
    )
    .expect("the report");
    std::fs::write(
        fixture_index(root, njutest::app::reports::Index::Any),
        json!({
            "schema": njutest::report::SCHEMA,
            "directory": fixture_run_name(&one),
            "run_id": one.as_str(),
        })
        .to_string(),
    )
    .expect("the pointer a reader follows");
}

#[test]
fn a_finding_this_cannot_place_does_not_hide_the_ones_after_it() {
    let root = tempfile::tempdir().expect("a directory");
    let report = reported_at(
        Position {
            line: 7,
            column: 9,
            character_column: 5,
        },
        true,
        CLEF,
    );

    let shown = diagnostics(&report, root.path(), Encoding::Utf8)
        .expect("the checked report has representable diagnostics");

    assert_eq!(
        shown.len(),
        1,
        "a finding with no file to be shown in is one to step over, not one to stop at: \
         everything the run found after it is still something the editor has to show: \
         {shown:?}"
    );
}

#[test]
fn a_file_the_run_cannot_vouch_for_is_noted_and_a_line_its_measured_file_lacks_is_counted_as_the_report_counted_it()
 {
    let root = tempfile::tempdir().expect("a directory");
    let short = "one line\n";
    let report = reported_at(
        Position {
            line: 7,
            column: 11,
            character_column: 5,
        },
        false,
        short,
    );

    let missing = diagnostics(&report, root.path(), Encoding::Utf16)
        .expect("the checked report has representable diagnostics");
    assert_eq!(
        missing[0].diagnostics[0]["code"], "not-yet-asked",
        "a file this cannot read is one it cannot hold to the run's record, so the finding \
         is not placed in it and the reader is told why: {missing:?}"
    );

    holding(root.path(), short);
    let counted = diagnostics(&report, root.path(), Encoding::Utf16)
        .expect("the checked report has representable diagnostics");
    assert_eq!(
        counted[0].diagnostics[0]["range"]["start"]["character"], 4,
        "a file that is the one the run read but has no line 7 has no characters there to \
         count, and the report's own scalar column is the number that is right wherever a \
         line holds nothing outside the basic plane: {counted:?}"
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
                { "data": { "mutant": "aaaaaaaaaaaaaaaaaaaa" } }
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
        njutest::app::lsp::uri_of(&root.path().join("src/lib.rs"))
            .expect("the fixture path is valid UTF-8"),
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

    std::fs::create_dir_all(fixture_runs(root.path())).expect("mkdir");
    std::fs::write(
        fixture_index(root.path(), njutest::app::reports::Index::Any),
        "{ not json",
    )
    .expect("a pointer nobody can follow");
    let unreadable = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    std::fs::write(
        fixture_index(root.path(), njutest::app::reports::Index::Any),
        json!({
            "directory": fixture_run_name(&run_id("gone"))
        })
        .to_string(),
    )
    .expect("a pointer to a run whose report is not there");
    let missing = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    let published = |said: Vec<u8>| {
        answers(said)
            .into_iter()
            .filter(|one| one["method"] == "textDocument/publishDiagnostics")
            .collect::<Vec<_>>()
    };
    assert!(
        published(never.said).is_empty()
            && published(unreadable.said).is_empty()
            && published(missing.said).is_empty(),
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

#[test]
fn a_server_that_has_been_asked_to_stop_says_nothing_more_about_the_tree() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());

    let output = served(
        &[
            json!({ "jsonrpc": "2.0", "id": 11, "method": "shutdown" }),
            json!({ "jsonrpc": "2.0", "method": "textDocument/didSave" }),
        ],
        root.path(),
    );

    let said = answers(output.said);
    assert!(
        said.iter()
            .all(|one| one["method"] != "textDocument/publishDiagnostics"),
        "a client that asked this to shut down is waiting for that answer and for \
         nothing else: what a later notification says goes to nobody, and a server \
         still talking is one the editor has to keep reading to be rid of: {said:?}"
    );
    assert_eq!(
        said.len(),
        1,
        "and the shutdown itself is still answered, because the client is waiting for \
         that one: {said:?}"
    );
}

#[test]
fn the_whole_exchange_an_editor_has_with_this_server_is_recorded() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());

    let output = served(
        &[
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {
                "capabilities": { "general": { "positionEncodings": ["utf-8", "utf-16"] } }
            } }),
            json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" }),
            json!({ "jsonrpc": "2.0", "id": 2, "method": "textDocument/codeAction",
                "params": { "context": { "diagnostics": [
                    { "data": { "mutant": "src/lib.rs:demo:gt-to-ge@7", "id": "aaaaaaaaaaaaaaaaaaaa", "rule": "gt-to-ge" } }
                ] } } }),
            json!({ "jsonrpc": "2.0", "id": 3, "method": "shutdown" }),
            json!({ "jsonrpc": "2.0", "method": "exit" }),
        ],
        root.path(),
    );

    assert!(
        njutest_devkit::process::strict_utf8(&output.said).starts_with("Content-Length: "),
        "every message is framed the way the protocol frames them"
    );
    let mut lines = Vec::new();
    for said in answers(output.said) {
        let text = serde_json::to_string(&said).expect("one line");
        lines.extend_from_slice(
            text.replace(
                &njutest::app::lsp::uri_of(root.path()).expect("the fixture path is valid UTF-8"),
                "file://<root>",
            )
            .replace(&root.path().display().to_string(), "<root>")
            .as_bytes(),
        );
        lines.push(b'\n');
    }
    let golden =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/lsp.golden.jsonl");
    njutest_devkit::golden::golden(&golden, &lines).expect("the recorded exchange");
}

#[test]
fn a_header_block_that_never_says_how_long_the_body_is_is_not_a_message() {
    let body = json!({ "jsonrpc": "2.0", "id": 12, "method": "shutdown" }).to_string();
    let mut input =
        Cursor::new(format!("Content-Type: application/vscode-jsonrpc\r\n\r\n{body}").into_bytes());

    assert!(
        message(&mut input).is_none(),
        "the length is what says where this message ends and the next one begins, and \
         reading to the end of the stream instead would swallow every message after it"
    );
}

#[test]
fn a_pointer_that_names_no_run_is_not_a_run() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());
    std::fs::write(
        fixture_index(root.path(), njutest::app::reports::Index::Any),
        json!({ "run_id": "one" }).to_string(),
    )
    .expect("a pointer that says which run and not where it is");

    let output = served(
        &[json!({ "jsonrpc": "2.0", "method": "textDocument/didOpen" })],
        root.path(),
    );

    assert!(
        answers(output.said)
            .into_iter()
            .all(|one| one["method"] != "textDocument/publishDiagnostics"),
        "the directory is the only thing in that file this can follow, and guessing one \
         would put whatever is at the guess in front of a person as what their run found"
    );
}

/// Where `src/lib.rs` in `root` is, as the client names it.
fn library(root: &std::path::Path) -> String {
    njutest::app::lsp::uri_of(&root.join("src/lib.rs")).expect("a UTF-8 path")
}

/// A request for what the server says about the document at `uri`, over every line of it.
fn asking(id: u32, method: &str, uri: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "id": id, "method": method,
        "params": {
            "textDocument": { "uri": uri },
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 100, "character": 0 } },
        },
    })
}

/// What a client holding `text` as `src/lib.rs` is told, having opened it and then sent `asked`.
fn guarding(root: &std::path::Path, text: &str, asked: &[Value]) -> Vec<Value> {
    let mut messages = vec![
        json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        json!({
            "jsonrpc": "2.0", "method": "textDocument/didOpen",
            "params": { "textDocument": {
                "uri": library(root), "languageId": "rust", "version": 1, "text": text,
            } },
        }),
    ];
    messages.extend(asked.iter().cloned());
    answers(served(&messages, root).said)
}

/// The answer to the request sent under `id`.
fn answered(said: &[Value], id: u32) -> &Value {
    let Some(answer) = said.iter().find(|one| one["id"] == id) else {
        panic!("an answer under id {id}: {said:?}");
    };
    &answer["result"]
}

/// A client's edit that leaves `text` in the buffer at `uri`.
fn changed(uri: &str, version: u32, text: &str) -> Value {
    json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [ { "text": text } ],
        },
    })
}

#[test]
fn the_server_says_it_marks_lines_and_asks_for_the_whole_buffer_as_it_changes() {
    let root = tempfile::tempdir().expect("a directory");
    let said = answers(
        served(
            &[json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} })],
            root.path(),
        )
        .said,
    );
    let capabilities = &answered(&said, 1)["capabilities"];
    assert_eq!(capabilities["inlayHintProvider"], Value::Bool(true));
    assert_eq!(
        capabilities["codeLensProvider"],
        json!({ "resolveProvider": false })
    );
    assert_eq!(
        capabilities["textDocumentSync"]["change"],
        json!(1),
        "a mark is only true of the bytes the run read, so the server has to hold the buffer \
         the client holds rather than the file on disk"
    );
}

#[test]
fn an_open_file_the_run_measured_is_marked_where_its_changes_start_with_what_stands_behind_each_mark()
 {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());
    let uri = library(root.path());
    let said = guarding(
        root.path(),
        CLEF,
        &[
            asking(2, "textDocument/inlayHint", &uri),
            asking(3, "textDocument/codeLens", &uri),
        ],
    );
    let hints = answered(&said, 2).as_array().expect("inlay hints");
    let [hint] = hints.as_slice() else {
        panic!("one mark for the one line a change starts on: {said:?}");
    };
    assert_eq!(
        hint["position"],
        json!({ "line": 6, "character": 8 }),
        "the mark sits at the end of line 7, counted as the client counts: the clef is two \
         UTF-16 units"
    );
    assert_eq!(hint["label"], "\u{25cb} left free");
    let told = hint["tooltip"].as_str().expect("a tooltip in plain text");
    assert!(
        told.contains("deleting `>`")
            && told.contains("nothing noticed it")
            && told.contains("njutest explain src/lib.rs:demo:gt-to-ge@7"),
        "hovering the mark says what was changed, what stands behind the mark, and the \
         command that asks about it: {told}"
    );
    let lenses = answered(&said, 3).as_array().expect("code lenses");
    let [lens] = lenses.as_slice() else {
        panic!("one lens for the one item: {said:?}");
    };
    assert_eq!(lens["range"]["start"], json!({ "line": 6, "character": 0 }));
    assert_eq!(lens["command"]["title"], "demo: 1 left free");
    assert_eq!(
        lens["command"]["arguments"],
        json!(["src/lib.rs:demo"]),
        "the lens names the item as `njutest spec` reads it"
    );
}

#[test]
fn a_buffer_holding_other_bytes_than_the_run_read_is_marked_nowhere_until_it_holds_them_again() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());
    let uri = library(root.path());
    let unopened =
        njutest::app::lsp::uri_of(&root.path().join("src/other.rs")).expect("a UTF-8 path");
    let said = guarding(
        root.path(),
        CLEF,
        &[
            changed(&uri, 2, &format!("{CLEF}\n")),
            asking(2, "textDocument/inlayHint", &uri),
            asking(3, "textDocument/codeLens", &uri),
            changed(&uri, 3, CLEF),
            asking(4, "textDocument/inlayHint", &uri),
            asking(5, "textDocument/inlayHint", &unopened),
        ],
    );
    assert_eq!(
        answered(&said, 2),
        &json!([]),
        "an edit the file on disk does not have yet still makes the buffer another program, \
         and a mark on it would be about code nobody asked about"
    );
    assert_eq!(answered(&said, 3), &json!([]));
    assert_eq!(
        answered(&said, 4).as_array().map(Vec::len),
        Some(1),
        "undoing the edit gives back the bytes the run read, and the mark with them"
    );
    assert_eq!(
        answered(&said, 5),
        &json!([]),
        "a document the client never opened is one the server holds no bytes of"
    );
}

#[test]
fn the_latest_run_is_read_once_however_often_marks_are_asked_for() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());
    let uri = library(root.path());
    let said = guarding(
        root.path(),
        CLEF,
        &[
            asking(2, "textDocument/inlayHint", &uri),
            asking(3, "textDocument/inlayHint", &uri),
            asking(4, "textDocument/codeLens", &uri),
        ],
    );
    let reads = said
        .iter()
        .filter(|one| {
            one["method"] == "window/logMessage"
                && one["params"]["message"]
                    .as_str()
                    .is_some_and(|said| said.starts_with("reading run one"))
        })
        .count();
    assert_eq!(
        reads, 1,
        "an editor asks for marks on every scroll, and the report is read once for as long as \
         its run is the latest: {said:?}"
    );
}

#[test]
fn an_edit_with_a_range_is_not_taken_for_the_whole_document() {
    let root = tempfile::tempdir().expect("a directory");
    ran(root.path());
    let uri = library(root.path());
    let incremental = json!({
        "jsonrpc": "2.0", "method": "textDocument/didChange",
        "params": {
            "textDocument": { "uri": uri, "version": 2 },
            "contentChanges": [ {
                "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } },
                "text": CLEF,
            } ],
        },
    });
    let said = guarding(
        root.path(),
        CLEF,
        &[incremental, asking(2, "textDocument/inlayHint", &uri)],
    );
    assert_eq!(
        answered(&said, 2),
        &json!([]),
        "an edit with a range is a piece of the document put somewhere in it: the buffer now \
         holds the run's bytes twice, and taking the piece for the whole would mark it"
    );
    assert!(
        said.iter().any(|one| {
            one["method"] == "window/logMessage"
                && one["params"]["message"]
                    .as_str()
                    .is_some_and(|said| said.contains("sent an edit as a range"))
        }),
        "the client is told why its marks went away: {said:?}"
    );
}
