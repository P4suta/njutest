// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a libtest harness's own report establishes about one run of it: announced, closed, and every count agreeing (ADR 0046).

use crate::execute::Summary;

/// Which of a binary's tests a run asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Asked<'a> {
    /// Every test the binary holds.
    Whole,
    /// Exactly these, each given with `--exact`.
    Exact(&'a [String]),
}

/// A run whose harness finished and accounted for every test it announced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessReport {
    /// The harness's own closing line.
    pub summary: Summary,
    /// The tests its closing list named as failed.
    pub failed: Vec<String>,
    /// How many tests it announced, or nothing where the output kept only its tail.
    pub announced: Option<u32>,
}

/// Why a run's harness did not account for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unaccounted {
    /// No `running N tests` line opened the harness's report, and the output was kept whole.
    Unannounced,
    /// The report was announced and never reached a `test result:` line after it.
    Unfinished,
    /// The closing line's counts do not add up to the number of tests announced.
    CountsDisagree {
        /// How many tests the harness announced.
        announced: u32,
        /// How many its closing line accounts for.
        accounted: u32,
    },
    /// The harness announced another number of tests than the run asked for by name.
    SelectionDisagrees {
        /// How many tests the run asked for.
        asked: usize,
        /// How many the harness announced.
        announced: u32,
    },
    /// The closing line says `ok` with a failure, or `FAILED` with none.
    VerdictContradicts,
    /// The closing list of failures names another number of tests than failed, or one the run did not ask for.
    FailuresDisagree {
        /// How many tests the list names.
        named: usize,
        /// How many the closing line says failed.
        failed: u32,
    },
    /// The reports together count more tests than a count holds.
    TooMany,
    /// The process's exit status is neither the harness's success nor its failure status for what it said.
    ExitContradicts {
        /// The status the process ended with.
        code: i32,
    },
}

impl std::fmt::Display for Unaccounted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unannounced => {
                formatter.write_str("no `running N tests` line opened the harness's report")
            }
            Self::Unfinished => formatter.write_str(
                "the process ended before the harness closed its report with a `test result:` line",
            ),
            Self::CountsDisagree {
                announced,
                accounted,
            } => write!(
                formatter,
                "the harness announced {announced} tests and its closing line accounts for {accounted}"
            ),
            Self::SelectionDisagrees { asked, announced } => write!(
                formatter,
                "the run asked for {asked} tests by name and the harness announced {announced}"
            ),
            Self::VerdictContradicts => formatter
                .write_str("the closing line's verdict contradicts its own count of failures"),
            Self::FailuresDisagree { named, failed } => write!(
                formatter,
                "the closing list names {named} failed tests where the closing line counts {failed}, \
                 or names one the run did not ask for"
            ),
            Self::TooMany => {
                formatter.write_str("the reports together count more tests than a count holds")
            }
            Self::ExitContradicts { code } => write!(
                formatter,
                "the process exited with {code}, which is neither the harness's success nor its \
                 failure status for what it said"
            ),
        }
    }
}

/// What libtest's harness exits with when a test failed.
pub const FAILURE_STATUS: i32 = 101;

/// Every line of `output` that is text, in order, without its ending; a line that is not text is lost, never fatal.
fn text_lines(output: &[u8]) -> Vec<&str> {
    output
        .split(|byte| *byte == b'\n')
        .filter_map(|line| {
            match std::str::from_utf8(match line.strip_suffix(b"\r") {
                Some(without_return) => without_return,
                None => line,
            }) {
                Ok(text) => Some(text),
                Err(_not_text) => None,
            }
        })
        .collect()
}

/// The count a `running N tests` line announces.
pub(crate) fn announcement(line: &str) -> Option<u32> {
    let rest = line.trim().strip_prefix("running ")?;
    let (count, noun) = rest.split_once(' ')?;
    let count = match count.parse::<u32>() {
        Ok(count) => count,
        Err(_not_a_count) => return None,
    };
    let expected = if count == 1 { "test" } else { "tests" };
    (noun == expected).then_some(count)
}

