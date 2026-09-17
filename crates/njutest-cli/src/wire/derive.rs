// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The faults a recording of a seam licenses, derived from what went past rather than guessed at.

use sha2::{Digest as _, Sha256};

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
    pub rule: String,
    /// One sentence saying what a run would do.
    pub asks: String,
}

/// What a run may ask of an exchange whose answer nothing parsed.
///
/// Cutting short, holding up and dropping need no reading, so they are
/// licensed by any exchange at all.
const UNPARSED: [(&str, &str); 4] = [
    (
        "truncate-response",
        "cut the answer short, as a connection that died mid-body does",
    ),
    (
        "delay-response",
        "hold the answer back past what the caller waits for",
    ),
    (
        "drop-connection",
        "drop the connection with nothing said back",
    ),
    (
        "replay-request",
        "deliver the same request twice, as a retry after a lost answer does",
    ),
];

/// What a run may ask only of an exchange something came before on the same seam.
///
/// Answering with what the one before it got is a question about a caller
/// reading a replica that has not caught up, and the first exchange on a seam
/// has nothing to be answered with.
const PRECEDED: [(&str, &str); 1] = [(
    "stale-response",
    "answer with what the exchange before it got, as a replica behind the writer does",
)];

/// What a run may ask of an exchange whose answer it read.
const PARSED: [(&str, &str); 2] = [
    (
        "status-server-error",
        "answer 500 where the upstream answered otherwise",
    ),
    (
        "status-not-found",
        "answer 404 where the upstream answered otherwise",
    ),
];

/// Every fault the `observed` exchanges license, in the order they were observed.
///
/// Nothing is proposed about an exchange that did not happen, and nothing is
/// proposed about a protocol the interposer did not read: a question put to
/// an answer nobody parsed is a question about a program this run never saw.
#[must_use]
pub fn derive(observed: &[Exchange]) -> Vec<Fault> {
    let mut faults = Vec::new();
    for exchange in observed {
        for (rule, asks) in UNPARSED {
            faults.push(one(exchange, rule, asks));
        }
        if exchange.seq > 0 {
            for (rule, asks) in PRECEDED {
                faults.push(one(exchange, rule, asks));
            }
        }
        if matches!(exchange.spoken, Spoken::Http { .. }) {
            for (rule, asks) in PARSED {
                faults.push(one(exchange, rule, asks));
            }
        }
    }
    faults
}

/// One fault, named by the exchange it is about and the rule it applies.
fn one(exchange: &Exchange, rule: &str, asks: &str) -> Fault {
    Fault {
        id: identity(exchange, rule),
        capability: exchange.capability.clone(),
        seq: exchange.seq,
        during: exchange.during.clone(),
        rule: rule.to_owned(),
        asks: asks.to_owned(),
    }
}

/// The identity of one question about one exchange, reproducible from those alone.
fn identity(exchange: &Exchange, rule: &str) -> String {
    let mut hasher = Sha256::new();
    for part in [
        ID_DOMAIN,
        &exchange.capability,
        &exchange.seq.to_string(),
        rule,
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
