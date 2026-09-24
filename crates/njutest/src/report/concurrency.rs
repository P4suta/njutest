// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run establishes about whether each test binary runs one thread, as a report keeps it.

use serde::{Deserialize, Serialize};

use crate::concurrency::proof::Standing;

use super::Limitation;

/// What a run established about one test binary's threads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConcurrencyRecord {
    /// The binary, as `package/kind/name`.
    pub target: String,
    /// What was established.
    pub standing: Standing,
}

/// The limitation the records earn: every binary not proven to run one thread, named in the closing list, since no schedule of any is explored yet.
#[must_use]
pub fn limited(records: &[ConcurrencyRecord]) -> Option<Limitation> {
    let named: Vec<&str> = records
        .iter()
        .filter(|record| match record.standing {
            Standing::SingleThreaded => false,
            Standing::Concurrent { .. } | Standing::NotProven { .. } => true,
        })
        .map(|record| record.target.as_str())
        .collect();
    if named.is_empty() {
        return None;
    }
    Some(Limitation::new(
        crate::limitation::SCHEDULE_NOT_EXPLORED,
        &format!(
            "{} not proven to run one thread and no schedule of {} was explored, so what {} \
             when {} threads interleave otherwise is not known ({})",
            if named.len() == 1 {
                "1 test binary is".to_owned()
            } else {
                format!("{} test binaries are", named.len())
            },
            if named.len() == 1 { "it" } else { "them" },
            if named.len() == 1 {
                "it does"
            } else {
                "they do"
            },
            if named.len() == 1 { "its" } else { "their" },
            named.join(", ")
        ),
    ))
}

/// Whether the records name each binary once, in order: what a part is refused for where they do not.
#[must_use]
pub fn ordered(records: &[ConcurrencyRecord]) -> bool {
    records
        .windows(2)
        .all(|pair| matches!(pair, [one, next] if one.target < next.target))
}
