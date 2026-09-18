// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an interposer records about one seam: every exchange, in the words of the protocol that carried it.

pub mod derive;
pub mod dialled;
pub mod interpose;
pub mod prove;
pub mod rule;
pub mod settle;

use serde::{Deserialize, Serialize};

/// How much of what goes past an interposer is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Wire {
    /// Read nothing but what needs no parsing, which is what a seam nobody named the protocol of gets.
    #[default]
    Raw,
    /// Read the request line and the status line, and count the rest.
    Http,
}

/// Names the shape, so a later reader is never guessing what it holds.
pub const SCHEMA: &str = "njutest-wire-v1";

/// What one exchange was, in the words of the protocol that carried it.
///
/// A closed set, because a catalogue of faults is derived from these and a
/// derivation that cannot read a protocol has nothing to propose about it.
/// [`Self::Raw`] is the honest answer for one the interposer does not parse:
/// how long it took and how much went each way, which is enough to delay it,
/// cut it short, or drop it, and not enough to invent a status for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "wire", rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Spoken {
    /// One HTTP request and the response it got.
    Http {
        /// The method.
        method: String,
        /// The path, without the query.
        path: String,
        /// The status the upstream answered with.
        status: u16,
        /// How many bytes the request carried.
        request_bytes: u64,
        /// How many bytes came back.
        response_bytes: u64,
        /// How many of those were the body, which is what says whether cutting it short changes anything.
        body_bytes: u64,
        /// The status line as the upstream wrote it, which is what says whether restating it changes anything.
        status_line: String,
    },
    /// One exchange over a protocol the interposer does not parse.
    Raw {
        /// How many bytes went upstream.
        request_bytes: u64,
        /// How many came back.
        response_bytes: u64,
    },
}

/// One exchange over one seam, as the interposer recorded it.
///
/// Not `deny_unknown_fields`: serde cannot refuse an unknown field and flatten
/// one in the same breath, and the protocol has to be a field of the line
/// rather than a table under it. The published schema is what refuses a
/// document with something extra in it, which is where `docs/report-v1.md`
/// says that job belongs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Exchange {
    /// The capability the seam serves, as `[resources.<name>]` names it.
    pub capability: String,
    /// Which exchange this is on that seam, from zero.
    pub seq: u64,
    /// The test that was running, when the run could tell. `None` says it could not, rather than naming the last one it happened to know.
    pub during: Option<String>,
    /// How long the upstream took.
    pub duration_ms: u64,
    /// What was said, and by which protocol.
    #[serde(flatten)]
    pub spoken: Spoken,
}

/// One line of the stream: the exchange, and the schema it answers to.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Line {
    /// Always [`SCHEMA`].
    schema: String,
    /// The exchange itself.
    #[serde(flatten)]
    exchange: Exchange,
}

/// What a reader got back, and how much of the stream it could not take.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Read {
    /// The exchanges, in the order the stream held them.
    pub exchanges: Vec<Exchange>,
    /// How many non-empty lines were not exchanges this release understands.
    pub unread: u64,
}

/// The stream an interposer writes: one exchange per line, each naming the schema.
#[must_use]
pub fn written(exchanges: &[Exchange]) -> String {
    let mut out = String::new();
    for exchange in exchanges {
        let line = Line {
            schema: SCHEMA.to_owned(),
            exchange: exchange.clone(),
        };
        if let Ok(text) = serde_json::to_string(&line) {
            out.push_str(&text);
            out.push('\n');
        }
    }
    out
}

/// What `recorded` holds, with the lines it could not take counted rather than passed over.
///
/// A recording a reader silently shortened is one a derivation would take for
/// a seam that was quieter than it was, so the count travels with the answer.
#[must_use]
pub fn read(recorded: &str) -> Read {
    let mut answer = Read::default();
    for line in recorded.lines().filter(|line| !line.trim().is_empty()) {
        match serde_json::from_str::<Line>(line) {
            Ok(held) if held.schema == SCHEMA => answer.exchanges.push(held.exchange),
            _ => answer.unread = answer.unread.saturating_add(1),
        }
    }
    answer
}
