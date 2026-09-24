// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether one test binary runs one thread, re-derived from what an engine recording witnesses and held to the reported standing exactly (ADR 0034).

use std::collections::BTreeSet;

use serde_json::Value;

/// What an engine recording witnesses about one binary.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Witnessed {
    /// How many sites its baseline reached off its tests' threads, or nothing where no baseline reach was recorded.
    pub loose: Option<u64>,
    /// Its kind and whether libtest runs it, as the build record says, or nothing where the build named no such target.
    pub kind: Option<(String, bool)>,
    /// The harness arguments its baseline ran with, or nothing where the baseline record carried none.
    pub args: Option<Vec<String>>,
}

/// The reasons a recording witnesses, by the names a report spells them with.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Derived {
    /// Why it can run more than one thread.
    pub because: BTreeSet<&'static str>,
    /// Why nothing is proven.
    pub why: BTreeSet<&'static str>,
}

/// The reasons for concurrency a recording can witness; the rest come from a scan this audit does not repeat.
pub const WITNESSED_BECAUSE: [&str; 2] = ["loose-reach", "parallel-tests"];

/// The reasons nothing is proven that a recording can witness.
pub const WITNESSED_WHY: [&str; 3] = ["no-touch", "not-libtest", "doctest"];

/// Every reason for concurrency a report can give.
pub const BECAUSE: [&str; 3] = ["loose-reach", "parallel-tests", "starts"];

/// Every reason nothing is proven that a report can give.
pub const WHY: [&str; 5] = [
    "no-touch",
    "not-libtest",
    "doctest",
    "unread",
    "native-code",
];

/// What a recording says too little about for a standing to be derived.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UnwitnessedError {
    /// The build named no such target.
    #[error("the build record names no such target")]
    TargetUnbuilt,
    /// Its baseline record carried no harness arguments, so how many threads libtest ran it on is not known.
    #[error("its baseline record carries no harness arguments")]
    ArgumentsUnrecorded,
}

/// What a reported standing contradicts.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContradictionError {
    /// The report proves it single-threaded where the recording witnesses a reason it is not.
    #[error("it is reported single-threaded, and the recording witnesses {because:?} and {why:?}")]
    Proven {
        /// The reasons for concurrency.
        because: BTreeSet<&'static str>,
        /// The reasons nothing is proven.
        why: BTreeSet<&'static str>,
    },
    /// The witnessed reasons the report names are not the ones the recording witnesses.
    #[error("it is reported {state} for {reported:?}, and the recording witnesses {witnessed:?}")]
    Reasons {
        /// The reported state.
        state: String,
        /// The witnessed reasons the report names.
        reported: BTreeSet<String>,
        /// The ones the recording witnesses.
        witnessed: BTreeSet<&'static str>,
    },
    /// The report says nothing is proven where the recording witnesses concurrency.
    #[error("it is reported not proven, and the recording witnesses {because:?}")]
    Concurrent {
        /// The reasons for concurrency.
        because: BTreeSet<&'static str>,
    },
    /// A proven binary that names a reason it is not.
    #[error("it is reported single-threaded, and it names reasons in `{list}`")]
    ProvenWithReasons {
        /// The list that should not be there.
        list: &'static str,
    },
    /// A reason no run gives.
    #[error("{kind:?} is no reason a run gives")]
    Reason {
        /// The kind.
        kind: String,
    },
    /// A state no run gives, or one that names no reason.
    #[error("{state:?} with no reason is no standing a run gives")]
    Unknown {
        /// The state.
        state: String,
    },
}

/// The libtest options that take the next word as their value.
const LIBTEST_VALUED: [&str; 7] = [
    "--test-threads",
    "--skip",
    "--logfile",
    "--format",
    "--color",
    "-Z",
    "--shuffle-seed",
];

/// The libtest options that take no value.
const LIBTEST_FLAGS: [&str; 17] = [
    "--include-ignored",
    "--ignored",
    "--force-run-in-process",
    "--exclude-should-panic",
    "--test",
    "--bench",
    "--list",
    "--nocapture",
    "--no-capture",
    "--show-output",
    "--exact",
    "-q",
    "--quiet",
    "--shuffle",
    "--report-time",
    "--ensure-time",
    "--fail-fast",
];

