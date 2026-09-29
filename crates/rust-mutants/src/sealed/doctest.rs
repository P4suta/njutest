// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Doctests built for the sealed target: rustdoc hands every binary it would run to a capture, and what it prints says which doctest each one holds (ADR 0046).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The program rustdoc runs each doctest binary through: it keeps the binary under the next free claim, prints the claim, and fails, so that rustdoc prints which doctest the claim holds.
pub const CAPTURE_SOURCE: &str = r#"use std::io::ErrorKind;

fn main() -> std::process::ExitCode {
    let mut arguments = std::env::args_os().skip(1);
    let (Some(directory), Some(binary)) = (arguments.next(), arguments.last()) else {
        return std::process::ExitCode::from(3);
    };
    let directory = std::path::PathBuf::from(directory);
    let mut claim: u64 = 0;
    loop {
        let path = directory.join(format!("{claim}.claim"));
        match std::fs::OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_) => break,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => claim += 1,
            Err(_) => return std::process::ExitCode::from(3),
        }
    }
    let part = directory.join(format!("{claim}.part"));
    let kept = directory.join(format!("{claim}.wasm"));
    if std::fs::copy(&binary, &part).is_err() || std::fs::rename(&part, &kept).is_err() {
        return std::process::ExitCode::from(3);
    }
    println!("rust-mutants-captured {claim}");
    std::process::ExitCode::from(1)
}
"#;

/// What the capture prints before the claim of each binary it kept.
const MARKER: &str = "rust-mutants-captured ";

/// What rustdoc prints last where it ran merged doctests.
const MERGED_CLOSING: &str = "all doctests ran in ";

/// What rustdoc appends to the name of a doctest it only compiles.
const COMPILED_ONLY: [&str; 2] = [" - compile", " - compile fail"];

/// What libtest appends to the name of a test that should panic.
const SHOULD_PANIC: &str = " - should panic";

/// What rustdoc's `main` exits with when the doctest it ran returned an error: `ExitCode::FAILURE`.
pub const FAILURE_STATUS: i32 = 1;

/// The variable a merged doctest binary reads the index of the one doctest to run from.
pub const RUN_ONE: &str = "RUSTDOC_DOCTEST_RUN_NB_TEST";

/// What a merged doctest binary panics with when the index names no doctest it holds.
pub const NO_SUCH_INDEX: &str = "Unexpected value for `RUSTDOC_DOCTEST_RUN_NB_TEST`";

/// What a doctest passes by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expects {
    /// Returning.
    Return,
    /// Failing, as `should_panic` asks.
    Panic,
}

impl Expects {
    /// The harness the judgement reads a doctest with this expectation by.
    #[must_use]
    pub const fn harness(self) -> rust_mutants_decision::judgement::Harness {
        match self {
            Self::Return => rust_mutants_decision::judgement::Harness::Doctest,
            Self::Panic => rust_mutants_decision::judgement::Harness::ShouldPanic,
        }
    }
}

/// One doctest compiled into a binary of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alone {
    /// Its name, as rustdoc names it.
    pub name: String,
    /// The binary.
    pub binary: PathBuf,
    /// What it passes by.
    pub expects: Expects,
}

/// Every doctest of one library that rustdoc runs, as the capture received them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    /// Each binary holding the doctests of one merged compilation, which run one at a time by index.
    pub merged: Vec<PathBuf>,
    /// Each doctest compiled alone.
    pub alone: Vec<Alone>,
    /// Each doctest rustdoc runs whose compilation for the sealed target failed, by name.
    pub unbuilt: Vec<String>,
}

/// Why rustdoc's report of the doctests it built does not account for what the capture holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Uncaptured {
    /// rustdoc reported no doctest and no merged binary: the library did not build for the sealed target, or rustdoc stopped first.
    Unreported,
    /// A line of the report is neither a doctest's result nor part of the report around them.
    Unread {
        /// The line.
        line: String,
    },
    /// The report does not close, or its closing counts disagree with the doctests it names.
    CountsDisagree {
        /// How many it announced.
        announced: u32,
        /// How many it named or counted.
        accounted: u64,
    },
    /// A binary was captured out of the order rustdoc ran the doctests in.
    OutOfOrder {
        /// The doctest.
        name: String,
        /// The claim the capture printed for it.
        said: u64,
        /// The claim the order gives it.
        expected: u64,
    },
    /// The capture holds another number of claims than the report accounts for.
    ClaimsDisagree {
        /// How many the report accounts for.
        reported: u64,
        /// How many the capture holds.
        held: u64,
    },
    /// A claim the report accounts for holds no binary.
    Missing {
        /// The claim.
        claim: u64,
    },
}

