// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest lsp`: what a completed run found, in the editor the code is being written in.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, Read as _, Write};
use std::path::Path;

use serde_json::{Value, json};

use crate::cli::{EXIT_ASSURED, EXIT_ERROR};
use crate::report::Report;
use crate::spec::Specification;

/// How a client counts the characters of a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-16 code units, which is what a client that says nothing means.
    Utf16,
    /// Bytes, which is what this report already records.
    Utf8,
}

impl Default for Encoding {
    fn default() -> Self {
        Self::PROTOCOL_DEFAULT
    }
}

impl Encoding {
    const PROTOCOL_DEFAULT: Self = Self::Utf16;

    /// The name this encoding answers to in `initialize`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Utf16 => "utf-16",
            Self::Utf8 => "utf-8",
        }
    }

    /// How many units the client counts in `text`.
    #[must_use]
    pub fn width(self, text: &str) -> u32 {
        let units = match self {
            Self::Utf16 => text.encode_utf16().count(),
            Self::Utf8 => text.len(),
        };
        match u32::try_from(units) {
            Ok(units) => units,
            Err(_too_long) => u32::MAX,
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

/// The findings of `report` that name a place in a file, by file, placed only in files that still hold the bytes the run read.
///
/// A file edited since the run, or one nothing can hold to the run's record, gets one information diagnostic saying which run measured it and that nothing it found there is shown until a run measures the file again: a finding placed by line and column in code the run never measured points at the wrong thing.
/// # Errors
/// Returns the report's checked projection failure instead of publishing a partial diagnostic set.
pub fn diagnostics(
    report: &Report,
    root: &Path,
    encoding: Encoding,
) -> Result<Vec<Reported>, crate::report::CountError> {
    let sources = crate::presentation::Sources::read(root, report)?;
    let mut by_file: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut unshown: BTreeMap<String, crate::presentation::Missing> = BTreeMap::new();
    let conclusion = report.conclusion()?;
    for finding in &conclusion.findings {
        let Some(mutant) = conclusion
            .mutants
            .iter()
            .find(|one| one.display_id() == finding.subject || one.id() == finding.subject)
        else {
            continue;
        };
        if let Err(missing) = sources.standing(mutant.path()) {
            unshown.insert(mutant.path().to_owned(), missing);
            continue;
        }
        let at = match finding.position {
            Some(position) => position,
            None => mutant.position(),
        };
        let character = column(&sources, mutant.path(), at, encoding);
        let line = at.line.saturating_sub(1);
        by_file
            .entry(mutant.path().to_owned())
            .or_default()
            .push(json!({
                "range": {
                    "start": { "line": line, "character": character },
                    "end": { "line": line, "character": character },
                },
                "severity": if finding.kind.is_defect() { 1 } else { 2 },
                "source": "njutest",
                "code": finding.kind_name(),
                "message": format!("{}: {}", finding.subject, finding.detail),
                "data": {
                    "mutant": crate::naming::locator(mutant),
                    "id": mutant.display_id(),
                    "rule": mutant.rule(),
                },
            }));
    }
    for (path, missing) in unshown {
        by_file.insert(
            path,
            vec![json!({
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 0, "character": 0 },
                },
                "severity": 3,
                "source": "njutest",
                "code": "not-yet-asked",
                "message": unshown_note(report.run_id(), missing),
            })],
        );
    }
    Ok(by_file
        .into_iter()
        .map(|(path, diagnostics)| Reported { path, diagnostics })
        .collect())
}

/// What a reader of a file the run's findings are not shown in is told: why, which run, and what shows them again.
fn unshown_note(run: &str, missing: crate::presentation::Missing) -> String {
    format!(
        "{}; {}",
        missing.told(),
        crate::presentation::until_measured(run)
    )
}

/// Where the report's column falls in the units the client counts, read off the line as the run measured it.
fn column(
    sources: &crate::presentation::Sources,
    path: &str,
    at: crate::report::Position,
    encoding: Encoding,
) -> u32 {
    let scalar = at.character_column.saturating_sub(1);
    if encoding == Encoding::Utf8 {
        return at.column.saturating_sub(1);
    }
    let line = match sources.at(path, at.line) {
        crate::presentation::Excerpt::Read(line) => line,
        crate::presentation::Excerpt::Instead(_missing) => return scalar,
    };
    let units: usize = line
        .text()
        .chars()
        .take(match usize::try_from(scalar) {
            Ok(column) => column,
            Err(_) => return scalar,
        })
        .map(char::len_utf16)
        .sum();
    match u32::try_from(units) {
        Ok(units) => units,
        Err(_) => scalar,
    }
}

