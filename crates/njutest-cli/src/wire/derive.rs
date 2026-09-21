// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The faults a recording of a seam licenses, derived from what went past rather than guessed at.

use sha2::{Digest as _, Sha256};

use super::rule::Rule;
use super::{Exchange, Spoken};

/// Separates these identities from every other kind this workspace mints.
pub const ID_DOMAIN: &str = "njutest-fault-id-v1";

/// Why an observed exchange could not be given the v1 identity its bytes
/// require.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeriveError {
    /// One length prefix is wider than the v1 u32 identity recipe.
    #[error(
        "fault identity field {field} contains {bytes} bytes, outside the v1 u32 length prefix"
    )]
    FieldTooLong {
        /// The closed recipe field.
        field: &'static str,
        /// Its exact byte length.
        bytes: usize,
    },
}

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
/// # Errors
/// Returns [`DeriveError`] rather than clipping a v1 length prefix.
pub fn derive(observed: &[Exchange]) -> Result<Vec<Fault>, DeriveError> {
    let mut faults = Vec::new();
    for exchange in observed {
        for rule in Rule::UNPARSED {
            faults.push(one(exchange, rule)?);
        }
        if exchange.seq > 0 {
            for rule in Rule::PRECEDED {
                faults.push(one(exchange, rule)?);
            }
        }
        if matches!(exchange.spoken, Spoken::Http { .. }) {
            for rule in Rule::PARSED {
                faults.push(one(exchange, rule)?);
            }
        }
    }
    Ok(faults)
}

/// One fault, named by the exchange it is about and the rule it applies.
fn one(exchange: &Exchange, rule: Rule) -> Result<Fault, DeriveError> {
    Ok(Fault {
        id: identity(exchange, rule)?,
        capability: exchange.capability.clone(),
        seq: exchange.seq,
        during: exchange.during.clone(),
        rule,
    })
}

/// The identity of one question about one exchange, reproducible from those alone.
fn identity(exchange: &Exchange, rule: Rule) -> Result<String, DeriveError> {
    let mut hasher = Sha256::new();
    let sequence = exchange.seq.to_string();
    let spoken = spoken(&exchange.spoken);
    for (field, part) in [
        ("domain", ID_DOMAIN),
        ("capability", exchange.capability.as_str()),
        ("sequence", sequence.as_str()),
        ("rule", rule.name()),
        ("spoken", spoken.as_str()),
    ] {
        let bytes = part.as_bytes();
        let length =
            u32::try_from(bytes.len()).map_err(|_outside_recipe| DeriveError::FieldTooLong {
                field,
                bytes: bytes.len(),
            })?;
        hasher.update(length.to_be_bytes());
        hasher.update(bytes);
    }
    Ok(hex::encode(hasher.finalize()))
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
