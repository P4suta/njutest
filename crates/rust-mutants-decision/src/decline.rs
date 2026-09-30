// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a survival comes to once the tests that declined to measure in it are held to the ones that declined in its baseline (ADR 0043).

/// What one execution's declines leave of it, held to the declines the run's baseline made of the same target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Held<'a, T> {
    /// Every decline is one the baseline made, in the same words: these tests are set aside.
    SetAside,
    /// A test declined where the baseline's did not, or gave other words: the mutation changed what the test did, which is a detection.
    Detected {
        /// The first such decline.
        by: &'a T,
    },
}

/// `declined`, one execution's believed declines, held to `baseline`, the declines the baseline made in the same target.
#[must_use]
pub fn held<'a, T: PartialEq>(declined: &'a [T], baseline: &[T]) -> Held<'a, T> {
    match declined.iter().find(|one| !baseline.contains(one)) {
        Some(by) => Held::Detected { by },
        None => Held::SetAside,
    }
}

/// What a survival comes to, held to the notice of the process that established it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Concluded<'a, T> {
    /// The notice cannot be believed, and neither can the pass it came with.
    Errored,
    /// A test declined where the baseline's did not, or in other words, which is a detection.
    DeclinedUnderTheMutant {
        /// The first such decline.
        by: &'a T,
    },
    /// Every test the process passed declined as the baseline's did, so the process measured nothing.
    Declined,
    /// The survival stands: no test declined, or some that declined as the baseline's did are set aside and the others passed.
    Survived,
}

/// What a survival whose process passed `passed` tests comes to, where its notice was believed to decline `believed`, or was not believed, held to `baseline`, the declines the baseline made in the same target.
#[must_use]
pub fn concluded<'a, T: PartialEq>(
    believed: Option<&'a [T]>,
    baseline: &[T],
    passed: usize,
) -> Concluded<'a, T> {
    let Some(declined) = believed else {
        return Concluded::Errored;
    };
    match held(declined, baseline) {
        Held::Detected { by } => Concluded::DeclinedUnderTheMutant { by },
        Held::SetAside if !declined.is_empty() && declined.len() == passed => Concluded::Declined,
        Held::SetAside => Concluded::Survived,
    }
}

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod kani_laws;
