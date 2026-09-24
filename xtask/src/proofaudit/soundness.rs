// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What interpreting the suite established, re-derived from the recorded run of the interpreter and what it said, and held to what the report says of soundness.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Audit, Layer, Notes, Recording, field, rows};

/// What Miri says when it has found unsoundness.
pub const UNDEFINED: &str = "Undefined Behavior";

/// What Miri says when it cannot interpret the suite whole.
pub const UNSUPPORTED: [&str; 3] = [
    "unsupported operation",
    "can't call foreign function",
    "unsupported target",
];

/// What a toolchain says when there is nothing to interpret with.
pub const ABSENT: [&str; 3] = ["no such command", "no such subcommand", "is not installed"];

/// What starts each line in which libtest says how one test binary ended.
pub const RESULT: &str = "test result: ";

/// What such a line says next when a test of the binary failed.
pub const FAILED: &str = "FAILED";

/// One recorded run of a program, and what it said where the recording kept it.
#[derive(Debug, Clone, Copy)]
pub struct Said<'a> {
    /// The run's exec record.
    pub exec: &'a Value,
    /// What it printed, where the recording kept a copy the audit could read whole.
    pub output: Option<&'a str>,
}

/// What one interpretation came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Came {
    /// The toolchain had nothing to interpret with.
    Absent,
    /// It ran out of time.
    TimedOut,
    /// It found unsoundness.
    Undefined,
    /// It could not interpret the suite whole.
    Unsupported,
    /// A test failed under it.
    Failed,
    /// Every test it ran passed, and it ran some.
    Passed,
    /// It ended with no test result.
    RanNoTest,
}

impl Came {
    /// Whether the suite counts as interpreted.
    const fn executed(self) -> bool {
        match self {
            Self::Undefined | Self::Unsupported | Self::Failed | Self::Passed => true,
            Self::Absent | Self::TimedOut | Self::RanNoTest => false,
        }
    }

    /// The kinds of the findings about soundness it earns.
    fn findings(self) -> BTreeSet<&'static str> {
        match self {
            Self::Undefined => BTreeSet::from(["undefined-behaviour"]),
            Self::Failed => BTreeSet::from(["failing-test"]),
            Self::Absent | Self::Unsupported | Self::RanNoTest => BTreeSet::from(["not-measured"]),
            Self::TimedOut | Self::Passed => BTreeSet::new(),
        }
    }
}

/// Whether `exec` is the interpreter run over the suite.
#[must_use]
pub fn interprets(exec: &Value) -> bool {
    words(exec).windows(2).any(|pair| pair == ["miri", "test"])
}

/// Whether `exec` is the question a failed interpretation asks of the toolchain afterwards.
fn asks_for_the_interpreter(exec: &Value) -> bool {
    words(exec)
        .windows(2)
        .any(|pair| pair == ["miri", "--version"])
}

/// The words of `exec`'s command line.
fn words(exec: &Value) -> Vec<&str> {
    exec.get("argv")
        .and_then(Value::as_array)
        .map_or_else(Vec::new, |argv| {
            argv.iter().filter_map(Value::as_str).collect()
        })
}

/// How `exec` ended: `Some(code)` for a process that exited with one, and its `stopped` kind otherwise.
fn ended(exec: &Value) -> (String, Option<i64>) {
    let stopped = exec.get("stopped");
    let kind = stopped
        .and_then(|one| field(one, "kind"))
        .unwrap_or_default();
    let code = stopped
        .and_then(|one| one.get("exit"))
        .filter(|exit| field(exit, "kind").as_deref() == Some("code"))
        .and_then(|exit| exit.get("value"))
        .and_then(Value::as_i64);
    (kind, code)
}

/// What the interpretation `run` came to, with the toolchain's answer `probe` where it was asked, or nothing where what it said was not kept whole.
fn came(run: Said<'_>, probe: Option<&Value>) -> Option<Came> {
    let said = run.output?;
    let (kind, code) = ended(run.exec);
    if kind == "not-started" || ABSENT.iter().any(|marker| said.contains(marker)) {
        return Some(Came::Absent);
    }
    if kind == "timed-out" || kind == "stalled" {
        return Some(Came::TimedOut);
    }
    if code != Some(0) && probe.is_some_and(|asked| ended(asked).1 != Some(0)) {
        return Some(Came::Absent);
    }
    if said.contains(UNDEFINED) {
        return Some(Came::Undefined);
    }
    if UNSUPPORTED.iter().any(|marker| said.contains(marker)) {
        return Some(Came::Unsupported);
    }
    let results: Vec<&str> = said
        .lines()
        .filter_map(|line| line.trim().strip_prefix(RESULT))
        .collect();
    let failed = results.iter().any(|result| result.starts_with(FAILED));
    Some(match code {
        Some(0) if !results.is_empty() && !failed => Came::Passed,
        Some(code) if code != 0 && failed => Came::Failed,
        Some(_) | None => Came::RanNoTest,
    })
}

