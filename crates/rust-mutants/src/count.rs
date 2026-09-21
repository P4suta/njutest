// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Counts that know what they are counting.

use std::marker::PhantomData;

/// What a count is a count of.
///
/// A run counts six different things and used to count them all in `u64`.
/// Nothing stopped a line printing a count of pairs beside a count of mutants,
/// and one did: a reader who added them was adding two different quantities,
/// and the only way to find out was to try.
/// The unit is in the type now, so the addition does not compile and the line has to say which it is showing.
pub trait Unit {
    /// The word a reader sees, plural, because a count of one is the exception.
    const PLURAL: &'static str;
}

/// One mutant put to one target: the unit a run spends, and what a process is started for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pairs;

/// One mutation of the catalog, whatever number of targets it is put to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mutants;

/// One test of one target, which is what a pair costs when its route names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Tests;

/// One test binary the workspace builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Targets;

/// One place the rules target, which produces a candidate or a reason it did not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Places;

impl Unit for Pairs {
    const PLURAL: &'static str = "pairs";
}

impl Unit for Mutants {
    const PLURAL: &'static str = "mutants";
}

impl Unit for Tests {
    const PLURAL: &'static str = "tests";
}

impl Unit for Targets {
    const PLURAL: &'static str = "targets";
}

impl Unit for Places {
    const PLURAL: &'static str = "places";
}

/// How many of `U` there are.
///
/// There is no `Display`: a count cannot reach a reader without the code saying which unit it is in, which is what [`Count::said`] does and what the line that lost a reader did not.
pub struct Count<U: Unit> {
    of: u64,
    unit: PhantomData<U>,
}

impl<U: Unit> Count<U> {
    /// This many.
    #[must_use]
    pub const fn new(of: u64) -> Self {
        Self {
            of,
            unit: PhantomData,
        }
    }

    /// The number, for arithmetic that has already been shown to be about one unit.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.of
    }

    /// The count and the word for what it counts, which is the only way it reaches a reader.
    #[must_use]
    pub fn said(self) -> String {
        format!("{} {}", self.of, U::PLURAL)
    }

    /// This many more of the same thing, or nothing when the exact count does not fit.
    #[must_use]
    pub const fn checked_add(self, other: Self) -> Option<Self> {
        match self.of.checked_add(other.of) {
            Some(of) => Some(Self::new(of)),
            None => None,
        }
    }

    /// The exact difference, or nothing when `other` is larger.
    #[must_use]
    pub const fn checked_sub(self, other: Self) -> Option<Self> {
        match self.of.checked_sub(other.of) {
            Some(of) => Some(Self::new(of)),
            None => None,
        }
    }

    /// What share of `whole` this is, or nothing where the whole is nothing.
    ///
    /// A share of nothing is not nought per cent, and a reader shown one reads a run that measured everything as a run that measured nothing.
    #[must_use]
    pub fn share_of(self, whole: Self) -> Option<f64> {
        ratio(self.of, whole.of)
    }
}

/// `part / whole` without truncating either 64-bit count to an apparently valid smaller count.
#[must_use]
pub(crate) fn ratio(part: u64, whole: u64) -> Option<f64> {
    (whole != 0).then(|| widen(part) / widen(whole))
}

fn widen(value: u64) -> f64 {
    let bytes = value.to_be_bytes();
    let high = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    let low = u32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
    f64::from(high).mul_add(4_294_967_296.0, f64::from(low))
}

impl Count<Mutants> {
    /// These mutants put to every one of `targets`, which is that many pairs.
    ///
    /// This is the only way to get from one unit to the other, and it is the conversion the arithmetic used to make in silence: a line that showed mutants beside the pairs they came to invited a reader to add two different quantities, and the multiplication that joins them was a bare `*` nobody had to name.
    #[must_use]
    pub const fn checked_against(self, targets: Count<Targets>) -> Option<Count<Pairs>> {
        match self.of.checked_mul(targets.of) {
            Some(of) => Some(Count::new(of)),
            None => None,
        }
    }
}

impl<U: Unit> Clone for Count<U> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<U: Unit> Copy for Count<U> {}

impl<U: Unit> Default for Count<U> {
    fn default() -> Self {
        Self::new(0)
    }
}

impl<U: Unit> PartialEq for Count<U> {
    fn eq(&self, other: &Self) -> bool {
        self.of == other.of
    }
}

impl<U: Unit> Eq for Count<U> {}

impl<U: Unit> PartialOrd for Count<U> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<U: Unit> Ord for Count<U> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.of.cmp(&other.of)
    }
}

impl<U: Unit> std::fmt::Debug for Count<U> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.of, U::PLURAL)
    }
}

impl<U: Unit> From<u64> for Count<U> {
    fn from(of: u64) -> Self {
        Self::new(of)
    }
}

impl<U: Unit> From<u32> for Count<U> {
    fn from(of: u32) -> Self {
        Self::new(u64::from(of))
    }
}
