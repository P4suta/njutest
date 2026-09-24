// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-derivation of the faults a run's seam recording licensed.
//!
//! Nothing here calls the runner's catalogue: the rules and the identity recipe are written out again from the runner trace and assurance contracts, so a run and this audit agreeing means two implementations agreed.

use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// Separates a fault identity from every other kind this workspace mints.
pub const ID_DOMAIN: &str = "njutest-fault-id-v1";

/// What a run may ask of any exchange, whatever the interposer read of it.
pub const UNPARSED: [&str; 4] = [
    "truncate-response",
    "delay-response",
    "drop-connection",
    "replay-request",
];

/// What a run may ask only of an exchange something came before on the same seam.
pub const PRECEDED: [&str; 1] = ["stale-response"];

/// What a run may ask only of an exchange whose answer it read.
pub const PARSED: [&str; 2] = ["status-server-error", "status-not-found"];

/// One exchange the recording says went past a seam.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exchange {
    /// The capability the seam serves.
    pub capability: String,
    /// Where it fell in the order on that seam.
    pub seq: u64,
    /// How much of it was read.
    pub wire: String,
    /// What was asked for, where the wire says how to read one.
    pub method: Option<String>,
    /// Where it was asked of, where the wire says how to read one.
    pub path: Option<String>,
    /// What the upstream answered, where the wire says how to read one.
    pub status: Option<u16>,
}

/// One fault the recording says a run put to the suite.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Exec {
    /// The fault's identity.
    pub fault: String,
    /// The seam it names.
    pub capability: String,
    /// The exchange it names.
    pub seq: u64,
    /// What it asked the seam to do.
    pub rule: String,
    /// Who decided it.
    pub decision: String,
    /// The target that noticed, where one did.
    pub noticed_by: Option<String>,
}

/// What a recording holds about the seams a run watched.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Watched {
    /// Every exchange that went past, in the order the recording holds them.
    pub exchanges: Vec<Exchange>,
    /// Every fault the run put.
    pub execs: Vec<Exec>,
}

/// Why the independent audit could not mint the identity that the producer is required to mint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
    /// A length-prefixed identity field lies outside the v1 recipe.
    #[error("the {field} identity field has {bytes} bytes; the v1 recipe permits at most u32::MAX")]
    FieldTooLong {
        /// Which field exceeded the recipe.
        field: &'static str,
        /// Its exact byte length.
        bytes: usize,
    },
}

/// Everything the recording says about the seams, read from the stream alone.
///
/// # Errors
/// A corrupt non-empty line is rejected rather than disappearing from the evidence.
pub fn read(recorded: &str) -> Result<Watched, crate::route::ReadError> {
    let mut watched = Watched::default();
    for event in crate::route::events(recorded, crate::schemas::Producer::Runner)? {
        match text(&event, "type").as_deref() {
            Some("wire-exchange") => {
                if let Some(record) = event.get("exchange") {
                    watched.exchanges.push(exchange(record));
                }
            }
            Some("wire-exec") => {
                if let Some(record) = event.get("wire") {
                    watched.execs.push(exec(record));
                }
            }
            _ => {}
        }
    }
    Ok(watched)
}

/// Every fault an exchange licenses, re-derived rather than read back.
///
/// # Errors
/// Returns [`IdentityError::FieldTooLong`] when any field lies outside the length-prefixed v1 identity recipe.
pub fn licensed(exchange: &Exchange) -> Result<Vec<(String, String)>, IdentityError> {
    let mut rules: Vec<&str> = UNPARSED.to_vec();
    if exchange.seq > 0 {
        rules.extend(PRECEDED);
    }
    if exchange.wire == "http" {
        rules.extend(PARSED);
    }
    rules
        .into_iter()
        .map(|rule| identity(exchange, rule).map(|id| (id, rule.to_owned())))
        .collect()
}

/// The identity of one question about one exchange, minted from those alone.
///
/// # Errors
/// Returns [`IdentityError::FieldTooLong`] when any field lies outside the length-prefixed v1 identity recipe.
pub fn identity(exchange: &Exchange, rule: &str) -> Result<String, IdentityError> {
    let spoken = if exchange.wire == "http" {
        format!(
            "http {} {} {}",
            exchange.method.clone().unwrap_or_default(),
            exchange.path.clone().unwrap_or_default(),
            exchange.status.unwrap_or_default()
        )
    } else {
        "raw".to_owned()
    };
    let mut hasher = Sha256::new();
    let sequence = exchange.seq.to_string();
    for (field, part) in [
        ("domain", ID_DOMAIN),
        ("capability", exchange.capability.as_str()),
        ("sequence", sequence.as_str()),
        ("rule", rule),
        ("spoken", spoken.as_str()),
    ] {
        let bytes = part.as_bytes();
        let length = field_length(field, bytes.len())?;
        hasher.update(length.to_be_bytes());
        hasher.update(bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn field_length(field: &'static str, bytes: usize) -> Result<u32, IdentityError> {
    match u32::try_from(bytes) {
        Ok(length) => Ok(length),
        Err(_outside_wire_recipe) => Err(IdentityError::FieldTooLong { field, bytes }),
    }
}

/// One exchange, as the recording writes it.
fn exchange(record: &Value) -> Exchange {
    let read = record.get("read").unwrap_or(&Value::Null);
    Exchange {
        capability: text(record, "capability").unwrap_or_default(),
        seq: number(record, "seq").unwrap_or_default(),
        wire: text(read, "wire").unwrap_or_default(),
        method: text(read, "method"),
        path: text(read, "path"),
        status: number(read, "status").and_then(|one| match u16::try_from(one) {
            Ok(status) => Some(status),
            Err(_) => None,
        }),
    }
}

fn exec(record: &Value) -> Exec {
    let answer = record.get("answer").unwrap_or(&Value::Null);
    Exec {
        fault: text(record, "fault").unwrap_or_default(),
        capability: text(record, "capability").unwrap_or_default(),
        seq: number(record, "seq").unwrap_or_default(),
        rule: text(record, "rule").unwrap_or_default(),
        decision: text(answer, "decision").unwrap_or_default(),
        noticed_by: text(answer, "noticed_by"),
    }
}

/// One string field, or nothing where the recording does not carry it.
fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

/// One whole number field, or nothing where the recording does not carry it.
fn number(value: &Value, key: &str) -> Option<u64> {
    value.get(key).and_then(Value::as_u64)
}
