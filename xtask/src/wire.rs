// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An independent re-derivation of the faults a run's seam recording licensed. Nothing here calls the runner's catalogue: the rules and the identity recipe are written out again from what `docs/trace-v1.md` and `docs/report-v1.md` say they are, so a run and this audit agreeing means two implementations agreed.

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

/// Everything the recording says about the seams, read from the stream alone.
#[must_use]
pub fn read(recorded: &str) -> Watched {
    let mut watched = Watched::default();
    for line in recorded.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(event) = serde_json::from_str::<Value>(line) else {
            continue;
        };
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
    watched
}

/// Every fault an exchange licenses, re-derived rather than read back.
#[must_use]
pub fn licensed(exchange: &Exchange) -> Vec<(String, String)> {
    let mut rules: Vec<&str> = UNPARSED.to_vec();
    if exchange.seq > 0 {
        rules.extend(PRECEDED);
    }
    if exchange.wire == "http" {
        rules.extend(PARSED);
    }
    rules
        .into_iter()
        .map(|rule| (identity(exchange, rule), rule.to_owned()))
        .collect()
}

/// The identity of one question about one exchange, minted from those alone.
#[must_use]
pub fn identity(exchange: &Exchange, rule: &str) -> String {
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
    for part in [
        ID_DOMAIN,
        &exchange.capability,
        &exchange.seq.to_string(),
        rule,
        &spoken,
    ] {
        let bytes = part.as_bytes();
        hasher.update(u32::try_from(bytes.len()).unwrap_or(u32::MAX).to_be_bytes());
        hasher.update(bytes);
    }
    hex::encode(hasher.finalize())
}

/// One exchange, as the recording writes it.
fn exchange(record: &Value) -> Exchange {
    Exchange {
        capability: text(record, "capability").unwrap_or_default(),
        seq: number(record, "seq").unwrap_or_default(),
        wire: text(record, "wire").unwrap_or_default(),
        method: text(record, "method"),
        path: text(record, "path"),
        status: number(record, "status").and_then(|one| u16::try_from(one).ok()),
    }
}

/// One fault execution, as the recording writes it.
fn exec(record: &Value) -> Exec {
    Exec {
        fault: text(record, "fault").unwrap_or_default(),
        capability: text(record, "capability").unwrap_or_default(),
        seq: number(record, "seq").unwrap_or_default(),
        rule: text(record, "rule").unwrap_or_default(),
        decision: text(record, "decision").unwrap_or_default(),
        noticed_by: text(record, "noticed_by"),
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