/// The names the last `failures:` list before the closing line holds.
fn failure_list(lines: &[&str]) -> Vec<String> {
    let Some(header) = lines
        .iter()
        .rposition(|line| line.trim_end() == "failures:")
    else {
        return Vec::new();
    };
    lines
        .iter()
        .skip(header.saturating_add(1))
        .skip_while(|line| line.trim().is_empty())
        .take_while(|line| line.starts_with("    ") && !line.trim().is_empty())
        .map(|line| line.trim().to_owned())
        .collect()
}

/// Whether `names` names exactly as many tests as the harness counted.
fn same_count(names: usize, counted: u32) -> bool {
    u32::try_from(names).is_ok_and(|names| names == counted)
}

/// How many tests a closing line accounts for.
///
/// # Errors
/// [`Unaccounted::TooMany`] where its counts together pass what a count holds, which is no number of tests.
fn accounted(summary: &Summary) -> Result<u32, Unaccounted> {
    summary
        .passed
        .checked_add(summary.failed)
        .and_then(|sum| sum.checked_add(summary.ignored))
        .and_then(|sum| sum.checked_add(summary.measured))
        .ok_or(Unaccounted::TooMany)
}

/// Whether one report, announcing `announced` where it announced at all and closing with `summary` after `body`, accounts for itself: its counts agree, its verdict agrees with them, and its closing list names as many failures as it counts.
fn closed_report(
    body: &[&str],
    announced: Option<u32>,
    summary: &Summary,
) -> Result<Vec<String>, Unaccounted> {
    if let Some(announced) = announced {
        let accounted = accounted(summary)?;
        if accounted != announced {
            return Err(Unaccounted::CountsDisagree {
                announced,
                accounted,
            });
        }
    }
    if summary.ok != (summary.failed == 0) {
        return Err(Unaccounted::VerdictContradicts);
    }
    let failed = failure_list(body);
    if !same_count(failed.len(), summary.failed) {
        return Err(Unaccounted::FailuresDisagree {
            named: failed.len(),
            failed: summary.failed,
        });
    }
    Ok(failed)
}

/// Whether `exit` is the status a harness ends with for a report that says `ok`.
const fn exits_as_said(exit: Option<i32>, ok: bool) -> Result<(), Unaccounted> {
    match exit {
        Some(0) if ok => Ok(()),
        Some(FAILURE_STATUS) if !ok => Ok(()),
        Some(code) => Err(Unaccounted::ExitContradicts { code }),
        None => Ok(()),
    }
}

/// What the harness's own report in `output` establishes about the run that asked it for `asked` and ended with `exit`.
///
/// `exit` is the process's status where it exited with one; a process that ended any other way is judged by what it printed.
///
/// # Errors
/// Every way the report fails to account for the run, the first found in the order the report is read.
pub fn harness_report(
    output: &[u8],
    asked: Asked<'_>,
    exit: Option<i32>,
) -> Result<HarnessReport, Unaccounted> {
    let lines = text_lines(output);
    let truncated = lines
        .first()
        .is_some_and(|line| line.starts_with(crate::runner::OUTPUT_TRUNCATED_PREFIX));
    let opened = lines
        .iter()
        .enumerate()
        .find_map(|(at, line)| announcement(line).map(|count| (at, count)));
    let closed = lines.iter().enumerate().rev().find_map(|(at, line)| {
        crate::execute::parse_summary_line(line).map(|summary| (at, summary))
    });
    let (announced, summary, closing) = match (opened, closed) {
        (Some((open, count)), Some((close, summary))) if open < close => {
            (Some(count), summary, close)
        }
        (Some(_), Some(_) | None) => return Err(Unaccounted::Unfinished),
        (None, Some((close, summary))) if truncated => (None, summary, close),
        (None, Some(_) | None) => return Err(Unaccounted::Unannounced),
    };
    if let (Asked::Exact(names), Some(announced)) = (asked, announced)
        && accounted(&summary)? == announced
        && !same_count(names.len(), announced)
    {
        return Err(Unaccounted::SelectionDisagrees {
            asked: names.len(),
            announced,
        });
    }
    let failed = closed_report(lines.split_at(closing).0, announced, &summary)?;
    let foreign = match asked {
        Asked::Whole => false,
        Asked::Exact(names) => failed.iter().any(|name| !names.contains(name)),
    };
    if foreign {
        return Err(Unaccounted::FailuresDisagree {
            named: failed.len(),
            failed: summary.failed,
        });
    }
    exits_as_said(exit, summary.ok)?;
    Ok(HarnessReport {
        summary,
        failed,
        announced,
    })
}

