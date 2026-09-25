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
    /// A part of the standing is not the shape a run writes it in.
    #[error("its {part} is not the shape a run writes")]
    Unshaped {
        /// The part.
        part: &'static str,
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
        .ok_or(ContradictionError::Unshaped { part: "state" })?
        .to_owned();
    known(standing)?;
    let named = |list: &'static str, witnessable: &[&str]| named(standing, list, witnessable);
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
            let (count, reported) = named("because", &WITNESSED_BECAUSE)?;
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
            let (count, reported) = named("why", &WITNESSED_WHY)?;
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
        for reason in reasons(standing, list)? {
            let kind = reason
                .get("kind")
                .and_then(Value::as_str)
                .ok_or(ContradictionError::Unshaped { part: "reason" })?;
            if !known.contains(&kind) {
                return Err(ContradictionError::Reason {
                    kind: kind.to_owned(),
                });
            }
        }
    }
    Ok(())
}

/// The reasons `standing` gives in `list`: none where it has no such list, which is how a state that carries no reasons of that kind is written.
///
/// # Errors
/// [`ContradictionError::Unshaped`] where the list is there and is not a list.
fn reasons<'a>(standing: &'a Value, list: &'static str) -> Result<&'a [Value], ContradictionError> {
    match standing.get(list) {
        None => Ok(&[]),
        Some(Value::Array(reasons)) => Ok(reasons),
        Some(_) => Err(ContradictionError::Unshaped { part: list }),
    }
}

/// How many reasons `standing` gives in `list`, and which of them are `witnessable`.
///
/// # Errors
/// [`ContradictionError::Unshaped`] where the list, or a reason in it, is not the shape a run writes.
fn named(
    standing: &Value,
    list: &'static str,
    witnessable: &[&str],
) -> Result<(usize, BTreeSet<String>), ContradictionError> {
    let all: Vec<String> = reasons(standing, list)?
        .iter()
        .map(|reason| {
            reason
                .get("kind")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
                .ok_or(ContradictionError::Unshaped { part: "reason" })
        })
        .collect::<Result<_, _>>()?;
    let witnessed = all
        .iter()
        .filter(|kind| witnessable.contains(&kind.as_str()))
        .cloned()
        .collect();
    Ok((all.len(), witnessed))
}

/// How many rounds confirm a delayed failure, written again from the runner's contract rather than read from its code.
pub const CONFIRMING_ROUNDS: usize = 5;

/// One control the exploration of a binary started, in the order the engine recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The guard it paused its threads at, or nothing for an undelayed control.
    pub delayed: Option<u64>,
    /// The guard an undelayed control names itself the confirming half of a round for.
    pub confirms: Option<u64>,
    /// How it ended.
    pub ended: crate::knobs::Ended,
    /// The tests that failed.
    pub failed: BTreeSet<String>,
}

/// What the exploration of one binary comes to, replayed from its controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Explored {
    /// No control was started.
    Nothing,
    /// Every delayed control passed.
    Sampled {
        /// Every guard delayed, in order.
        delayed: Vec<u64>,
    },
    /// No site broke it and some settled nothing.
    Undecided {
        /// Every guard delayed, in order.
        delayed: Vec<u64>,
        /// Those that settled nothing.
        undecided: Vec<u64>,
    },
    /// Every confirming round held at one site.
    Broke {
        /// The guard.
        site: u64,
        /// The tests that failed.
        failed: BTreeSet<String>,
        /// How many rounds held.
        rounds: usize,
    },
}

/// Where a recorded sequence is not one the exploration procedure starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReplayError {
    /// A control the procedure would not have started there.
    #[error("control {at} is not one the exploration starts at that point")]
    Stray {
        /// Its place in the sequence.
        at: usize,
    },
    /// The sequence ends inside a confirming round.
    #[error("the recording ends inside a confirming round")]
    Truncated,
    /// A control after the schedule broke.
    #[error("control {at} was started after the schedule broke")]
    AfterBroke {
        /// Its place in the sequence.
        at: usize,
    },
}

/// What the exploration of one binary comes to, replaying the procedure over `runs`.
///
/// Each delayed site has its first control, and where that failed, rounds of a delayed control that must fail exactly the same tests and an undelayed one that must pass, stopping at the first that does not.
///
/// # Errors
/// [`ReplayError`] where the sequence is not one the procedure gives.
pub fn replayed(runs: &[Run]) -> Result<Explored, ReplayError> {
    let (mut delayed, mut undecided) = (Vec::new(), Vec::new());
    let mut rest = runs.iter().enumerate();
    while let Some((at, first)) = rest.next() {
        let site = first.delayed.ok_or(ReplayError::Stray { at })?;
        if first.confirms.is_some() {
            return Err(ReplayError::Stray { at });
        }
        delayed.push(site);
        match first.ended {
            crate::knobs::Ended::Passed => continue,
            crate::knobs::Ended::Waited | crate::knobs::Ended::Unsettled => {
                undecided.push(site);
                continue;
            }
            crate::knobs::Ended::Failed => {}
        }
        if confirmed(&mut rest, site, &first.failed)? {
            if let Some((at, _)) = rest.next() {
                return Err(ReplayError::AfterBroke { at });
            }
            return Ok(Explored::Broke {
                site,
                failed: first.failed.clone(),
                rounds: CONFIRMING_ROUNDS,
            });
        }
        undecided.push(site);
    }
    Ok(match (delayed.is_empty(), undecided.is_empty()) {
        (true, _) => Explored::Nothing,
        (false, true) => Explored::Sampled { delayed },
        (false, false) => Explored::Undecided { delayed, undecided },
    })
}

