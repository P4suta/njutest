// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What interpreting the suite established, re-derived from the recorded run of the interpreter and what it said, and held to what the report says of soundness.

use std::collections::BTreeSet;

use serde_json::Value;

use super::{Audit, Decided, Layer, Notes, Recording, field, rows};

/// What Miri's diagnostic says next when it has found unsoundness.
pub const UNDEFINED: &str = "Undefined Behavior:";

/// What starts a line in which the interpreter or the toolchain speaks.
pub const DIAGNOSTIC: &str = "error: ";

/// What starts the line cargo prints before each test binary.
pub const BINARY: [&str; 2] = ["Running ", "Doc-tests "];

/// What a test's captured output opens with, and the two endings its header can have.
pub const CAPTURE_OPEN: (&str, [&str; 2]) = ("---- ", [" stdout ----", " stderr ----"]);

/// The line that closes every failing test's captured output.
pub const CAPTURE_CLOSE: &str = "failures:";

/// What libtest prints around a test's name before its outcome, which a diagnostic may follow on the same line.
pub const TEST_PREFIX: (&str, &str) = ("test ", " ... ");

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

/// What the recording kept of one run's output, held to the size and digest its exec record gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kept {
    /// The copy is the output the record describes, and it is text.
    Whole(String),
    /// The copy is not the output the record describes: its size or its digest differs.
    Mismatched,
    /// The copy is the output the record describes, and it is not text.
    NotText,
    /// There is no copy the audit may read: none was kept, it is not where the runner keeps one, it was cut, or it could not be read whole and in bounds.
    Unread {
        /// Which, in words.
        why: String,
    },
}

/// One recorded run of a program, and what it said where the recording kept it.
#[derive(Debug, Clone, Copy)]
struct Said<'a> {
    /// The run's exec record.
    exec: &'a Value,
    /// What it printed, where the recording kept a copy the audit could read.
    output: Option<&'a Kept>,
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

    /// The names of the limitations about the interpreter it earns, which is how a run states what it could not interpret.
    fn limitations(self) -> BTreeSet<&'static str> {
        match self {
            Self::Absent => BTreeSet::from(["miri-unavailable"]),
            Self::TimedOut => BTreeSet::from(["miri-timed-out"]),
            Self::Unsupported => BTreeSet::from(["miri-unsupported"]),
            Self::RanNoTest => BTreeSet::from(["miri-ran-no-test"]),
            Self::Undefined | Self::Failed | Self::Passed => BTreeSet::new(),
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

/// What the interpretation `run` came to, with the toolchain's answer `probe` where it was asked, or nothing where what either said was not kept as text.
fn came(run: Said<'_>, probe: Option<Said<'_>>) -> Option<Came> {
    let Some(Kept::Whole(said)) = run.output else {
        return None;
    };
    let (kind, code) = ended(run.exec);
    let heard = Heard::of(said);
    if kind == "not-started" || heard.absent {
        return Some(Came::Absent);
    }
    if kind == "timed-out" || kind == "stalled" {
        return Some(Came::TimedOut);
    }
    let asked = match probe {
        Some(asked) if code != Some(0) => probed(asked),
        Some(_) | None => Probed::Unsaid,
    };
    match asked {
        Probed::Absent => Some(Came::Absent),
        Probed::Unread => None,
        Probed::Unsaid => Some(heard.came(code)),
    }
}

/// What the toolchain's answer to the version question says about its interpreter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Probed {
    /// Nothing was launched, or it exited with a code and said the interpreter is not installed.
    Absent,
    /// It did not say the interpreter is absent: it answered, or it ran out of time or ended by a signal and so nobody answered.
    Unsaid,
    /// What it said was not kept as text, so it cannot be read either way.
    Unread,
}

/// What the toolchain's answer `asked` says about its interpreter.
fn probed(asked: Said<'_>) -> Probed {
    let (kind, code) = ended(asked.exec);
    if kind == "not-started" {
        return Probed::Absent;
    }
    let said_absent = |said: &str| {
        if Heard::of(said).absent {
            Probed::Absent
        } else {
            Probed::Unsaid
        }
    };
    match code {
        Some(0) | None => Probed::Unsaid,
        Some(_) => match asked.output {
            Some(Kept::Whole(said)) => said_absent(said),
            Some(Kept::Mismatched | Kept::NotText | Kept::Unread { .. }) => Probed::Unread,
            None => Probed::Unsaid,
        },
    }
}