/// Holds what the report says of soundness to what the recorded interpretation came to, in both directions; `execs` is every exec record of the runner's recording and `outputs` what the recording kept of each run it re-derives from.
pub(super) fn audited(
    recording: &Recording<'_>,
    execs: Option<&[Value]>,
    outputs: &[(String, String)],
    audit: &mut Audit,
) {
    let mut notes = Notes::on(audit, Layer::Soundness);
    let reported = Reported::of(recording);
    let Some(execs) = execs else {
        if reported.executed == Interpreted::Said || !reported.claimed.is_empty() {
            notes.unaudited(
                "soundness",
                "the run kept no recording, so what the interpreter came to cannot be re-derived"
                    .to_owned(),
            );
        }
        return;
    };
    let said = |exec: &Value| -> Option<String> {
        let kept = field(exec, "output_path")?;
        outputs
            .iter()
            .find(|(path, _text)| *path == kept)
            .map(|(_path, text)| text.clone())
    };
    let runs: Vec<&Value> = execs.iter().filter(|exec| interprets(exec)).collect();
    let probe = execs.iter().find(|exec| asks_for_the_interpreter(exec));
    match runs.as_slice() {
        [] => never_ran(&reported, &mut notes),
        [one] => {
            let output = said(one);
            match came(
                Said {
                    exec: one,
                    output: output.as_deref(),
                },
                probe,
            ) {
                Some(derived) => compared(&reported, derived, &mut notes),
                None => notes.unaudited(
                    "soundness",
                    "what the interpreter said was not kept whole, so what it came to cannot be \
                     re-derived"
                        .to_owned(),
                ),
            }
        }
        several => notes.unaudited(
            "soundness",
            format!(
                "the recording holds {} runs of the interpreter and the report is one build's",
                several.len()
            ),
        ),
    }
}

/// What a report says about whether the suite was interpreted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Interpreted {
    /// It says the suite was interpreted.
    Said,
    /// It says the suite was not.
    Denied,
    /// It does not say.
    Unsaid,
}

impl Interpreted {
    /// What a report whose `executed` field holds `value` says.
    fn of(value: Option<&Value>) -> Self {
        match value.and_then(Value::as_bool) {
            Some(true) => Self::Said,
            Some(false) => Self::Denied,
            None => Self::Unsaid,
        }
    }

    /// What a report says when the interpreter's run came to `executed`.
    const fn owed(executed: bool) -> Self {
        if executed { Self::Said } else { Self::Denied }
    }
}

/// What the report says of soundness.
struct Reported {
    /// Whether it says the suite was interpreted.
    executed: Interpreted,
    /// The kinds of its findings about soundness.
    claimed: BTreeSet<String>,
}

impl Reported {
    /// What `recording`'s report says of soundness.
    fn of(recording: &Recording<'_>) -> Self {
        Self {
            executed: Interpreted::of(recording.document.pointer("/accounting/soundness/executed")),
            claimed: rows(recording.document, "findings")
                .iter()
                .filter(|finding| field(finding, "subject").as_deref() == Some("soundness"))
                .filter_map(|finding| field(finding, "kind"))
                .collect(),
        }
    }
}

/// Holds a report whose run recorded no interpreter to saying it interpreted nothing and found nothing under it.
fn never_ran(reported: &Reported, notes: &mut Notes<'_>) {
    if reported.executed == Interpreted::Said {
        notes.violated(
            "soundness",
            "the report says the suite was interpreted and the recording holds no run of the \
             interpreter"
                .to_owned(),
        );
    }
    for kind in &reported.claimed {
        if kind == "failing-test" || kind == "undefined-behaviour" {
            notes.violated(
                "soundness",
                format!("the report names a {kind} under the interpreter it never ran"),
            );
        }
    }
}

/// Holds the report to what the one recorded interpretation came to.
fn compared(reported: &Reported, derived: Came, notes: &mut Notes<'_>) {
    if reported.executed != Interpreted::owed(derived.executed()) {
        notes.violated(
            "soundness",
            format!(
                "the report says the suite was{} interpreted, and what the interpreter said \
                 comes to {derived:?}",
                if reported.executed == Interpreted::Said {
                    ""
                } else {
                    " not"
                }
            ),
        );
    }
    let owed: BTreeSet<String> = derived.findings().into_iter().map(str::to_owned).collect();
    let judged: BTreeSet<String> = reported
        .claimed
        .iter()
        .filter(|kind| {
            *kind == "failing-test" || *kind == "undefined-behaviour" || *kind == "not-measured"
        })
        .cloned()
        .collect();
    if judged != owed {
        notes.violated(
            "soundness",
            format!(
                "the report's findings about soundness are {judged:?}, and what the interpreter \
                 said comes to {derived:?}, which earns {owed:?}"
            ),
        );
    }
}