/// Whether every confirming round of `site` holds, consuming the controls the procedure started for them.
fn confirmed<'a>(
    rest: &mut impl Iterator<Item = (usize, &'a Run)>,
    site: u64,
    failed: &BTreeSet<String>,
) -> Result<bool, ReplayError> {
    for _ in 0..CONFIRMING_ROUNDS {
        let (at, again) = rest.next().ok_or(ReplayError::Truncated)?;
        if again.delayed != Some(site) {
            return Err(ReplayError::Stray { at });
        }
        if !(again.ended == crate::knobs::Ended::Failed && again.failed == *failed) {
            return Ok(false);
        }
        let (at, without) = rest.next().ok_or(ReplayError::Truncated)?;
        if without.delayed.is_some() || without.confirms != Some(site) {
            return Err(ReplayError::Stray { at });
        }
        if without.ended != crate::knobs::Ended::Passed {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What a reported exploration contradicts in the replayed one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExploreContradictionError {
    /// The report says something the replay does not come to.
    #[error("the report says {reported}, and the recorded controls come to {derived:?}")]
    Differs {
        /// What the report says.
        reported: String,
        /// What the controls come to.
        derived: Explored,
    },
}

/// What else a reported exploration answers to beyond its own controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    /// Whether the audited standing of the binary is single-threaded.
    pub single_threaded: bool,
    /// Whether its baseline passed.
    pub passing: bool,
    /// How many guards its baseline reached, or nothing where no baseline reach of it was recorded.
    pub reached: Option<usize>,
    /// Whether the run delayed any guard of any binary, which is what asking for schedules does.
    pub asked_any: bool,
}

/// Why a binary with no delayed control was not explored, as the recording decides it, or nothing where it cannot: a passing binary whose baseline reach was not recorded.
#[must_use]
pub const fn unexplored_because(context: Context) -> Option<&'static str> {
    if context.single_threaded {
        Some("not-needed")
    } else if !context.passing {
        Some("not-passing")
    } else {
        match context.reached {
            Some(0) => Some("no-site"),
            Some(_) => Some("not-asked"),
            None => None,
        }
    }
}

/// Whether the reported `explored` is exactly what the controls come to, for a binary `context` describes.
///
/// # Errors
/// [`ExploreContradictionError`] where it is not.
pub fn agrees_explored(
    explored: &Value,
    derived: &Explored,
    context: Context,
) -> Result<(), ExploreContradictionError> {
    let sites = |key: &str| -> Option<Vec<u64>> {
        explored
            .get(key)?
            .as_array()?
            .iter()
            .map(Value::as_u64)
            .collect()
    };
    let asked = explored.get("asked").and_then(Value::as_u64);
    let holds = match (explored.get("state").and_then(Value::as_str), derived) {
        (Some("unexplored"), Explored::Nothing) => unexplored_because(context).is_some_and(|why| {
            explored.get("why").and_then(Value::as_str) == Some(why)
                && !(why == "not-asked" && context.asked_any)
        }),
        (Some("sampled"), Explored::Sampled { delayed }) => {
            !context.single_threaded
                && sites("delayed").as_ref() == Some(delayed)
                && fits(asked, delayed, context.reached)
        }
        (Some("undecided"), Explored::Undecided { delayed, undecided }) => {
            !context.single_threaded
                && sites("delayed").as_ref() == Some(delayed)
                && sites("undecided").as_ref() == Some(undecided)
                && fits(asked, delayed, context.reached)
        }
        (
            Some("broke"),
            Explored::Broke {
                site,
                failed,
                rounds,
            },
        ) => {
            let named: Option<BTreeSet<String>> = explored
                .get("failed")
                .and_then(Value::as_array)
                .and_then(|tests| {
                    tests
                        .iter()
                        .map(|one| one.as_str().map(ToOwned::to_owned))
                        .collect()
                });
            !context.single_threaded
                && explored.get("site").and_then(Value::as_u64) == Some(*site)
                && named.as_ref() == Some(failed)
                && u64::try_from(*rounds).is_ok_and(|rounds| {
                    explored.get("rounds").and_then(Value::as_u64) == Some(rounds)
                })
        }
        (
            _,
            Explored::Nothing
            | Explored::Sampled { .. }
            | Explored::Undecided { .. }
            | Explored::Broke { .. },
        ) => false,
    };
    if holds {
        Ok(())
    } else {
        Err(ExploreContradictionError::Differs {
            reported: explored.to_string(),
            derived: derived.clone(),
        })
    }
}

/// Whether `delayed` is as many guards as were asked for, or every guard reached where fewer were; never where the baseline's reach was not recorded, which is what the count is held to.
fn fits(asked: Option<u64>, delayed: &[u64], reached: Option<usize>) -> bool {
    asked.zip(reached).is_some_and(|(asked, reached)| {
        usize::try_from(asked).is_ok_and(|asked| delayed.len() == asked.min(reached))
    })
}