/// What the interpreter's output `said` comes to where its run exited with `code`, read by the structure the published contract gives it, as the contract names the verdict.
#[cfg(feature = "testkit")]
#[must_use]
pub fn verdict(said: &str, code: Option<i64>) -> &'static str {
    let heard = Heard::of(said);
    if heard.absent {
        return "absent";
    }
    match heard.came(code) {
        Came::Absent => "absent",
        Came::TimedOut => "timed-out",
        Came::Undefined => "undefined",
        Came::Unsupported => "unsupported",
        Came::Failed => "failed",
        Came::Passed => "passed",
        Came::RanNoTest => "ran-no-test",
    }
}

/// How one binary the interpreter started came out, where it gave a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    /// Every test of it passed.
    Passed,
    /// A test of it failed.
    Failed,
}

/// Where one line of the output stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    /// Outside any test's captured output.
    Open,
    /// Inside a failing test's captured output, which says nothing about the run.
    Captured,
}

/// What the interpreter and the toolchain said in one output, and what each binary came to.
#[derive(Debug, Default)]
struct Heard {
    /// How many binaries were started.
    started: usize,
    /// How each result given said a binary came out, in whatever order cargo's and libtest's streams interleaved.
    outcomes: Vec<Outcome>,
    /// Whether a diagnostic found undefined behaviour.
    undefined: bool,
    /// Whether a diagnostic could not interpret something.
    unsupported: bool,
    /// Whether a diagnostic said there is nothing to interpret with.
    absent: bool,
}

impl Heard {
    /// `said`, read line by line.
    fn of(said: &str) -> Self {
        let mut heard = Self::default();
        let mut place = Place::Open;
        let lines: Vec<&str> = said.lines().map(str::trim_end).collect();
        for (at, line) in lines.iter().copied().enumerate() {
            let (open, endings) = CAPTURE_OPEN;
            if line.starts_with(open) && endings.iter().any(|end| line.ends_with(end)) {
                place = Place::Captured;
                continue;
            }
            if place == Place::Captured {
                if libtest_closes(&lines, at) {
                    place = Place::Open;
                }
                continue;
            }
            heard.line(line.trim_start());
        }
        heard
    }

    /// One line outside any captured output.
    fn line(&mut self, line: &str) {
        if BINARY.iter().any(|start| line.starts_with(start)) {
            self.started = self.started.saturating_add(1);
            return;
        }
        if let Some(rest) = line.strip_prefix(RESULT) {
            if let Some(outcome) = summarised(rest) {
                self.outcomes.push(outcome);
            }
            return;
        }
        let (start, end) = TEST_PREFIX;
        let spoken = match line
            .strip_prefix(start)
            .and_then(|rest| rest.split_once(end))
        {
            Some((_name, after)) => after,
            None => line,
        };
        let Some(diagnostic) = spoken.strip_prefix(DIAGNOSTIC) else {
            return;
        };
        self.undefined |= diagnostic.starts_with(UNDEFINED);
        self.unsupported |= UNSUPPORTED.iter().any(|marker| diagnostic.contains(marker));
        self.absent |= ABSENT.iter().any(|marker| diagnostic.contains(marker));
    }

    /// What the run came to where it exited with `code`.
    fn came(&self, code: Option<i64>) -> Came {
        if self.undefined {
            return Came::Undefined;
        }
        if self.unsupported {
            return Came::Unsupported;
        }
        let failed = self.outcomes.contains(&Outcome::Failed);
        let every_passed = self.started > 0
            && self.outcomes.len() == self.started
            && self
                .outcomes
                .iter()
                .all(|outcome| *outcome == Outcome::Passed);
        match code {
            Some(0) if every_passed => Came::Passed,
            Some(code) if code != 0 && failed => Came::Failed,
            Some(_) | None => Came::RanNoTest,
        }
    }
}

