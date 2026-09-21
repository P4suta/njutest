// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest review`: the gaps of one run, one at a time, with somebody answering.

use std::io::{BufRead, Write};

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Review as Arguments};
use crate::presentation::review::{AboutAGap, AboutTheUnsettled, Reason, Reviewed};

/// What a reviewer types, and what it means.
///
/// One letter each, because a reviewer answers this once per gap and a run with thirty of them is thirty answers.
/// `a` asks for a reason afterwards rather than on the same line: a reason typed to keep a line short is the reason nobody reads later.
struct AtTheKeyboard<'a> {
    input: std::io::StdinLock<'static>,
    out: &'a mut dyn Write,
    failure: Option<std::io::Error>,
}

impl AtTheKeyboard<'_> {
    /// One line, or nothing where the reader has gone.
    fn line(&mut self) -> Option<String> {
        let mut said = String::new();
        match self.input.read_line(&mut said) {
            Ok(0) => None,
            Ok(_) => Some(said),
            Err(error) => {
                self.failure = Some(error);
                None
            }
        }
    }

    /// Records the first stream failure and tells the question loop to stop.
    fn wrote(&mut self, result: std::io::Result<()>) -> bool {
        match result {
            Ok(()) => true,
            Err(error) => {
                if self.failure.is_none() {
                    self.failure = Some(error);
                }
                false
            }
        }
    }

    /// Writes the prompt and waits, so a reader is not left with a cursor and no question.
    fn asking(&mut self, keys: &str) -> Option<String> {
        let written = write!(self.out, "  {keys} > ");
        if !self.wrote(written) {
            return None;
        }
        let flushed = self.out.flush();
        if !self.wrote(flushed) {
            return None;
        }
        self.line()
    }
}

impl crate::presentation::review::Answers for AtTheKeyboard<'_> {
    fn about_a_gap(
        &mut self,
        _spot: &crate::presentation::Spot,
        _blindness: crate::presentation::Blindness,
        drawn: &str,
    ) -> AboutAGap {
        let written = writeln!(self.out, "\n{drawn}");
        if !self.wrote(written) {
            return AboutAGap::Stop;
        }
        loop {
            let Some(said) = self.asking("[a]ccept  [l]eave  [s]top") else {
                return AboutAGap::Stop;
            };
            match said.trim() {
                "a" | "accept" => {
                    let Some(why) = self.asking("why may it stand?") else {
                        return AboutAGap::Stop;
                    };
                    if let Some(reason) = Reason::of(&why) {
                        return AboutAGap::Accept(reason);
                    }
                    let written = writeln!(
                        self.out,
                        "  an acceptance with no reason is a mutation nobody looked at, \
                         recorded as one somebody did"
                    );
                    if !self.wrote(written) {
                        return AboutAGap::Stop;
                    }
                }
                "l" | "leave" | "" => return AboutAGap::Leave,
                "s" | "stop" | "q" | "quit" => return AboutAGap::Stop,
                _ => {}
            }
        }
    }

    fn about_the_unsettled(
        &mut self,
        _spot: &crate::presentation::Spot,
        _unsettled: crate::presentation::Unsettled,
        drawn: &str,
    ) -> AboutTheUnsettled {
        let written = writeln!(
            self.out,
            "\n{drawn}\n   the run established nothing here, so there is nothing to accept"
        );
        if !self.wrote(written) {
            return AboutTheUnsettled::Stop;
        }
        loop {
            let Some(said) = self.asking("[l]eave  [s]top") else {
                return AboutTheUnsettled::Stop;
            };
            match said.trim() {
                "l" | "leave" | "" => return AboutTheUnsettled::Leave,
                "s" | "stop" | "q" | "quit" => return AboutTheUnsettled::Stop,
                "a" | "accept" => {
                    let written = writeln!(
                        self.out,
                        "  the run established nothing here, so there is nothing to \
                         accept: find out why before deciding it may stand"
                    );
                    if !self.wrote(written) {
                        return AboutTheUnsettled::Stop;
                    }
                }
                _ => {}
            }
        }
    }
}

/// Shows one run's gaps one at a time and records what a reviewer decided.
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
            super::diagnose(stderr, &error.to_string())?;
            return Ok(EXIT_ERROR);
        }
    };
    let kept = run.said_document().to_owned();
    let sources = match crate::presentation::Sources::read(root, &report) {
        Ok(sources) => sources,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };
    let told = match crate::presentation::Told::of(&report, &sources, &kept) {
        Ok(told) => told,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };

    let mut asking = AtTheKeyboard {
        input: std::io::stdin().lock(),
        out: stdout,
        failure: None,
    };
    let decided = crate::presentation::review::review(&told, environment.terminal, &mut asking);
    let interaction_failure = asking.failure.take();
    drop(asking);
    if let Some(error) = interaction_failure {
        super::diagnose(stderr, &error.to_string())?;
        return Ok(EXIT_ERROR);
    }
    said(stdout, &decided)?;
    Ok(EXIT_ASSURED)
}

/// What the review came to, and the commands that record it.
///
/// The decisions are printed rather than written, because a review is a person deciding and a file is a change to their project: `njutest accept` is the command that makes it, it is already the one every other surface prints,
/// and a reviewer who wants them all runs the lines they were given.
fn said(out: &mut dyn Write, reviewed: &Reviewed) -> std::io::Result<()> {
    if reviewed.accepted.is_empty() {
        writeln!(out, "\nnothing accepted; {} left", reviewed.left)?;
    } else {
        writeln!(out, "\nrun these to record what you decided:")?;
        for (named, reason) in &reviewed.accepted {
            writeln!(out, "  njutest accept {named} --reason {:?}", reason.said())?;
        }
    }
    if let Some(stopped) = &reviewed.stopped_at {
        writeln!(out, "stopped at {stopped}")?;
    }
    Ok(())
}
