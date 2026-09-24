// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Exploring the schedules of a binary not proven single-threaded: which guards to delay, and what the delayed controls establish.

use std::collections::BTreeSet;

/// The at most `count` sites of `reached` to delay, spread across it rather than taken from its start, the same on every run.
#[must_use]
pub fn chosen(reached: &BTreeSet<u32>, count: u32) -> Vec<u32> {
    let mut ordered: Vec<([u8; 32], u32)> =
        reached.iter().map(|site| (key(*site), *site)).collect();
    ordered.sort_unstable();
    ordered
        .into_iter()
        .zip(0..count)
        .map(|((_, site), _)| site)
        .collect()
}

/// The order a site is chosen in: the SHA-256 of its index, so the chosen are spread across the catalog and the same on every run.
fn key(site: u32) -> [u8; 32] {
    use sha2::Digest as _;
    sha2::Sha256::digest(site.to_le_bytes()).into()
}

/// How one control under a delay ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// Every test it ran passed.
    Passed,
    /// These tests failed.
    Failed(Vec<String>),
    /// It established nothing: it ran past its bound or could not be read.
    Unsettled,
}

/// What delaying one site established, given the first delayed control and, where it failed, the two that repeat it and the one undelayed control run beside them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delayed {
    /// The tests passed with the site delayed.
    Passed,
    /// The tests fail with the site delayed, twice more, and pass without the delay: the schedule decides the verdict.
    Broke {
        /// The tests that failed, as the first delayed control named them.
        failed: Vec<String>,
    },
    /// Nothing was established: a control was unsettled, a repeat passed, or the undelayed control failed too.
    Undecided,
}

/// What `first`, and where it failed `repeats` and `undelayed`, establish about one delayed site.
#[must_use]
pub fn delayed(first: &Ended, repeats: &[Ended], undelayed: Option<&Ended>) -> Delayed {
    match first {
        Ended::Passed => Delayed::Passed,
        Ended::Unsettled => Delayed::Undecided,
        Ended::Failed(failed) => {
            let repeated = repeats.len() == 2
                && repeats
                    .iter()
                    .all(|repeat| matches!(repeat, Ended::Failed(_)));
            let clean = matches!(undelayed, Some(Ended::Passed));
            if repeated && clean {
                Delayed::Broke {
                    failed: failed.clone(),
                }
            } else {
                Delayed::Undecided
            }
        }
    }
}