/// The counts of two closing lines together, where they fit.
fn added(sum: Summary, one: &Summary) -> Option<Summary> {
    Some(Summary {
        ok: sum.ok && one.ok,
        passed: sum.passed.checked_add(one.passed)?,
        failed: sum.failed.checked_add(one.failed)?,
        ignored: sum.ignored.checked_add(one.ignored)?,
        measured: sum.measured.checked_add(one.measured)?,
        filtered_out: sum.filtered_out.checked_add(one.filtered_out)?,
    })
}

/// What a run whose harness prints one whole report after another establishes: rustdoc's, which prints the report of each merged doctest binary it ran and then its own, and ended with `exit`.
///
/// # Errors
/// Every way one of its reports fails to account for itself, the first found, and a run that printed no report at all.
pub fn harness_reports(output: &[u8], exit: Option<i32>) -> Result<HarnessReport, Unaccounted> {
    let lines = text_lines(output);
    let truncated = lines
        .first()
        .is_some_and(|line| line.starts_with(crate::runner::OUTPUT_TRUNCATED_PREFIX));
    let mut open: Option<(usize, Option<u32>)> = truncated.then_some((0, None));
    let mut summary = Summary {
        ok: true,
        passed: 0,
        failed: 0,
        ignored: 0,
        measured: 0,
        filtered_out: 0,
    };
    let mut failed = Vec::new();
    let mut announced = Some(0_u32);
    let mut reported = false;
    for (at, line) in lines.iter().enumerate() {
        if let Some(count) = announcement(line) {
            if matches!(open, Some((_, Some(_)))) {
                return Err(Unaccounted::Unfinished);
            }
            open = Some((at, Some(count)));
        } else if let Some(closing) = crate::execute::parse_summary_line(line) {
            let Some((start, said)) = open.take() else {
                return Err(Unaccounted::Unannounced);
            };
            let body = lines.split_at(at).0.split_at(start).1;
            failed.extend(closed_report(body, said, &closing)?);
            summary = added(summary, &closing).ok_or(Unaccounted::TooMany)?;
            announced = match (announced, said) {
                (Some(sum), Some(one)) => Some(sum.checked_add(one).ok_or(Unaccounted::TooMany)?),
                (None | Some(_), None) | (None, Some(_)) => None,
            };
            reported = true;
        }
    }
    if matches!(open, Some((_, Some(_)))) {
        return Err(Unaccounted::Unfinished);
    }
    if !reported {
        return Err(Unaccounted::Unannounced);
    }
    exits_as_said(exit, summary.ok)?;
    Ok(HarnessReport {
        summary,
        failed,
        announced,
    })
}