/// One message, framed the way the protocol frames them.
#[must_use]
pub fn framed(message: &Value) -> String {
    let body = message.to_string();
    format!("Content-Length: {}\r\n\r\n{body}", body.len())
}

/// The next message, or nothing when the stream ended.
pub fn message(input: &mut dyn BufRead) -> Option<Value> {
    let mut length = None;
    let mut header = String::new();
    loop {
        header.clear();
        let read = match input.read_line(&mut header) {
            Ok(read) => read,
            Err(_) => return None,
        };
        if read == 0 {
            return None;
        }
        let line = header.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(said) = line.strip_prefix("Content-Length:") {
            length = match said.trim().parse::<u64>() {
                Ok(length) => Some(length),
                Err(_) => return None,
            };
        }
    }
    let mut arriving = input.take(length?);
    let mut body = Vec::new();
    if arriving.read_to_end(&mut body).is_err() || arriving.limit() != 0 {
        return None;
    }
    match crate::strictjson::decode_slice(&body) {
        Ok(message) => Some(message),
        Err(_) => None,
    }
}

/// Serves the protocol over `input` and `output` until the client asks it to stop.
///
/// # Errors
/// None: a message this server does not answer is one it says nothing about,
/// and a report it cannot read is a run that has not happened yet.
pub fn serve(input: &mut dyn BufRead, output: &mut dyn Write, root: &Path) -> u8 {
    let mut encoding = Encoding::default();
    let mut stopping = false;
    let mut held = Held::default();
    while let Some(request) = message(input) {
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let id = request.get("id").cloned();
        let answered = match method.as_str() {
            "initialize" => {
                encoding = Encoding::asked(&request);
                reply(output, id.as_ref(), &capabilities(encoding))
            }
            "textDocument/didOpen" => {
                held.opened(&request);
                if stopping {
                    Ok(())
                } else {
                    publish(output, root, encoding)
                }
            }
            "initialized" | "textDocument/didSave" => {
                if stopping {
                    Ok(())
                } else {
                    publish(output, root, encoding)
                }
            }
            "textDocument/didChange" => held.changed(output, &request),
            "textDocument/didClose" => {
                held.closed(&request);
                Ok(())
            }
            "textDocument/inlayHint" => match held.guarded(output, root, &request) {
                Ok(guarded) => reply(
                    output,
                    id.as_ref(),
                    &guarded.map_or_else(|| json!([]), |one| one.hints(&request, encoding)),
                ),
                Err(write_error) => Err(write_error),
            },
            "textDocument/codeLens" => match held.guarded(output, root, &request) {
                Ok(guarded) => reply(
                    output,
                    id.as_ref(),
                    &guarded.map_or_else(|| json!([]), |one| one.lenses()),
                ),
                Err(write_error) => Err(write_error),
            },
            "textDocument/codeAction" => reply(output, id.as_ref(), &actions(&request)),
            "shutdown" => {
                stopping = true;
                reply(output, id.as_ref(), &Value::Null)
            }
            "exit" => break,
            unknown => answer_anyway(output, id.as_ref(), unknown),
        };
        match answered {
            Ok(()) => {}
            Err(_write_error) => return EXIT_ERROR,
        }
    }
    EXIT_ASSURED
}

/// What this server does, which is read a report, offer the command that records an acceptance, and mark the lines of a file whose bytes are the ones the run read.
///
/// A mark is only true of those bytes, so the server asks for the whole buffer on every change and holds what the client holds rather than what is on disk.
fn capabilities(encoding: Encoding) -> Value {
    json!({
        "capabilities": {
            "positionEncoding": encoding.name(),
            "textDocumentSync": { "openClose": true, "save": true, "change": 1 },
            "codeActionProvider": true,
            "inlayHintProvider": true,
            "codeLensProvider": { "resolveProvider": false },
        },
        "serverInfo": { "name": "njutest", "version": crate::VERSION },
    })
}

/// What to say about a method this server does not implement.
///
/// The one catch-all in the workspace that is kept on purpose.
/// A closed set is matched exhaustively here as everywhere (ADR 0023), and the methods of this protocol are not a set this workspace closes: a client may send any of them, the set grows without us, and the protocol says a server answers an unknown request rather than refusing it.
/// A catch-all over what somebody else's wire may carry is the handling; a catch-all over a set written in this repository is the defect.
fn answer_anyway(output: &mut dyn Write, id: Option<&Value>, _method: &str) -> std::io::Result<()> {
    if id.is_some() {
        reply(output, id, &Value::Null)
    } else {
        Ok(())
    }
}

