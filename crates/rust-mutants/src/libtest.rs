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
pub struct Account {
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
        .filter_map(
            |line| match std::str::from_utf8(line.strip_suffix(b"\r").unwrap_or(line)) {
                Ok(text) => Some(text),
                Err(_not_text) => None,
            },
        )
        .collect()
}

/// The count a `running N tests` line announces.
fn announcement(line: &str) -> Option<u32> {
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

/// What the harness's own report in `output` establishes about the run that asked it for `asked` and ended with `exit`.
///
/// `exit` is the process's status where it exited with one; a process that ended any other way is judged by what it printed.
///
/// # Errors
/// Every way the report fails to account for the run, the first found in the order the report is read.
pub fn account(output: &[u8], asked: Asked<'_>, exit: Option<i32>) -> Result<Account, Unaccounted> {
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
    let accounted = summary
        .passed
        .checked_add(summary.failed)
        .and_then(|sum| sum.checked_add(summary.ignored))
        .and_then(|sum| sum.checked_add(summary.measured))
        .unwrap_or(u32::MAX);
    if let Some(announced) = announced
        && accounted != announced
    {
        return Err(Unaccounted::CountsDisagree {
            announced,
            accounted,
        });
    }
    if let (Asked::Exact(names), Some(announced)) = (asked, announced)
        && !same_count(names.len(), announced)
    {
        return Err(Unaccounted::SelectionDisagrees {
            asked: names.len(),
            announced,
        });
    }
    if summary.ok != (summary.failed == 0) {
        return Err(Unaccounted::VerdictContradicts);
    }
    let failed = failure_list(lines.get(..closing).unwrap_or_default());
    let foreign = match asked {
        Asked::Whole => false,
        Asked::Exact(names) => failed.iter().any(|name| !names.contains(name)),
    };
    if !same_count(failed.len(), summary.failed) || foreign {
        return Err(Unaccounted::FailuresDisagree {
            named: failed.len(),
            failed: summary.failed,
        });
    }
    match exit {
        Some(0) if summary.ok => {}
        Some(FAILURE_STATUS) if !summary.ok => {}
        Some(code) => return Err(Unaccounted::ExitContradicts { code }),
        None => {}
    }
    Ok(Account {
        summary,
        failed,
        announced,
    })
}

#[cfg(test)]
mod tests;