impl std::fmt::Display for Uncaptured {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreported => formatter.write_str(
                "rustdoc reported no doctest, so the library did not build for the sealed target",
            ),
            Self::Unread { line } => write!(
                formatter,
                "rustdoc printed {line:?}, which is not part of a report of doctests"
            ),
            Self::CountsDisagree {
                announced,
                accounted,
            } => write!(
                formatter,
                "rustdoc announced {announced} doctests and its report accounts for {accounted}"
            ),
            Self::OutOfOrder {
                name,
                said,
                expected,
            } => write!(
                formatter,
                "{name} was captured as binary {said} where the order rustdoc ran them in makes it \
                 {expected}"
            ),
            Self::ClaimsDisagree { reported, held } => write!(
                formatter,
                "rustdoc's report accounts for {reported} captured binaries and the capture holds \
                 {held}"
            ),
            Self::Missing { claim } => {
                write!(formatter, "the capture holds no binary for claim {claim}")
            }
        }
    }
}

/// What the capture's directory holds: how many claims it gave out, and the binary each kept one holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Held {
    /// How many claims it gave out.
    pub claims: u64,
    /// The binary of each claim it kept.
    pub binaries: BTreeMap<u64, PathBuf>,
}

impl Held {
    /// What `directory` holds.
    ///
    /// # Errors
    /// A directory that cannot be listed.
    pub fn read(directory: &Path) -> std::io::Result<Self> {
        let mut held = Self::default();
        for entry in std::fs::read_dir(directory)? {
            let path = entry?.path();
            let Some((stem, extension)) = path
                .file_stem()
                .and_then(std::ffi::OsStr::to_str)
                .zip(path.extension().and_then(std::ffi::OsStr::to_str))
            else {
                continue;
            };
            let Ok(claim) = stem.parse::<u64>() else {
                continue;
            };
            match extension {
                "claim" => held.claims = held.claims.saturating_add(1),
                "wasm" => {
                    held.binaries.insert(claim, path);
                }
                _ => {}
            }
        }
        Ok(held)
    }
}

/// One doctest's result line: its name and the word libtest ended it with.
struct Ran<'a> {
    name: &'a str,
    word: &'a str,
}

/// What rustdoc printed, in its parts.
#[derive(Default)]
struct Report<'a> {
    merged: Vec<u64>,
    announced: Option<u32>,
    results: Vec<Ran<'a>>,
    blocks: BTreeMap<&'a str, Vec<u64>>,
    summary: Option<crate::execute::Summary>,
    merged_closed: bool,
}

/// The claim a marker line printed.
fn marker(line: &str) -> Option<u64> {
    match line.strip_prefix(MARKER)?.trim_end().parse::<u64>() {
        Ok(claim) => Some(claim),
        Err(_not_a_claim) => None,
    }
}

/// A doctest's result line, `test NAME ... WORD`.
fn ran(line: &str) -> Option<Ran<'_>> {
    let (name, word) = line.strip_prefix("test ")?.split_once(" ... ")?;
    Some(Ran {
        name,
        word: word.trim_end(),
    })
}

/// Reads rustdoc's standard output into its parts.
fn report(stdout: &str) -> Result<Report<'_>, Uncaptured> {
    let mut report = Report::default();
    let mut block: Option<&str> = None;
    let mut failures = 0_u8;
    for line in stdout.lines() {
        let unread = || Uncaptured::Unread {
            line: line.to_owned(),
        };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(summary) = crate::execute::parse_summary_line(line) {
            report.summary = Some(summary);
            block = None;
            continue;
        }
        if line.starts_with(MERGED_CLOSING) {
            report.merged_closed = true;
            continue;
        }
        if report.announced.is_none() {
            match (marker(line), crate::libtest::announcement(line)) {
                (Some(claim), _) => report.merged.push(claim),
                (None, Some(count)) => report.announced = Some(count),
                (None, None) => return Err(unread()),
            }
            continue;
        }
        if line.trim_end() == "failures:" {
            failures = failures.saturating_add(1);
            block = None;
            continue;
        }
        match failures {
            0 => report.results.push(ran(line).ok_or_else(unread)?),
            1 => {
                if let Some(name) = line
                    .strip_prefix("---- ")
                    .and_then(|rest| rest.strip_suffix(" stdout ----"))
                {
                    block = Some(name);
                    report.blocks.entry(name).or_default();
                } else if let (Some(name), Some(claim)) = (block, marker(line)) {
                    report.blocks.entry(name).or_default().push(claim);
                }
            }
            _ => {
                if !line.starts_with("    ") {
                    return Err(unread());
                }
            }
        }
    }
    Ok(report)
}

