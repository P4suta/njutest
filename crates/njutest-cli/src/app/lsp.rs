// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest lsp`: what a completed run found, in the editor the code is being written in.
//!
//! This server runs nothing. It reads the report a run already wrote and
//! projects it, which is what keeps `verify` the only thing that establishes
//! anything and this a way of reading it. A code action hands back the
//! `njutest accept` a reviewer would type; applying it is that command's job,
//! because a run is read-only and `fix --apply` is the one thing that writes.

use std::io::{BufRead, Read as _, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::EXIT_ASSURED;
use crate::report::Report;

/// How a client counts the characters of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    /// UTF-16 code units, which is what a client that says nothing means.
    #[default]
    Utf16,
    /// Bytes, which is what this report already records.
    Utf8,
}

impl Encoding {
    /// The name this encoding answers to in `initialize`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16",
            Self::Utf8 => "utf-8",
        }
    }

    /// What the client asked for out of what this server offers, or UTF-16, which is what the protocol means by saying nothing.
    #[must_use]
    pub fn asked(initialize: &Value) -> Self {
        let offered = initialize
            .get("params")
            .and_then(|one| one.get("capabilities"))
            .and_then(|one| one.get("general"))
            .and_then(|one| one.get("positionEncodings"))
            .and_then(Value::as_array);
        let wants = |name: &str| {
            offered.is_some_and(|list| list.iter().any(|one| one.as_str() == Some(name)))
        };
        if wants(Self::Utf8.name()) {
            Self::Utf8
        } else {
            Self::Utf16
        }
    }
}

/// One file's findings, ready to publish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reported {
    /// The workspace-relative path, as the report records it.
    pub path: String,
    /// What to show in it.
    pub diagnostics: Vec<Value>,
}

/// The findings of `report` that name a place in a file, by file.
///
/// A finding carries a position and not a path, because what it is about is
/// named by its subject: a surviving mutation is a mutant, and the mutant
/// record is what says which file it is in. A finding whose subject is not a
/// mutation of this run — a failing target, a missing one — has no file to be
/// shown in and is left out rather than guessed at.
#[must_use]
pub fn diagnostics(report: &Report, root: &Path, encoding: Encoding) -> Vec<Reported> {
    let mut by_file: std::collections::BTreeMap<String, Vec<Value>> =
        std::collections::BTreeMap::new();
    for finding in &report.findings {
        let Some(mutant) = report
            .mutants
            .iter()
            .find(|one| one.display_id == finding.subject || one.id == finding.subject)
        else {
            continue;
        };
        let at = finding.position.unwrap_or(mutant.position);
        let character = column(root, &mutant.path, at, encoding);
        let line = at.line.saturating_sub(1);
        by_file.entry(mutant.path.clone()).or_default().push(json!({
            "range": {
                "start": { "line": line, "character": character },
                "end": { "line": line, "character": character },
            },
            "severity": if finding.kind.is_defect() { 1 } else { 2 },
            "source": "njutest",
            "code": finding.kind_name(),
            "message": format!("{}: {}", finding.subject, finding.detail),
            "data": { "mutant": mutant.display_id, "rule": mutant.rule },
        }));
    }
    by_file
        .into_iter()
        .map(|(path, diagnostics)| Reported { path, diagnostics })
        .collect()
}

/// Where the report's column falls in the units the client counts.
///
/// The report records both a byte column and a scalar one, so UTF-8 needs no
/// file at all. UTF-16 does: how many code units a scalar takes is a fact
/// about the character, and the only place the characters are is the line
/// itself. A line this cannot read falls back to the scalar column, which is
/// the same number wherever a line holds nothing outside the basic plane.
fn column(root: &Path, path: &str, at: crate::report::Position, encoding: Encoding) -> u32 {
    let scalar = at.character_column.saturating_sub(1);
    if encoding == Encoding::Utf8 {
        return at.column.saturating_sub(1);
    }
    let Ok(text) = std::fs::read_to_string(root.join(path)) else {
        return scalar;
    };
    let at_line = usize::try_from(at.line.saturating_sub(1)).unwrap_or(usize::MAX);
    let Some(line) = text.lines().nth(at_line) else {
        return scalar;
    };
    let units: usize = line
        .chars()
        .take(usize::try_from(scalar).unwrap_or(usize::MAX))
        .map(char::len_utf16)
        .sum();
    u32::try_from(units).unwrap_or(scalar)
}

