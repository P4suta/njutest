// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest review`: the gaps of one run, one at a time, with somebody answering.

use std::io::{BufRead, Write};

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Review as Arguments};
use crate::presentation::review::{AboutAGap, AboutTheUnsettled, Reason, Reviewed};

/// What a reviewer types, and what it means.
///
/// One letter each, because a reviewer answers this once per gap and a run
/// with thirty of them is thirty answers. `a` asks for a reason afterwards
/// rather than on the same line: a reason typed to keep a line short is the
/// reason nobody reads later.
struct AtTheKeyboard<'a> {
    input: std::io::StdinLock<'static>,
    out: &'a mut dyn Write,
}

impl AtTheKeyboard<'_> {
    /// One line, or nothing where the reader has gone.
    fn line(&mut self) -> Option<String> {
        let mut said = String::new();
        match self.input.read_line(&mut said) {
            Ok(0) | Err(_) => None,
            Ok(_) => Some(said),
        }
    }

    /// Writes the prompt and waits, so a reader is not left with a cursor and no question.
    fn asking(&mut self, keys: &str) -> Option<String> {
        let _written = write!(self.out, "  {keys} > ");
        let _flushed = self.out.flush();
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
        let _written = writeln!(self.out, "\n{drawn}");
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
                    let _written = writeln!(
                        self.out,
                        "  an acceptance with no reason is a mutation nobody looked at, \
                         recorded as one somebody did"
                    );
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
        let _written = writeln!(
            self.out,
            "\n{drawn}\n   the run established nothing here, so there is nothing to accept"
        );
        loop {
            let Some(said) = self.asking("[l]eave  [s]top") else {
                return AboutTheUnsettled::Stop;
            };
            match said.trim() {
                "l" | "leave" | "" => return AboutTheUnsettled::Leave,
                "s" | "stop" | "q" | "quit" => return AboutTheUnsettled::Stop,
                "a" | "accept" => {
                    let _written = writeln!(
                        self.out,
                        "  the run established nothing here, so there is nothing to \
                         accept: find out why before deciding it may stand"
                    );
                }
                _ => {}
            }
        }
    }
}

/// Shows one run's gaps one at a time and records what a reviewer decided.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
    let report = match runs::report(root, &run) {
        Ok(report) => report,
        Err(error) => {
            super::diagnose(stderr, &error.to_string());
            return EXIT_ERROR;
        }
    };
    let store = crate::app::reports::Store::read(root);
    let kept = store.said(&store.run(&run).join(crate::app::reports::DOCUMENT_NAME));
    let sources = crate::presentation::Sources::read(root, &report);
    let told = crate::presentation::Told::of(&report, &sources, &kept);

    let decided = {
        let mut asking = AtTheKeyboard {
            input: std::io::stdin().lock(),
            out: stdout,
        };
        crate::presentation::review::review(&told, environment.terminal, &mut asking)
    };
    said(stdout, &decided);
    EXIT_ASSURED
}

/// What the review came to, and the commands that record it.
///
/// The decisions are printed rather than written, because a review is a person
/// deciding and a file is a change to their project: `njutest accept` is the
/// command that makes it, it is already the one every other surface prints,
/// and a reviewer who wants them all runs the lines they were given.
fn said(out: &mut dyn Write, reviewed: &Reviewed) {
    if reviewed.accepted.is_empty() {
        let _written = writeln!(out, "\nnothing accepted; {} left", reviewed.left);
    } else {
        let _written = writeln!(out, "\nrun these to record what you decided:");
        for (named, reason) in &reviewed.accepted {
            let _written = writeln!(out, "  njutest accept {named} --reason {:?}", reason.said());
        }
    }
    if let Some(stopped) = &reviewed.stopped_at {
        let _written = writeln!(out, "stopped at {stopped}");
    }
}
