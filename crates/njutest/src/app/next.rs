// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest next`: the cheapest gap in the tests, one at a time, and the checked test that closes it written only once it holds up again.

use std::io::{BufRead, Write};

use crate::app::runs;
use crate::build::Cargo;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, EXIT_INSUFFICIENT, Environment, Next as Arguments};
use crate::next::{Deciding, Offer, OnGap, OnOffer};
use crate::trace::Recorder;
use crate::watch::Watch;

/// Somebody at the keyboard, answering one letter per gap.
struct AtTheKeyboard<'a> {
    input: std::io::StdinLock<'static>,
    out: &'a mut dyn Write,
    failure: Option<std::io::Error>,
}

impl AtTheKeyboard<'_> {
    /// Shows `said`, asks with `keys`, and returns the trimmed answer, or nothing where the reader has gone or a stream failed.
    fn asking(&mut self, said: &str, keys: &str) -> Option<String> {
        let shown = writeln!(self.out, "\n{said}")
            .and_then(|()| write!(self.out, "  {keys} > "))
            .and_then(|()| self.out.flush());
        if let Err(error) = shown {
            self.failure = Some(error);
            return None;
        }
        let mut answer = String::new();
        match self.input.read_line(&mut answer) {
            Ok(0) => None,
            Ok(_) => Some(answer.trim().to_owned()),
            Err(error) => {
                self.failure = Some(error);
                None
            }
        }
    }
}

impl Deciding for AtTheKeyboard<'_> {
    fn about_an_offer(&mut self, said: &str) -> OnOffer {
        loop {
            let Some(answer) = self.asking(said, "t take  n next  s stop") else {
                return OnOffer::Stop;
            };
            match answer.as_str() {
                "t" | "take" | "y" | "yes" => return OnOffer::Take,
                "n" | "next" | "" => return OnOffer::Next,
                "s" | "stop" | "q" | "quit" => return OnOffer::Stop,
                _ => {}
            }
        }
    }

    fn about_a_gap(&mut self, said: &str) -> OnGap {
        loop {
            let Some(answer) = self.asking(said, "n next  s stop") else {
                return OnGap::Stop;
            };
            match answer.as_str() {
                "n" | "next" | "" => return OnGap::Next,
                "s" | "stop" | "q" | "quit" => return OnGap::Stop,
                _ => {}
            }
        }
    }
}

/// Whoever asked for the cheapest checked test with `--take`: it takes the first offer and asks nothing after it.
struct TakingTheFirst<'a> {
    out: &'a mut dyn Write,
    failure: Option<std::io::Error>,
    answered: bool,
}

impl TakingTheFirst<'_> {
    /// Shows `said`, keeping a failure to write it.
    fn show(&mut self, said: &str) {
        if let Err(error) = writeln!(self.out, "{said}") {
            self.failure = Some(error);
        }
    }
}

impl Deciding for TakingTheFirst<'_> {
    fn about_an_offer(&mut self, said: &str) -> OnOffer {
        if std::mem::replace(&mut self.answered, true) {
            return OnOffer::Stop;
        }
        self.show(said);
        if self.failure.is_none() {
            OnOffer::Take
        } else {
            OnOffer::Stop
        }
    }

    fn about_a_gap(&mut self, said: &str) -> OnGap {
        if !std::mem::replace(&mut self.answered, true) {
            self.show(said);
        }
        OnGap::Stop
    }
}

/// Offers one run's gaps one at a time and writes each checked test somebody takes, once it holds up again against the tree as it is.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let report = match runs::report(&run) {
        Ok(report) => report,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let conclusion = match report.conclusion() {
        Ok(conclusion) => conclusion,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };
    let sources = match crate::presentation::Sources::read(root, &report) {
        Ok(sources) => sources,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };
    let told = match crate::presentation::Told::of(&report, &sources, run.said_document()) {
        Ok(told) => told,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };
    let gaps = crate::next::gaps(&told, &conclusion.candidates);
    if gaps.is_empty() {
        super::say(
            stdout,
            &format!("{} leaves no gap a test can close", run.id()),
        )?;
        return Ok(EXIT_ASSURED);
    }
    let (walked, failure) = decided(&gaps, arguments.take, stdout);
    if let Some(error) = failure {
        super::diagnose(stderr, &error.to_string())?;
        return Ok(EXIT_ERROR);
    }
    let refused = written(
        &walked.taken,
        (arguments, environment, run.config()),
        stdout,
        stderr,
    )?;
    super::say(
        stdout,
        &format!(
            "{}{} taken, {refused} not; {} {} still open",
            if walked.stopped {
                "stopped before the last gap; "
            } else {
                ""
            },
            walked.taken.len().saturating_sub(refused),
            walked.open,
            if walked.open == 1 {
                "mutation is"
            } else {
                "mutations are"
            },
        ),
    )?;
    Ok(if refused > 0 {
        EXIT_INSUFFICIENT
    } else {
        EXIT_ASSURED
    })
}

/// Asks about `gaps` at the keyboard, or takes the first with `take`; what was decided, and the first stream failure.
fn decided<'a>(
    gaps: &[crate::next::Gap<'a>],
    take: bool,
    stdout: &mut dyn Write,
) -> (crate::next::Walked<'a>, Option<std::io::Error>) {
    if take {
        let mut taking = TakingTheFirst {
            out: stdout,
            failure: None,
            answered: false,
        };
        let walked = crate::next::walk(gaps, &mut taking);
        (walked, taking.failure)
    } else {
        let mut asking = AtTheKeyboard {
            input: std::io::stdin().lock(),
            out: stdout,
            failure: None,
        };
        let walked = crate::next::walk(gaps, &mut asking);
        (walked, asking.failure)
    }
}

/// Checks and writes every taken offer, saying what became of each; how many could not be written.
fn written(
    taken: &[Offer<'_>],
    (arguments, environment, config): (&Arguments, &Environment, &crate::config::Config),
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<usize> {
    let trace = Recorder::disabled();
    let watch = Watch::new(&environment.cancel, &trace);
    let checking = super::fix::checking(
        (&environment.working_directory, environment),
        config,
        Cargo {
            offline: arguments.offline,
            locked: arguments.locked,
        },
    );
    let mut refused = 0_usize;
    for offer in taken {
        match one(offer, &checking, watch) {
            Ok(said) => super::say(stdout, &said)?,
            Err(why) => {
                refused = refused.saturating_add(1);
                super::diagnose(stderr, &why.to_string())?;
            }
        }
    }
    Ok(refused)
}

/// Checks one taken offer again against every mutation it was recorded as closing, and writes it only when every one still holds.
fn one(
    offer: &Offer<'_>,
    checking: &crate::assure::repair::Checking<'_>,
    watch: Watch<'_>,
) -> Result<String, super::fix::CandidateError> {
    let mut proposal = None;
    for record in &offer.closes {
        match super::fix::one(record, checking, watch)? {
            super::fix::Taken::Already(path) => {
                return Ok(format!("{path} is already what the test would write"));
            }
            super::fix::Taken::Written(checked) => proposal = Some(checked),
        }
    }
    let Some(proposal) = proposal else {
        return Ok(format!(
            "{} closes nothing that is still open",
            offer.path()
        ));
    };
    super::fix::write(checking.root, &proposal)?;
    Ok(format!(
        "wrote {}, which closes {} {}",
        proposal.path,
        offer.closes.len(),
        if offer.closes.len() == 1 {
            "mutation"
        } else {
            "mutations"
        }
    ))
}