/// The tests a harness listed with `--list`, where the listing closed with the count libtest ends one with and every name it listed comes to that count; nothing where the harness did not account for its listing so.
#[must_use]
pub fn listing(output: &[u8]) -> Option<Vec<String>> {
    let Ok(text) = std::str::from_utf8(output) else {
        return None;
    };
    let mut lines: Vec<&str> = text.lines().filter(|line| !line.is_empty()).collect();
    let (tests, benchmarks) = closing(lines.pop()?)?;
    let mut names = Vec::new();
    let mut benched = 0_u32;
    for line in lines {
        if let Some(name) = line.strip_suffix(": test") {
            names.push(name.to_owned());
        } else if line.strip_suffix(": benchmark").is_some() {
            benched = benched.checked_add(1)?;
        } else {
            return None;
        }
    }
    let listed = match u32::try_from(names.len()) {
        Ok(listed) => listed,
        Err(_too_many) => return None,
    };
    (listed == tests && benched == benchmarks).then_some(names)
}

/// The counts a listing's closing line states: `2 tests, 1 benchmark`.
fn closing(line: &str) -> Option<(u32, u32)> {
    let (tests, benchmarks) = line.split_once(", ")?;
    Some((counted(tests, "test")?, counted(benchmarks, "benchmark")?))
}

/// The number `said` counts of `noun`, in the singular exactly where it is one.
fn counted(said: &str, noun: &str) -> Option<u32> {
    let (number, word) = said.split_once(' ')?;
    let Ok(number) = number.parse::<u32>() else {
        return None;
    };
    let plural = match word.strip_prefix(noun) {
        Some("") => Some(false),
        Some("s") => Some(true),
        Some(_) | None => None,
    };
    match (number, plural) {
        (1, Some(false)) => Some(1),
        (1, Some(true) | None) | (_, None | Some(false)) => None,
        (_, Some(true)) => Some(number),
    }
}

/// An option of libtest's own that an invocation sets itself, which the harness refuses to be given twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Own {
    /// One test at a time, on one thread.
    OneThread,
    /// Every test's output as the test writes it.
    Uncaptured,
}

impl Own {
    /// How an invocation spells it.
    #[must_use]
    pub const fn spelled(self) -> &'static str {
        match self {
            Self::OneThread => "--test-threads=1",
            Self::Uncaptured => "--nocapture",
        }
    }

    /// How much of the configured arguments from `word` on sets this option.
    fn takes(self, word: &str) -> Taken {
        match self {
            Self::OneThread if word == "--test-threads" => Taken::WordAndValue,
            Self::OneThread if word.starts_with("--test-threads=") => Taken::Word,
            Self::Uncaptured if word == "--nocapture" => Taken::Word,
            Self::OneThread | Self::Uncaptured => Taken::Nothing,
        }
    }
}

/// How much of the configured arguments one occurrence of an option spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Taken {
    /// None of them: the word sets no option the invocation sets itself.
    Nothing,
    /// The word alone, which holds its value or takes none.
    Word,
    /// The word and the value after it.
    WordAndValue,
}

/// The harness arguments a run was configured with, which reach a test binary only beside the options an invocation sets itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Configured(Vec<String>);

impl Configured {
    /// Holds `arguments` as a run was configured with them.
    #[must_use]
    pub const fn new(arguments: Vec<String>) -> Self {
        Self(arguments)
    }

    /// The harness arguments of an invocation that sets `own` itself: those options, then every configured argument but one setting any of them, whose value the invocation's own replaces rather than repeats.
    #[must_use]
    pub fn beside(&self, own: &[Own]) -> Vec<String> {
        let mut arguments: Vec<String> = own
            .iter()
            .map(|option| option.spelled().to_owned())
            .collect();
        let (mut value_follows, mut options_ended) = (false, false);
        for word in &self.0 {
            if std::mem::take(&mut value_follows) {
                continue;
            }
            let taken = if options_ended {
                None
            } else {
                own.iter().map(|option| option.takes(word)).max()
            };
            match taken {
                Some(Taken::WordAndValue) => value_follows = true,
                Some(Taken::Word) => {}
                Some(Taken::Nothing) | None => {
                    options_ended = options_ended || word == "--";
                    arguments.push(word.clone());
                }
            }
        }
        arguments
    }
}

#[cfg(test)]
mod tests;