/// Whether libtest, and not a test's own output, wrote the `failures:` at `at`: it lists the failed names indented, leaves a blank line, and gives the binary's summary.
fn libtest_closes(lines: &[&str], at: usize) -> bool {
    if lines.get(at).copied() != Some(CAPTURE_CLOSE) {
        return false;
    }
    let mut next = at.saturating_add(1);
    let mut names = 0_usize;
    while let Some(line) = lines.get(next)
        && line.starts_with("    ")
        && !line.trim().is_empty()
    {
        names = names.saturating_add(1);
        next = next.saturating_add(1);
    }
    names > 0
        && lines.get(next).is_some_and(|line| line.is_empty())
        && lines
            .get(next.saturating_add(1))
            .and_then(|line| line.trim_start().strip_prefix(RESULT))
            .and_then(summarised)
            .is_some()
}

/// How a binary came out where `rest` is exactly libtest's summary after [`RESULT`], and nothing otherwise.
fn summarised(rest: &str) -> Option<Outcome> {
    let (status, counts) = rest.split_once(". ")?;
    let outcome = if status == "ok" {
        Outcome::Passed
    } else if status == FAILED {
        Outcome::Failed
    } else {
        return None;
    };
    let mut parts = counts.split("; ");
    for what in [
        " passed",
        " failed",
        " ignored",
        " measured",
        " filtered out",
    ] {
        let number = parts.next()?.strip_suffix(what)?;
        if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
    }
    let time = parts
        .next()?
        .strip_prefix("finished in ")?
        .strip_suffix('s')?;
    let digits = time.bytes().filter(u8::is_ascii_digit).count();
    let points = time.bytes().filter(|byte| *byte == b'.').count();
    let only_digits_and_a_point = time
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.');
    (parts.next().is_none() && digits > 0 && only_digits_and_a_point && points <= 1)
        .then_some(outcome)
}

/// Holds what the report says of soundness to what the recorded interpretation came to, in both directions; `execs` is every exec record of the runner's recording and `outputs` what the recording kept of each run it re-derives from.
pub(super) fn audited(
    recording: &Recording<'_>,
    execs: Option<&[Value]>,
    outputs: &[(String, Kept)],
    audit: &mut Audit,
) -> Decided {
    let mut notes = Notes::on(audit, Layer::Soundness);
    let reported = Reported::of(recording);
    let Some(execs) = execs else {
        if reported.executed != Interpreted::Said && reported.claimed.is_empty() {
            return notes
                .absent("the run kept no recording and its report says nothing was interpreted");
        }
        notes.unaudited(
            "soundness",
            "the run kept no recording, so what the interpreter came to cannot be re-derived"
                .to_owned(),
        );
        return notes.looked();
    };
    let said = |exec: &Value| -> Kept {
        let Some(named) = field(exec, "output_path") else {
            return if exec.get("output_bytes").and_then(Value::as_u64) == Some(0) {
                Kept::Whole(String::new())
            } else {
                Kept::Unread {
                    why: "the recording kept no copy of what it printed".to_owned(),
                }
            };
        };
        match outputs.iter().find(|(path, _kept)| *path == named) {
            Some((_path, kept)) => kept.clone(),
            None => Kept::Unread {
                why: format!("the recording kept no copy at {named}"),
            },
        }
    };
    let runs: Vec<&Value> = execs.iter().filter(|exec| interprets(exec)).collect();
    let probe = execs.iter().find(|exec| asks_for_the_interpreter(exec));
    match runs.as_slice() {
        [] => never_ran(&reported, &mut notes),
        [one] => {
            let output = said(one);
            if output == Kept::Mismatched {
                notes.violated(
                    "soundness",
                    "the output the recording kept of the interpreter's run is not the output its \
                     exec record gives the size and digest of"
                        .to_owned(),
                );
                return notes.looked();
            }
            let answered = probe.map(|asked| (asked, said(asked)));
            let asked = answered.as_ref().map(|(asked, output)| Said {
                exec: asked,
                output: Some(output),
            });
            match came(
                Said {
                    exec: one,
                    output: Some(&output),
                },
                asked,
            ) {
                Some(derived) => compared(&reported, derived, &mut notes),
                None => notes.unaudited(
                    "soundness",
                    unread(&output, answered.as_ref().map(|(_asked, output)| output)),
                ),
            }
        }
        several => notes.violated(
            "soundness",
            format!(
                "the recording holds {} runs of the interpreter for one build, which interprets \
                 its suite once, so the report rests on more than one answer",
                several.len()
            ),
        ),
    }
    notes.looked()
}

