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

/// How many rounds confirm a delayed failure: each a delayed control that fails exactly the same tests and an undelayed one that passes.
///
/// A test that fails at a rate `p` whatever the delay passes all of them with probability `p^6 (1-p)^5`, which is under one in a thousand at its worst.
pub const CONFIRMING_ROUNDS: u32 = 5;

/// Whether `delayed` fails exactly the tests `failed` names, in any order: the same failure again, not another.
#[must_use]
pub fn repeats(failed: &[String], delayed: &Ended) -> bool {
    match delayed {
        Ended::Failed(again) => {
            let first: BTreeSet<&String> = failed.iter().collect();
            let second: BTreeSet<&String> = again.iter().collect();
            first == second
        }
        Ended::Passed | Ended::Unsettled => false,
    }
}

/// Whether an undelayed control passed, which is what makes a delayed failure the delay's.
#[must_use]
pub const fn clean(undelayed: &Ended) -> bool {
    match undelayed {
        Ended::Passed => true,
        Ended::Failed(_) | Ended::Unsettled => false,
    }
}