/// The acceptance a reviewer would record for each mutation the request's range holds.
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
fn publish(output: &mut dyn Write, root: &Path, encoding: Encoding) -> std::io::Result<()> {
    let report = match latest(root) {
        Ok(Some(report)) => report,
        Ok(None) => return Ok(()),
        Err(error) => {
            notify(
                output,
                "window/logMessage",
                &json!({ "type": 1, "message": error.to_string() }),
            )?;
            return Ok(());
        }
    };
    let reported = diagnostics(&report, root, encoding).map_err(std::io::Error::other)?;
    for reported in reported {
        let uri = uri_of(&root.join(&reported.path)).map_err(std::io::Error::other)?;
        notify(
            output,
            "textDocument/publishDiagnostics",
            &json!({ "uri": uri, "diagnostics": reported.diagnostics }),
        )?;
    }
    Ok(())
}

/// What the server holds between messages: every document a client has open, as the client holds it, and the latest run it read.
#[derive(Debug, Default)]
struct Held {
    documents: BTreeMap<String, String>,
    latest: Option<Latest>,
}

/// The latest run, read once and kept for as long as it stays the latest.
#[derive(Debug)]
struct Latest {
    run: String,
    report: Report,
    recorded: BTreeMap<String, rust_mutants::id::HexDigest>,
}

/// A document whose bytes are the ones the latest run read, with what that run established about it.
#[derive(Debug)]
struct Guarded {
    specification: Specification,
    path: String,
    text: String,
}

impl Held {
    /// Keeps the text of a document the client opened.
    fn opened(&mut self, request: &Value) {
        let document = request
            .get("params")
            .and_then(|one| one.get("textDocument"));
        let uri = document
            .and_then(|one| one.get("uri"))
            .and_then(Value::as_str);
        let text = document
            .and_then(|one| one.get("text"))
            .and_then(Value::as_str);
        if let (Some(uri), Some(text)) = (uri, text) {
            self.documents.insert(uri.to_owned(), text.to_owned());
        }
    }

    /// Keeps the text a client's edit left, which the whole-document sync this server asks for sends entire.
    ///
    /// An edit it cannot read, or one that is a range of the document rather than all of it, leaves a buffer it no longer knows, so it forgets the document rather than keep bytes the client no longer has, and tells the client why.
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    fn changed(&mut self, output: &mut dyn Write, request: &Value) -> std::io::Result<()> {
        let params = request.get("params");
        let Some(uri) = params
            .and_then(|one| one.get("textDocument"))
            .and_then(|one| one.get("uri"))
            .and_then(Value::as_str)
        else {
            return Ok(());
        };
        let changes = params
            .and_then(|one| one.get("contentChanges"))
            .and_then(Value::as_array);
        let ranged = |one: &Value| one.get("range").is_some() || one.get("rangeLength").is_some();
        if changes.is_some_and(|changes| changes.iter().any(ranged)) {
            self.documents.remove(uri);
            return notify(
                output,
                "window/logMessage",
                &json!({
                    "type": 2,
                    "message": format!(
                        "the client sent an edit as a range though this server asked for whole \
                         documents, so it holds no bytes of {uri} and marks nothing there until \
                         the document is opened again"
                    ),
                }),
            );
        }
        match changes
            .and_then(|changes| changes.last())
            .and_then(|last| last.get("text"))
            .and_then(Value::as_str)
        {
            Some(text) => {
                self.documents.insert(uri.to_owned(), text.to_owned());
            }
            None => {
                self.documents.remove(uri);
            }
        }
        Ok(())
    }

    /// Forgets a document the client closed.
    fn closed(&mut self, request: &Value) {
        if let Some(uri) = request
            .get("params")
            .and_then(|one| one.get("textDocument"))
            .and_then(|one| one.get("uri"))
            .and_then(Value::as_str)
        {
            self.documents.remove(uri);
        }
    }

    /// The document a request is about, when the client holds exactly the bytes the latest run read of it.
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    fn guarded(
        &mut self,
        output: &mut dyn Write,
        root: &Path,
        request: &Value,
    ) -> std::io::Result<Option<Guarded>> {
        let Some(uri) = request
            .get("params")
            .and_then(|one| one.get("textDocument"))
            .and_then(|one| one.get("uri"))
            .and_then(Value::as_str)
        else {
            return Ok(None);
        };
        let (Some(text), Some(path)) = (self.documents.get(uri), path_of(uri, root)) else {
            return Ok(None);
        };
        let text = text.clone();
        self.refresh(output, root)?;
        let Some(latest) = &self.latest else {
            return Ok(None);
        };
        let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
        sha2::Digest::update(&mut hasher, text.as_bytes());
        if latest.recorded.get(&path) != Some(&rust_mutants::id::HexDigest::finish(hasher)) {
            return Ok(None);
        }
        match crate::spec::guarded(&latest.report, &path) {
            Ok((specification, path)) => Ok(Some(Guarded {
                specification,
                path,
                text,
            })),
            Err(crate::spec::SpecError::NamesNothing { .. }) => Ok(None),
            Err(error) => {
                notify(
                    output,
                    "window/logMessage",
                    &json!({ "type": 1, "message": error.to_string() }),
                )?;
                Ok(None)
            }
        }
    }