/// Whether libtest runs tests on one thread under `args`, read the way libtest reads them and written again here rather than taken from the runner.
#[must_use]
pub fn one_thread(args: &[String]) -> bool {
    const MANY: bool = false;
    let mut named = Vec::new();
    let mut words = args.iter();
    while let Some(word) = words.next() {
        if word == "--" {
            break;
        }
        if let Some((flag, value)) = word.split_once('=')
            && flag.starts_with("--")
        {
            if flag == "--test-threads" {
                named.push(Some(value));
            } else if !LIBTEST_VALUED.contains(&flag) {
                return MANY;
            }
        } else if LIBTEST_VALUED.contains(&word.as_str()) {
            let value = words.next().map(String::as_str);
            if word == "--test-threads" {
                named.push(value);
            }
        } else if word.starts_with('-') && !LIBTEST_FLAGS.contains(&word.as_str()) {
            return MANY;
        }
    }
    named.as_slice() == [Some("1")]
}

/// The reasons `witnessed` establishes.
///
/// # Errors
/// [`UnwitnessedError`] where the recording says too little.
pub fn derived(witnessed: &Witnessed) -> Result<Derived, UnwitnessedError> {
    let mut derived = Derived::default();
    match witnessed.loose {
        None => {
            derived.why.insert("no-touch");
        }
        Some(0) => {}
        Some(_) => {
            derived.because.insert("loose-reach");
        }
    }
    let (kind, harness) = witnessed
        .kind
        .as_ref()
        .ok_or(UnwitnessedError::TargetUnbuilt)?;
    if kind == "doc" {
        derived.why.insert("doctest");
    } else if !harness {
        derived.why.insert("not-libtest");
    } else if !one_thread(
        witnessed
            .args
            .as_deref()
            .ok_or(UnwitnessedError::ArgumentsUnrecorded)?,
    ) {
        derived.because.insert("parallel-tests");
    }
    Ok(derived)
}

/// Whether the reported `standing` names exactly the witnessed reasons `derived` holds, and is the state they come to.
///
/// # Errors
/// The [`ContradictionError`] it holds.
pub fn agrees(standing: &Value, derived: &Derived) -> Result<(), ContradictionError> {
    let state = standing
        .get("state")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    known(standing)?;
    let named = |list: &str, witnessable: &[&str]| named(standing, list, witnessable);
    let same = |reported: &BTreeSet<String>, witnessed: &BTreeSet<&'static str>| {
        reported.len() == witnessed.len() && witnessed.iter().all(|one| reported.contains(*one))
    };
    match state.as_str() {
        "single-threaded" => {
            if let Some(list) = ["because", "why"]
                .into_iter()
                .find(|list| standing.get(*list).is_some())
            {
                Err(ContradictionError::ProvenWithReasons { list })
            } else if derived.because.is_empty() && derived.why.is_empty() {
                Ok(())
            } else {
                Err(ContradictionError::Proven {
                    because: derived.because.clone(),
                    why: derived.why.clone(),
                })
            }
        }
        "concurrent" => {
            let (count, reported) = named("because", &WITNESSED_BECAUSE);
            if count == 0 {
                Err(ContradictionError::Unknown { state })
            } else if same(&reported, &derived.because) {
                Ok(())
            } else {
                Err(ContradictionError::Reasons {
                    state,
                    reported,
                    witnessed: derived.because.clone(),
                })
            }
        }
        "not-proven" => {
            let (count, reported) = named("why", &WITNESSED_WHY);
            if !derived.because.is_empty() {
                Err(ContradictionError::Concurrent {
                    because: derived.because.clone(),
                })
            } else if count == 0 {
                Err(ContradictionError::Unknown { state })
            } else if same(&reported, &derived.why) {
                Ok(())
            } else {
                Err(ContradictionError::Reasons {
                    state,
                    reported,
                    witnessed: derived.why.clone(),
                })
            }
        }
        _ => Err(ContradictionError::Unknown { state }),
    }
}

/// Whether every reason `standing` gives is one a run gives.
fn known(standing: &Value) -> Result<(), ContradictionError> {
    for (list, known) in [("because", &BECAUSE[..]), ("why", &WHY[..])] {
        for reason in standing
            .get(list)
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default()
        {
            let kind = reason
                .get("kind")
                .and_then(Value::as_str)
                .unwrap_or_default();
            if !known.contains(&kind) {
                return Err(ContradictionError::Reason {
                    kind: kind.to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// How many reasons `standing` gives in `list`, and which of them are `witnessable`.
fn named(standing: &Value, list: &str, witnessable: &[&str]) -> (usize, BTreeSet<String>) {
    let all: Vec<String> = standing
        .get(list)
        .and_then(Value::as_array)
        .map(|reasons| {
            reasons
                .iter()
                .filter_map(|reason| reason.get("kind").and_then(Value::as_str))
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let witnessed = all
        .iter()
        .filter(|kind| witnessable.contains(&kind.as_str()))
        .cloned()
        .collect();
    (all.len(), witnessed)
}
