// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The faults a recording of a seam licenses, derived from what went past rather than guessed at.

use sha2::{Digest as _, Sha256};

use super::rule::Rule;
use super::{Exchange, Spoken};

/// Separates these identities from every other kind this workspace mints.
pub const ID_DOMAIN: &str = "njutest-fault-id-v1";

/// One perturbation of one exchange a run observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    /// The stable identity, minted from the exchange and the rule alone.
    pub id: String,
    /// The capability the seam serves.
    pub capability: String,
    /// Which exchange on that seam this is about.
    pub seq: u64,
    /// The test that was running when it happened, when the run could tell.
    pub during: Option<String>,
    /// What this asks of the exchange.
    pub rule: Rule,
}

impl Fault {
    /// One sentence saying what a run would do, which is what a reader is told.
    #[must_use]
    pub const fn asks(&self) -> &'static str {
        self.rule.asks()
    }
}

/// Every fault the `observed` exchanges license, in the order they were observed.
///
/// Nothing is proposed about an exchange that did not happen, and nothing is
/// proposed about a protocol the interposer did not read: a question put to
/// an answer nobody parsed is a question about a program this run never saw.
#[must_use]
pub fn derive(observed: &[Exchange]) -> Vec<Fault> {
    let mut faults = Vec::new();
    for exchange in observed {
        for rule in Rule::UNPARSED {
            faults.push(one(exchange, rule));
        }
        if exchange.seq > 0 {
            for rule in Rule::PRECEDED {
                faults.push(one(exchange, rule));
            }
        }
        if matches!(exchange.spoken, Spoken::Http { .. }) {
            for rule in Rule::PARSED {
                faults.push(one(exchange, rule));
            }
        }
    }
    faults
}

/// One fault, named by the exchange it is about and the rule it applies.
fn one(exchange: &Exchange, rule: Rule) -> Fault {
    Fault {
        id: identity(exchange, rule),
        capability: exchange.capability.clone(),
        seq: exchange.seq,
        during: exchange.during.clone(),
        rule,
    }
}

/// The identity of one question about one exchange, reproducible from those alone.
fn identity(exchange: &Exchange, rule: Rule) -> String {
    let mut hasher = Sha256::new();
    for part in [
        ID_DOMAIN,
        &exchange.capability,
        &exchange.seq.to_string(),
        rule.name(),
        &spoken(&exchange.spoken),
    ] {
        let bytes = part.as_bytes();
        hasher.update(u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
        hasher.update(bytes);
    }
    hex::encode(hasher.finalize())
}

/// What was said, as the identity reads it: the protocol and what it named.
fn spoken(spoken: &Spoken) -> String {
    match spoken {
        Spoken::Http {
            method,
            path,
            status,
            ..
        } => format!("http {method} {path} {status}"),
        Spoken::Raw { .. } => "raw".to_owned(),
    }
}
