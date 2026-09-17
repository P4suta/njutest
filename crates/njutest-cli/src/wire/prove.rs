// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The questions about a seam that need no run, because the caller would be handed the bytes it was handed already.

use super::derive::Fault;
use super::{Exchange, Spoken};

/// The proof that cutting an answer with no body short changes nothing.
pub const NO_BODY_TO_CUT: &str = "no-body-to-cut";

/// Why `fault` needs no run, or nothing where it needs one.
///
/// Cutting an answer short keeps everything up to and including the blank line
/// that ends its head, so an answer that is all head comes back byte for byte
/// the same. Nothing that reads bytes can tell the two apart, which is a
/// stronger answer than any run: not "no test noticed" but "no observer
/// could". Every other question changes what the caller is handed, or when, so
/// nothing is claimed about it here — a proof layer that guessed would report
/// a gap as assured, which is the one answer this must never give.
#[must_use]
pub fn discharges(fault: &Fault, observed: &[Exchange]) -> Option<&'static str> {
    if fault.rule != "truncate-response" {
        return None;
    }
    let named = observed
        .iter()
        .find(|one| one.capability == fault.capability && one.seq == fault.seq)?;
    match named.spoken {
        Spoken::Http { body_bytes: 0, .. } => Some(NO_BODY_TO_CUT),
        Spoken::Http { .. } | Spoken::Raw { .. } => None,
    }
}
