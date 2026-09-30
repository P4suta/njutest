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

/// Which test arguments a capture builds the doctests with, which a merged binary bakes in at compile time and answers whatever else it is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Baked {
    /// None beyond the run's own: each doctest runs as itself when its binary runs.
    Run,
    /// `--list`: a merged binary lists the doctests it holds when it runs, and rustdoc lists the doctests it did not merge itself rather than running them.
    List,
    /// `--list --ignored`: a merged binary lists the doctests it holds that the sealed target ignores, which run as nothing when asked for by index.
    ListIgnored,
}

/// Every doctest of one library that rustdoc runs, as the capture received them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Captured {
    /// Each binary holding the doctests of one merged compilation, which run one at a time by index.
    pub merged: Vec<PathBuf>,
    /// Each binary holding the doctests of one merged compilation, built to list the ones the sealed target ignores, one per entry of `merged`, in its order.
    pub ignored: Vec<PathBuf>,
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
    /// The report announced its doctests and never closed, so it counts none of them.
    Unclosed {
        /// How many it announced.
        announced: u32,
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
            Self::Unclosed { announced } => write!(
                formatter,
                "rustdoc announced {announced} doctests and its report never closed"
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
    let Some(summary) = report.summary else {
        return Err(Uncaptured::Unclosed { announced });
    };
    let counted: u64 = [
        summary.passed,
        summary.failed,
        summary.ignored,
        summary.measured,
    ]
    .into_iter()
    .map(u64::from)
    .sum();
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
        let name = match doctest.name.strip_suffix(SHOULD_PANIC) {
            Some(expecting_a_panic) => expecting_a_panic,
            None => doctest.name,
        };
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
        match native.strip_suffix(SHOULD_PANIC) {
            Some(expecting_a_panic) => expecting_a_panic,
            None => native,
        }
        .to_owned(),
    )
}

/// What a build of one library's doctests with `--list` among their test arguments printed: the claim of every merged binary, each of which lists the doctests it holds when it runs, and the name of every doctest rustdoc listed itself, which it did not merge.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    /// The claim of each merged binary, in the order rustdoc ran them.
    pub merged: Vec<u64>,
    /// Every doctest rustdoc listed itself, by name.
    pub standalone: Vec<String>,
}

/// What libtest prints after the name of each test it lists.
const LISTED: &str = ": test";

/// The count of tests and of benchmarks a listing closes with, `N tests, M benchmarks`.
fn discovered(line: &str) -> Option<(u32, u32)> {
    let (tests, benchmarks) = line.trim_end().split_once(", ")?;
    let count = |part: &str, noun: &str| -> Option<u32> {
        let (number, word) = part.split_once(' ')?;
        let number = match number.parse::<u32>() {
            Ok(number) => number,
            Err(_not_a_count) => return None,
        };
        let expected = if number == 1 {
            noun.to_owned()
        } else {
            format!("{noun}s")
        };
        (word == expected).then_some(number)
    };
    Some((count(tests, "test")?, count(benchmarks, "benchmark")?))
}

/// What rustdoc printed on its standard output while it built one library's doctests with `--list` among their test arguments and handed every merged binary to the capture.
///
/// # Errors
/// [`Uncaptured::Unreported`] where it printed nothing it listed or ran, and every other way the report fails to be one, the first found.
pub fn listing(stdout: &[u8]) -> Result<Listing, Uncaptured> {
    let text = std::str::from_utf8(stdout).map_err(|_not_text| Uncaptured::Unread {
        line: crate::telling::LosslessBytes::new(stdout).to_string(),
    })?;
    let mut listing = Listing::default();
    let mut counted = None;
    let mut merged_closed = false;
    for line in text.lines() {
        let unread = || Uncaptured::Unread {
            line: line.to_owned(),
        };
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(MERGED_CLOSING) {
            merged_closed = true;
            continue;
        }
        let listed_yet = !listing.standalone.is_empty() || counted.is_some();
        if let Some(claim) = marker(line) {
            if listed_yet {
                return Err(unread());
            }
            listing.merged.push(claim);
            continue;
        }
        if counted.is_some() {
            return Err(unread());
        }
        if let Some(count) = discovered(line) {
            counted = Some(count);
            continue;
        }
        let name = line.strip_suffix(LISTED).ok_or_else(unread)?;
        listing.standalone.push(name.to_owned());
    }
    let named =
        u32::try_from(listing.standalone.len()).map_err(|_wide| Uncaptured::CountsDisagree {
            announced: u32::MAX,
            accounted: u64::MAX,
        })?;
    match counted {
        Some((tests, 0)) if tests == named => {}
        Some((tests, _)) => {
            return Err(Uncaptured::CountsDisagree {
                announced: tests,
                accounted: u64::from(named),
            });
        }
        None if !listing.standalone.is_empty() => {
            return Err(Uncaptured::CountsDisagree {
                announced: 0,
                accounted: u64::from(named),
            });
        }
        None if !listing.merged.is_empty() && merged_closed => {}
        None if !listing.merged.is_empty() => {
            return Err(Uncaptured::CountsDisagree {
                announced: 0,
                accounted: 0,
            });
        }
        None => return Err(Uncaptured::Unreported),
    }
    Ok(listing)
}

/// The binary each claim `listing` printed holds, in the order rustdoc ran them, where the claims are every one the capture gave out and none is out of order.
///
/// # Errors
/// A claim out of order, one the capture holds no binary for, or claims the capture gave out that the report does not account for.
pub fn merged_binaries(listing: &Listing, held: &Held) -> Result<Vec<PathBuf>, Uncaptured> {
    let mut next: u64 = 0;
    let mut binaries = Vec::with_capacity(listing.merged.len());
    for claim in &listing.merged {
        if *claim != next {
            return Err(Uncaptured::OutOfOrder {
                name: "a merged compilation".to_owned(),
                said: *claim,
                expected: next,
            });
        }
        binaries.push(kept(held, *claim)?);
        next = next
            .checked_add(1)
            .ok_or(Uncaptured::Missing { claim: *claim })?;
    }
    if next != held.claims {
        return Err(Uncaptured::ClaimsDisagree {
            reported: next,
            held: held.claims,
        });
    }
    Ok(binaries)
}

/// The doctests a merged binary built with `--list` among its test arguments names when it runs with no index, in index order: its harness lists every one it holds, in the order it holds them, whatever any of them does when it runs, and closes with their count.
#[must_use]
pub fn listed(stdout: &[u8]) -> Option<Vec<String>> {
    let text = match std::str::from_utf8(stdout) {
        Ok(text) => text,
        Err(_not_text) => return None,
    };
    let mut names = Vec::new();
    let mut counted = None;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        if counted.is_some() {
            return None;
        }
        if let Some(count) = discovered(line) {
            counted = Some(count);
            continue;
        }
        names.push(line.strip_suffix(LISTED)?.to_owned());
    }
    let (tests, benchmarks) = counted?;
    let whole = benchmarks == 0 && usize::try_from(tests).is_ok_and(|tests| tests == names.len());
    let unique = names.iter().collect::<BTreeSet<_>>().len() == names.len();
    (whole && unique).then_some(names)
}

#[cfg(test)]
mod tests;
