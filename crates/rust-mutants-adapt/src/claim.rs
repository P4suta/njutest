// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What one claim of a configuration comes to, once the locator a run finds it by has answered.

/// What one claim of a configuration names in a tree read without building it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// It names as many mutations as it says, by display identity.
    Names {
        /// The display identities, in catalog order.
        mutants: Vec<String>,
    },
    /// It names as many mutations as it says, and the line it holds is not where the first of them now is.
    Moved {
        /// The display identities, in catalog order.
        mutants: Vec<String>,
        /// The line the claim holds.
        from: u32,
        /// The line the first of them is on now.
        to: u32,
    },
    /// What it names sits only in a file no unit of this build reads, so it is judged where one does (ADR 0042).
    Uncompiled,
    /// It names nothing, or not as many as it says, so a run finds it unmatched.
    Unmatched {
        /// Why, in the words a run gives it.
        why: String,
    },
}

/// What a claim's locator named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Named {
    /// The display identities, in catalog order.
    pub mutants: Vec<String>,
    /// The line the claim holds and the line the first of them is on now, where the two differ.
    pub moved: Option<(u32, u32)>,
}

impl Resolution {
    /// What a claim whose locator named `named` comes to: the mutations it names, and the line they are on now where its own line is not theirs.
    #[must_use]
    pub fn named(named: Named) -> Self {
        let Named { mutants, moved } = named;
        match moved {
            None => Self::Names { mutants },
            Some((from, to)) => Self::Moved { mutants, from, to },
        }
    }

    /// What a claim whose locator named nothing comes to, `why` saying why in the words a run gives it: judged where another build reads what it names, which `uncompiled` says, and unmatched otherwise.
    #[must_use]
    pub fn unnamed(why: String, uncompiled: impl FnOnce() -> bool) -> Self {
        if uncompiled() {
            Self::Uncompiled
        } else {
            Self::Unmatched { why }
        }
    }

    /// Whether the claim says something that is not so: it names nothing, not as many as it says, or a line its mutation left.
    #[must_use]
    pub const fn rotted(&self) -> bool {
        match self {
            Self::Unmatched { .. } | Self::Moved { .. } => true,
            Self::Names { .. } | Self::Uncompiled => false,
        }
    }

    /// Whether what it names sits only in a file another build reads.
    #[must_use]
    pub const fn uncompiled(&self) -> bool {
        match self {
            Self::Uncompiled => true,
            Self::Names { .. } | Self::Moved { .. } | Self::Unmatched { .. } => false,
        }
    }
}

#[cfg(test)]
mod tests;