    /// Makes the run it holds the one the store points at now, reading its report only when that is another run.
    ///
    /// # Errors
    /// Returns the output stream's write failure.
    fn refresh(&mut self, output: &mut dyn Write, root: &Path) -> std::io::Result<()> {
        let pointed = match super::reports::Store::read(root)
            .and_then(|store| store.pointed_run(super::reports::Index::Any))
        {
            Ok(Some(pointed)) => pointed,
            Ok(None) => {
                self.latest = None;
                return Ok(());
            }
            Err(error) => {
                self.latest = None;
                return notify(
                    output,
                    "window/logMessage",
                    &json!({ "type": 1, "message": error.to_string() }),
                );
            }
        };
        let run = pointed.id().as_str().to_owned();
        if self.latest.as_ref().is_some_and(|latest| latest.run == run) {
            return Ok(());
        }
        notify(
            output,
            "window/logMessage",
            &json!({
                "type": 4,
                "message": format!("reading run {run} for the marks beside the code"),
            }),
        )?;
        let read = match pointed.document() {
            Ok(text) => match crate::strictjson::decode_str::<Report>(&text) {
                Ok(report) => match report.conclusion() {
                    Ok(conclusion) => Ok(Latest {
                        run,
                        recorded: conclusion.sources,
                        report,
                    }),
                    Err(error) => Err(error.to_string()),
                },
                Err(source) => Err(LatestError::Parse {
                    path: pointed.document_display(),
                    source,
                }
                .to_string()),
            },
            Err(error) => Err(LatestError::from(error).to_string()),
        };
        match read {
            Ok(latest) => {
                self.latest = Some(latest);
                Ok(())
            }
            Err(said) => {
                self.latest = None;
                notify(
                    output,
                    "window/logMessage",
                    &json!({ "type": 1, "message": said }),
                )
            }
        }
    }
}

impl Guarded {
    /// A mark at the end of every line a change starts on inside the lines `request` asks about, each with what stands behind it.
    fn hints(&self, request: &Value, encoding: Encoding) -> Value {
        let asked = |end: &str| {
            request
                .get("params")
                .and_then(|one| one.get("range"))
                .and_then(|one| one.get(end))
                .and_then(|one| one.get("line"))
                .and_then(Value::as_u64)
        };
        let from = asked("start").unwrap_or(0);
        let to = asked("end").unwrap_or(u64::MAX);
        let lines: Vec<&str> = self.text.split('\n').collect();
        let hints: Vec<Value> = self
            .specification
            .lines(&self.path)
            .iter()
            .filter_map(|line| {
                let at = line.number().saturating_sub(1);
                if u64::from(at) < from || u64::from(at) > to {
                    return None;
                }
                let text = match usize::try_from(at) {
                    Ok(index) => lines.get(index).copied().unwrap_or_default(),
                    Err(_beyond_this_platform) => "",
                };
                Some(json!({
                    "position": {
                        "line": at,
                        "character": encoding.width(text.trim_end_matches('\r')),
                    },
                    "label": crate::presentation::guard::labelled(line.section()),
                    "paddingLeft": true,
                    "tooltip": crate::presentation::guard::told(line),
                }))
            })
            .collect();
        Value::Array(hints)
    }

    /// A lens above the first change of every item of the file, saying how its changes stand and naming it as `njutest spec` reads it.
    fn lenses(&self) -> Value {
        let lenses: Vec<Value> = self
            .specification
            .items()
            .iter()
            .filter(|item| item.path() == self.path)
            .filter_map(|item| {
                let first = item.changes().iter().map(crate::spec::Change::line).min()?;
                let at = first.saturating_sub(1);
                Some(json!({
                    "range": {
                        "start": { "line": at, "character": 0 },
                        "end": { "line": at, "character": 0 },
                    },
                    "command": {
                        "title": crate::presentation::guard::summed(item),
                        "command": "njutest.spec",
                        "arguments": [format!("{}:{}", self.path, item.name())],
                    },
                }))
            })
            .collect();
        Value::Array(lenses)
    }
}