/// Whether the report closes with counts that agree with the doctests it names.
fn closed(report: &Report<'_>) -> Result<(), Uncaptured> {
    let Some(announced) = report.announced else {
        return match (report.merged.is_empty(), report.merged_closed) {
            (false, true) => Ok(()),
            (true, _) => Err(Uncaptured::Unreported),
            (false, false) => Err(Uncaptured::CountsDisagree {
                announced: 0,
                accounted: 0,
            }),
        };
    };
    let named = match u64::try_from(report.results.len()) {
        Ok(named) => named,
        Err(_wider) => u64::MAX,
    };
    let counted = report.summary.map_or(u64::MAX, |summary| {
        [
            summary.passed,
            summary.failed,
            summary.ignored,
            summary.measured,
        ]
        .into_iter()
        .map(u64::from)
        .sum()
    });
    let merged_closed = report.merged.is_empty() || report.merged_closed;
    if named != u64::from(announced) || counted != u64::from(announced) || !merged_closed {
        return Err(Uncaptured::CountsDisagree {
            announced,
            accounted: if named == u64::from(announced) {
                counted
            } else {
                named
            },
        });
    }
    Ok(())
}

/// What rustdoc printed on its standard output while it handed every doctest binary it built to the capture, read against what the capture holds.
///
/// # Errors
/// Every way the report fails to account for what the capture holds, the first found.
pub fn captured(stdout: &[u8], held: &Held) -> Result<Captured, Uncaptured> {
    let text = std::str::from_utf8(stdout).map_err(|_not_text| Uncaptured::Unread {
        line: crate::telling::LosslessBytes::new(stdout).to_string(),
    })?;
    let report = report(text)?;
    closed(&report)?;
    let mut next: u64 = 0;
    let mut captured = Captured::default();
    for claim in &report.merged {
        if *claim != next {
            return Err(Uncaptured::OutOfOrder {
                name: "a merged compilation".to_owned(),
                said: *claim,
                expected: next,
            });
        }
        captured.merged.push(kept(held, *claim)?);
        next = next.saturating_add(1);
    }
    for doctest in &report.results {
        let compiled_only = COMPILED_ONLY
            .iter()
            .any(|suffix| doctest.name.ends_with(suffix));
        let ignored = doctest.word == "ignored" || doctest.word.starts_with("ignored, ");
        if compiled_only || ignored {
            continue;
        }
        let name = doctest
            .name
            .strip_suffix(SHOULD_PANIC)
            .unwrap_or(doctest.name);
        let expects = match (
            doctest.word,
            report.blocks.get(doctest.name).map(Vec::as_slice),
        ) {
            ("ok", _) => Expects::Panic,
            ("FAILED", None | Some([])) => {
                captured.unbuilt.push(name.to_owned());
                continue;
            }
            ("FAILED", Some([claim])) if *claim == next => Expects::Return,
            ("FAILED", Some([claim])) => {
                return Err(Uncaptured::OutOfOrder {
                    name: name.to_owned(),
                    said: *claim,
                    expected: next,
                });
            }
            (_, _) => {
                return Err(Uncaptured::Unread {
                    line: format!("test {} ... {}", doctest.name, doctest.word),
                });
            }
        };
        captured.alone.push(Alone {
            name: name.to_owned(),
            binary: kept(held, next)?,
            expects,
        });
        next = next.saturating_add(1);
    }
    if next != held.claims {
        return Err(Uncaptured::ClaimsDisagree {
            reported: next,
            held: held.claims,
        });
    }
    Ok(captured)
}

/// The binary the capture kept under `claim`.
fn kept(held: &Held, claim: u64) -> Result<PathBuf, Uncaptured> {
    held.binaries
        .get(&claim)
        .cloned()
        .ok_or(Uncaptured::Missing { claim })
}

/// The name the station of the doctests gives a doctest the native run named `native`, or nothing where rustdoc only compiles it.
#[must_use]
pub fn natively_run(native: &str) -> Option<String> {
    if COMPILED_ONLY.iter().any(|suffix| native.ends_with(suffix)) {
        return None;
    }
    Some(
        native
            .strip_suffix(SHOULD_PANIC)
            .unwrap_or(native)
            .to_owned(),
    )
}

/// One doctest a merged binary holds, at its index there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// Its name, as rustdoc names it.
    pub name: String,
    /// What it passes by.
    pub expects: Expects,
    /// Whether the binary's harness ignored it, which it does to one ignored for the sealed target and to every one that should panic.
    pub ignored: bool,
}

