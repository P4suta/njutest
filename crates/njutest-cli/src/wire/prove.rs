// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The questions about a seam that need no run, because the caller would be handed the bytes it was handed already.

use super::derive::Fault;
use super::rule::Rule;
use super::{Exchange, Spoken};

/// The proof that cutting an answer with no body short changes nothing.
pub const NO_BODY_TO_CUT: &str = "no-body-to-cut";

/// The proof that restating a status line writes the line the upstream already wrote.
pub const ALREADY_THAT_ANSWER: &str = "already-that-answer";

/// Why `fault` needs no run, or nothing where it needs one.
///
/// Cutting an answer short keeps everything up to and including the blank line
/// that ends its head, so an answer that is all head comes back byte for byte
/// the same; restating a status line as what it already says writes the line
/// that is already there. Nothing that reads bytes can tell either pair apart,
/// which is a stronger answer than any run: not "no test noticed" but "no
/// observer could". Every other question changes what the caller is handed, or
/// when, so nothing is claimed about it here — a proof layer that guessed
/// would report a gap as assured, which is the one answer this must never
/// give.
#[must_use]
pub fn discharges(fault: &Fault, observed: &[Exchange]) -> Option<&'static str> {
    let named = observed
        .iter()
        .find(|one| one.capability == fault.capability && one.seq == fault.seq)?;
    let Spoken::Http {
        body_bytes,
        status_line,
        ..
    } = &named.spoken
    else {
        return None;
    };
    match fault.rule {
        Rule::TruncateResponse if *body_bytes == 0 => Some(NO_BODY_TO_CUT),
        Rule::StatusServerError | Rule::StatusNotFound => {
            already(status_line, fault.rule.restates()?)
        }
        Rule::TruncateResponse
        | Rule::DelayResponse
        | Rule::DropConnection
        | Rule::ReplayRequest
        | Rule::StaleResponse => None,
    }
}

/// Whether restating `line` as `status` writes the line that is already there.
///
/// Asked of the function that does the restating, so the proof is a check on
/// the injection rather than a second opinion about it. A 500 whose reason
/// phrase differs from the one a run writes is a different answer, and a proof
/// that held only most of the time would not be a proof.
fn already(line: &str, (status, reason): (u16, &str)) -> Option<&'static str> {
    let written = super::interpose::restated_line(line, status, reason);
    (written == line).then_some(ALREADY_THAT_ANSWER)
}