/// The file `uri` names, as a report names it from `root`, or nothing when it names no file under `root`.
///
/// `file://localhost/` is the local host RFC 8089 says it is, and a root the platform spells as a verbatim path is compared without its `//?/`.
#[must_use]
pub fn path_of(uri: &str, root: &Path) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let rest = match rest.strip_prefix("localhost") {
        Some(after) if after.starts_with('/') => after,
        Some(_) | None => rest,
    };
    let local = decoded(rest)?;
    let local = match local.as_bytes() {
        [b'/', drive, b':', ..] if drive.is_ascii_alphabetic() => local.get(1..)?.to_owned(),
        _ => local,
    };
    let slashed = match rust_mutants::id::slashed(root) {
        Ok(slashed) => slashed,
        Err(_not_text) => return None,
    };
    let root = slashed.strip_prefix("//?/").unwrap_or(&slashed);
    let within = local.get(..root.len())?;
    let drive = |one: &str| {
        let bytes = one.as_bytes();
        matches!(bytes, [letter, b':', ..] if letter.is_ascii_alphabetic())
    };
    let same = if drive(within) && drive(root) {
        within.eq_ignore_ascii_case(root)
    } else {
        within == root
    };
    if !same {
        return None;
    }
    let relative = local.get(root.len()..)?.strip_prefix('/')?;
    match rust_mutants::id::normalize_path(relative) {
        Ok(path) => Some(path),
        Err(_not_a_workspace_path) => None,
    }
}

/// `text` with every `%XX` escape a URI writes turned back into its byte, or nothing when an escape is malformed or the bytes are not UTF-8.
fn decoded(text: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, after)) = rest.split_first() {
        if byte == b'%' {
            let (hex, after) = after.split_at_checked(2)?;
            let hex = match std::str::from_utf8(hex) {
                Ok(hex) => hex,
                Err(_not_text) => return None,
            };
            match u8::from_str_radix(hex, 16) {
                Ok(value) => bytes.push(value),
                Err(_not_hex) => return None,
            }
            rest = after;
        } else {
            bytes.push(byte);
            rest = after;
        }
    }
    match String::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(_not_text) => None,
    }
}

/// `path` as the URI an editor holds the document under, every byte a URI path cannot carry as it is escaped.
///
/// A space, a `#`, a `?` or a `%` left as they are would make a client read the rest of the path as a fragment, a query or an escape, and put what was published under it on another document; [`path_of`] reads it back.
/// # Errors
/// Returns an error when `path` is not valid UTF-8 and therefore cannot be represented by the protocol without changing its identity.
pub fn uri_of(path: &Path) -> Result<String, rust_mutants::id::SlashedPathError> {
    let text = rust_mutants::id::slashed(path)?;
    let mut escaped = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/' | b':') {
            escaped.push(char::from(byte));
        } else {
            let written = write!(escaped, "%{byte:02X}");
            debug_assert!(written.is_ok(), "writing to a String cannot fail");
        }
    }
    if escaped.starts_with('/') {
        Ok(format!("file://{escaped}"))
    } else {
        Ok(format!("file:///{escaped}"))
    }
}

/// The report of the run that finished last, or nothing when none has.
fn latest(root: &Path) -> Result<Option<Report>, LatestError> {
    let Some(run) = super::reports::Store::read(root)?.pointed_run(super::reports::Index::Any)?
    else {
        return Ok(None);
    };
    let path = run.document_display();
    let text = run.document()?;
    crate::strictjson::decode_str(&text)
        .map(Some)
        .map_err(|source| LatestError::Parse { path, source })
}

#[derive(Debug, thiserror::Error)]
enum LatestError {
    #[error(transparent)]
    Store(#[from] super::reports::StoreError),
    #[error("parsing {path}: {source}")]
    Parse {
        path: String,
        #[source]
        source: serde_json::Error,
    },
}

/// Answers one request.
fn reply(output: &mut dyn Write, id: Option<&Value>, result: &Value) -> std::io::Result<()> {
    let message = json!({ "jsonrpc": "2.0", "id": id, "result": result });
    output.write_all(framed(&message).as_bytes())?;
    output.flush()
}

/// Tells the client something it did not ask for.
fn notify(output: &mut dyn Write, method: &str, params: &Value) -> std::io::Result<()> {
    let message = json!({ "jsonrpc": "2.0", "method": method, "params": params });
    output.write_all(framed(&message).as_bytes())?;
    output.flush()
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
