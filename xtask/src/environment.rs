// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The environment xtask was started with, read the way the host tells one variable's name from another.

use std::ffi::{OsStr, OsString};

/// How a platform tells two environment variable names apart, held with the engine's `vars::Spelling` to one table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Spelling {
    /// Byte for byte, as a unix machine does.
    Exact,
    /// Without regard to ASCII case, as Windows does.
    AsciiCaseless,
}

impl Spelling {
    /// The rule of the platform this was built for.
    pub const HOST: Self = if cfg!(windows) {
        Self::AsciiCaseless
    } else {
        Self::Exact
    };

    /// Whether `one` and `other` name one variable under this rule.
    #[must_use]
    pub fn same(self, one: &OsStr, other: &OsStr) -> bool {
        match self {
            Self::Exact => one == other,
            Self::AsciiCaseless => one.eq_ignore_ascii_case(other),
        }
    }

    /// Whether `name` begins with `prefix` under this rule.
    #[must_use]
    pub fn begins(self, name: &OsStr, prefix: &str) -> bool {
        let prefix = prefix.as_bytes();
        name.as_encoded_bytes()
            .get(..prefix.len())
            .is_some_and(|head| match self {
                Self::Exact => head == prefix,
                Self::AsciiCaseless => head.eq_ignore_ascii_case(prefix),
            })
    }

    /// The one spelling this rule gives every name it takes as `name`.
    #[must_use]
    pub fn canonical(self, name: &OsStr) -> OsString {
        match self {
            Self::Exact => name.to_owned(),
            Self::AsciiCaseless => name.to_ascii_uppercase(),
        }
    }
}

/// The environment a process was started with, and the rule its names are read by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    spelling: Spelling,
    pairs: Vec<(OsString, OsString)>,
}

impl Environment {
    /// `pairs` as the host reads them.
    #[must_use]
    pub fn of(pairs: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        Self::spelled(Spelling::HOST, pairs)
    }

    /// `pairs` under `spelling`.
    #[must_use]
    pub fn spelled(
        spelling: Spelling,
        pairs: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        Self {
            spelling,
            pairs: pairs.into_iter().collect(),
        }
    }

    /// The rule these names are read by.
    #[must_use]
    pub const fn spelling(&self) -> Spelling {
        self.spelling
    }

    /// The value of `name`, when it is set to something.
    #[must_use]
    pub fn value(&self, name: &str) -> Option<&OsStr> {
        self.pairs
            .iter()
            .find(|(held, value)| self.spelling.same(held, OsStr::new(name)) && !value.is_empty())
            .map(|(_held, value)| value.as_os_str())
    }

    /// Every name set here that begins with `prefix`, as it is spelled here.
    pub fn beginning<'a>(&'a self, prefix: &'a str) -> impl Iterator<Item = &'a OsStr> {
        self.pairs
            .iter()
            .filter(move |(name, _value)| self.spelling.begins(name, prefix))
            .map(|(name, _value)| name.as_os_str())
    }

    /// Every variable whose name `keep` takes, by the one spelling the rule gives it, in name order.
    #[must_use]
    pub fn canonical(&self, keep: impl Fn(&OsStr) -> bool) -> Vec<(OsString, &OsStr)> {
        let mut kept: Vec<(OsString, &OsStr)> = self
            .pairs
            .iter()
            .filter(|(name, _value)| keep(name))
            .map(|(name, value)| (self.spelling.canonical(name), value.as_os_str()))
            .collect();
        kept.sort();
        kept
    }
}