/// Why what the interpretation said, `run`, or the toolchain's answer to it, `probe`, could not be read to re-derive what it came to.
fn unread(run: &Kept, probe: Option<&Kept>) -> String {
    let (whose, kept) = match run {
        Kept::Whole(_) => ("the toolchain's answer about its interpreter", probe),
        Kept::Mismatched | Kept::NotText | Kept::Unread { .. } => {
            ("what the interpreter said", Some(run))
        }
    };
    let why = match kept {
        Some(Kept::Unread { why }) => why.clone(),
        Some(Kept::NotText) => "it is not text".to_owned(),
        Some(Kept::Mismatched) => "it is not the output its exec record describes".to_owned(),
        Some(Kept::Whole(_)) | None => "it was not kept whole".to_owned(),
    };
    format!(
        "{whose} was not read, because {why}, so what the interpreter came to cannot be re-derived"
    )
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
    /// The names of its limitations about the interpreter.
    stated: BTreeSet<String>,
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
            stated: rows(recording.document, "limitations")
                .iter()
                .filter_map(|limitation| field(limitation, "name"))
                .filter(|name| name.starts_with("miri-"))
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
    let owed_limits: BTreeSet<String> = derived
        .limitations()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if reported.stated != owed_limits {
        notes.violated(
            "soundness",
            format!(
                "the report's limitations about the interpreter are {:?}, and what it said comes \
                 to {derived:?}, which states {owed_limits:?}",
                reported.stated
            ),
        );
    }
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

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::{Came, Kept, Said, came};

    /// What an interpreter run printed where one of its tests failed.
    const FAILED_RUN: &str = "     Running unittests src/lib.rs (x)\n\nrunning 1 test\ntest t ... FAILED\n\nfailures:\n\n---- t stdout ----\nboom\n\nfailures:\n    t\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n";

    /// An exec record of `argv` that ended as `stopped`, with its output kept at `output/1.txt` or kept nowhere.
    fn exec(argv: &[&str], stopped: &Value, kept: bool) -> Value {
        json!({
            "argv": argv,
            "stopped": stopped,
            "output_path": if kept { json!("output/1.txt") } else { Value::Null },
        })
    }

    /// How a process that exited with `code` ended, as a recording spells it.
    fn exited(code: i64) -> Value {
        json!({ "kind": "exited", "exit": { "kind": "code", "value": code } })
    }

    #[test]
    fn a_question_about_the_interpreter_nobody_answered_is_not_an_interpreter_that_is_absent() {
        let run = exec(&["cargo", "+nightly", "miri", "test"], &exited(101), true);
        let output = Kept::Whole(FAILED_RUN.to_owned());
        let failed = Said {
            exec: &run,
            output: Some(&output),
        };
        let absent = Kept::Whole("error: no such command: `miri`\n".to_owned());
        let other = Kept::Whole("error: failed to download\n".to_owned());
        for (stopped, said, came_to, why) in [
            (
                json!({ "kind": "exited", "exit": { "kind": "signal", "value": 9 } }),
                None,
                Some(Came::Failed),
                "a probe ended by a signal answered nothing",
            ),
            (
                json!({ "kind": "timed-out", "raised": null }),
                None,
                Some(Came::Failed),
                "a probe that ran out of time answered nothing",
            ),
            (
                exited(101),
                Some(&other),
                Some(Came::Failed),
                "a probe that failed without saying the interpreter is missing did not say so",
            ),
            (
                exited(101),
                Some(&absent),
                Some(Came::Absent),
                "a probe that says there is no such command says the interpreter is absent",
            ),
            (
                json!({ "kind": "not-started", "cause": { "kind": "missing" } }),
                None,
                Some(Came::Absent),
                "a probe nothing launched is a toolchain that could not be run",
            ),
            (
                exited(101),
                Some(&Kept::Mismatched),
                None,
                "a probe whose kept answer is not the one it gave cannot be read either way",
            ),
        ] {
            let asked = exec(
                &["cargo", "+nightly", "miri", "--version"],
                &stopped,
                said.is_some(),
            );
            assert_eq!(
                came(
                    failed,
                    Some(Said {
                        exec: &asked,
                        output: said,
                    })
                ),
                came_to,
                "{why}"
            );
        }
    }
}