/// One message, framed the way the protocol frames them.
#[must_use]
pub fn framed(message: &Value) -> String {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

/// The next message, or nothing when the stream ended.
///
/// A header this reader does not know is skipped rather than refused: the
/// protocol allows more of them, and a server that stopped at the first one it
/// had not heard of would stop at a client doing nothing wrong.
///
/// The length a client declares bounds the read and is not trusted for the
/// allocation: a body that arrives shorter than its header said is not the
/// message the client framed, even when the bytes that did arrive happen to
/// parse, and a header naming a length nobody is going to send is not a reason
/// to reserve it.
pub fn message(input: &mut dyn BufRead) -> Option<Value> {
    let mut length = None;
    let mut header = String::new();
    loop {
        header.clear();
        if input.read_line(&mut header).ok()? == 0 {
            return None;
        }
        let line = header.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(said) = line.strip_prefix("Content-Length:") {
            length = said.trim().parse::<u64>().ok();
        }
    }
    let mut arriving = input.take(length?);
    let mut body = Vec::new();
    let _read = arriving.read_to_end(&mut body).ok()?;
    (arriving.limit() == 0)
        .then(|| serde_json::from_slice(&body).ok())
        .flatten()
}

/// Serves the protocol over `input` and `output` until the client asks it to stop.
///
/// A client that has asked this to shut down is one that has stopped reading:
/// it is waiting for the answer to that request and for nothing else, so what
/// a later notification would say goes to nobody. Only `exit` follows.
///
/// # Errors
/// None: a message this server does not answer is one it says nothing about,
/// and a report it cannot read is a run that has not happened yet.
pub fn serve(input: &mut dyn BufRead, output: &mut dyn Write, root: &Path) -> u8 {
    let mut encoding = Encoding::default();
    let mut stopping = false;
    while let Some(request) = message(input) {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let id = request.get("id").cloned();
        match method.as_str() {
            "initialize" => {
                encoding = Encoding::asked(&request);
                reply(output, id.as_ref(), &capabilities(encoding));
            }
            "initialized" | "textDocument/didOpen" | "textDocument/didSave" => {
                if !stopping {
                    publish(output, root, encoding);
                }
            }
            "textDocument/codeAction" => {
                reply(output, id.as_ref(), &actions(&request));
            }
            "shutdown" => {
                stopping = true;
                reply(output, id.as_ref(), &Value::Null);
            }
            "exit" => break,
            _ => {
                if id.is_some() {
                    reply(output, id.as_ref(), &Value::Null);
                }
            }
        }
    }
    EXIT_ASSURED
}

/// What this server does, which is read a report and offer the command that records an acceptance.
fn capabilities(encoding: Encoding) -> Value {
    json!({
        "capabilities": {
            "positionEncoding": encoding.name(),
            "textDocumentSync": { "openClose": true, "save": true },
            "codeActionProvider": true,
        },
        "serverInfo": { "name": "njutest", "version": crate::VERSION },
    })
}

/// The acceptance a reviewer would record for each mutation the request's range holds.
///
/// The action is the command line and not an edit. A run is read-only, and
/// `njutest accept` is what writes an acceptance, so handing back what to type
/// keeps the one thing that changes the tree in the one place that does it.
fn actions(request: &Value) -> Value {
    let mut offered = Vec::new();
    let empty = Vec::new();
    let held = request
        .get("params")
        .and_then(|one| one.get("context"))
        .and_then(|one| one.get("diagnostics"))
        .and_then(Value::as_array)
        .unwrap_or(&empty);
    for diagnostic in held {
        let Some(mutant) = diagnostic
            .get("data")
            .and_then(|one| one.get("mutant"))
            .and_then(Value::as_str)
        else {
            continue;
        };
        offered.push(json!({
            "title": format!("njutest accept {mutant} --reason '…'"),
            "kind": "quickfix",
            "diagnostics": [diagnostic],
            "command": {
                "title": "Copy the acceptance to record",
                "command": "njutest.accept",
                "arguments": [mutant],
            },
        }));
    }
    Value::Array(offered)
}

/// Publishes what the latest run found, and clears every file it found nothing in.
fn publish(output: &mut dyn Write, root: &Path, encoding: Encoding) {
    let Some(report) = latest(root) else {
        return;
    };
    for reported in diagnostics(&report, root, encoding) {
        let uri = uri_of(&root.join(&reported.path));
        notify(
            output,
            "textDocument/publishDiagnostics",
            &json!({ "uri": uri, "diagnostics": reported.diagnostics }),
        );
    }
}

/// `path` as the URI an editor holds the document under.
///
/// An editor matches a diagnostic to a document by the URI it is published
/// against, so a second spelling of one file is a diagnostic about a file
/// nobody has open. A URI separates with `/` whatever the platform separates
/// with, and a path that begins at a volume rather than at a root needs the
/// root the form puts before it.
#[must_use]
pub fn uri_of(path: &Path) -> String {
    let text = rust_mutants::id::slashed(path);
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        format!("file:///{text}")
    }
}

/// The report of the run that finished last, or nothing when none has.
fn latest(root: &Path) -> Option<Report> {
    let index = std::fs::read_to_string(root.join(super::reports::LATEST_ANY)).ok()?;
    let index: Value = serde_json::from_str(&index).ok()?;
    let directory = index.get("directory").and_then(Value::as_str)?;
    let path: PathBuf = root.join(directory).join(super::reports::DOCUMENT_NAME);
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

/// Answers one request.
fn reply(output: &mut dyn Write, id: Option<&Value>, result: &Value) {
    let message = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    drop(output.write_all(framed(&message).as_bytes()));
    drop(output.flush());
}

/// Tells the client something it did not ask for.
fn notify(output: &mut dyn Write, method: &str, params: &Value) {
    let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
    drop(output.write_all(framed(&message).as_bytes()));
    drop(output.flush());
}

/// Serves the protocol on this process's own streams.
#[must_use]
pub fn run(arguments: &crate::cli::Lsp, environment: &crate::cli::Environment) -> u8 {
    let root = environment.rooted(arguments.directory.as_deref());
    serve(
        &mut std::io::stdin().lock(),
        &mut std::io::stdout().lock(),
        &root,
    )
}