/// What a merged binary's own harness printed of the doctests it ran in one instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Printed {
    /// How many doctests it announced it would run.
    pub announced: u32,
    /// Each doctest it finished, in index order.
    pub finished: Vec<Listed>,
    /// The doctest it began after those and never finished, where it stopped inside one.
    pub stopped: Option<Listed>,
    /// Whether it finished every doctest it announced, filtered none out, and closed with counts that agree.
    pub whole: bool,
}

/// The doctest a result line of a merged binary's harness names.
fn listing(name: &str, ignored: bool) -> Listed {
    match name.strip_suffix(SHOULD_PANIC) {
        Some(name) => Listed {
            name: name.to_owned(),
            expects: Expects::Panic,
            ignored,
        },
        None => Listed {
            name: name.to_owned(),
            expects: Expects::Return,
            ignored,
        },
    }
}

/// What a merged binary's own harness printed when it ran its doctests in one instance, where it announced them and began none after one it never finished.
#[must_use]
pub fn printed(stdout: &[u8]) -> Option<Printed> {
    let Ok(text) = std::str::from_utf8(stdout) else {
        return None;
    };
    let mut announced = None;
    let mut finished = Vec::new();
    let mut stopped = None;
    let mut summary = None;
    for line in text.lines() {
        if announced.is_none() {
            announced = crate::libtest::announcement(line);
            continue;
        }
        if let Some(closing) = crate::execute::parse_summary_line(line) {
            summary = Some(closing);
            break;
        }
        if let Some((name, word)) = line
            .strip_prefix("test ")
            .and_then(|rest| rest.split_once(" ... "))
        {
            if stopped.is_some() {
                return None;
            }
            if word.is_empty() {
                stopped = Some(listing(name, false));
            } else {
                finished.push(listing(name, word.starts_with("ignored")));
            }
        }
    }
    let announced = announced?;
    let whole = stopped.is_none()
        && summary.is_some_and(|summary| {
            let accounted = summary
                .passed
                .checked_add(summary.failed)
                .and_then(|sum| sum.checked_add(summary.ignored))
                .and_then(|sum| sum.checked_add(summary.measured));
            let listed = u32::try_from(finished.len()).is_ok_and(|listed| listed == announced);
            listed && accounted == Some(announced) && summary.filtered_out == 0
        });
    Some(Printed {
        announced,
        finished,
        stopped,
        whole,
    })
}

/// The name rustdoc indexes a doctest of a merged binary by: its name, less what libtest appends to one it only compiles.
fn indexed(listed: &Listed) -> &str {
    listed
        .name
        .strip_suffix(COMPILED_ONLY[0])
        .unwrap_or(&listed.name)
}

/// The doctests the only merged binary of `captured` holds past the ones its harness `printed` before it stopped, named from `native`, the names the native run passed its doctests under: each that rustdoc merged, being neither one that must fail to compile nor one `captured` holds apart, in the order rustdoc indexes them; nothing where `captured` has another merged binary, which leaves which of them holds a doctest unsaid, or where one of them sorts before a doctest printed, which no binary that runs its doctests in order can hold.
#[must_use]
pub fn unprinted(
    native: &[String],
    captured: &Captured,
    printed: &[Listed],
) -> Option<Vec<Listed>> {
    if captured.merged.len() != 1 {
        return None;
    }
    let apart: BTreeSet<&str> = captured
        .alone
        .iter()
        .map(|alone| alone.name.as_str())
        .chain(captured.unbuilt.iter().map(String::as_str))
        .chain(printed.iter().map(|listed| listed.name.as_str()))
        .collect();
    let mut past: Vec<Listed> = native
        .iter()
        .filter(|name| !name.ends_with(COMPILED_ONLY[1]))
        .map(|name| listing(name, false))
        .filter(|listed| !apart.contains(listed.name.as_str()))
        .collect();
    past.sort_by(|one, other| indexed(one).cmp(indexed(other)));
    let last = printed.last().map(indexed);
    past.iter()
        .all(|listed| last.is_none_or(|last| indexed(listed) > last))
        .then_some(past)
}

/// The doctests a merged binary's own harness names when it runs every one of them in one instance, in index order, where it announced them, filtered none out, and closed.
#[must_use]
pub fn listed(stdout: &[u8]) -> Option<Vec<Listed>> {
    printed(stdout)
        .filter(|printed| printed.whole)
        .map(|printed| printed.finished)
}

#[cfg(test)]
mod tests;
