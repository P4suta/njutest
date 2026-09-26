// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The environment a run was given, read the way the platform spells names.

use std::ffi::{OsStr, OsString};

/// How a platform tells two environment variable names apart.
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

    /// The one spelling this rule gives every name it takes as `name`, which is what a digest of an environment reads.
    #[must_use]
    pub fn canonical(self, name: &OsStr) -> OsString {
        match self {
            Self::Exact => name.to_owned(),
            Self::AsciiCaseless => name.to_ascii_uppercase(),
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
}

/// Whether two environment variable names are one name on this platform.
#[must_use]
pub fn same_name(one: &OsStr, other: &OsStr) -> bool {
    Spelling::HOST.same(one, other)
}

/// An environment, each variable in it once however its name was spelled, read and changed only as its rule reads names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Variables {
    spelling: Spelling,
    held: Vec<(OsString, OsString)>,
}

impl Variables {
    /// No variables, under `spelling`.
    #[must_use]
    pub const fn none(spelling: Spelling) -> Self {
        Self {
            spelling,
            held: Vec::new(),
        }
    }

    /// `pairs` under `spelling`, a later spelling of a name replacing an earlier one as it does for a process started with them.
    #[must_use]
    pub fn spelled(
        spelling: Spelling,
        pairs: impl IntoIterator<Item = (OsString, OsString)>,
    ) -> Self {
        let mut variables = Self::none(spelling);
        for (name, value) in pairs {
            variables.set(name, value);
        }
        variables
    }

    /// `pairs` as this platform reads them.
    #[must_use]
    pub fn of(pairs: impl IntoIterator<Item = (OsString, OsString)>) -> Self {
        Self::spelled(Spelling::HOST, pairs)
    }

    /// The rule these variables are read under.
    #[must_use]
    pub const fn spelling(&self) -> Spelling {
        self.spelling
    }

    /// The value of `name`, if these variables hold it.
    #[must_use]
    pub fn var(&self, name: &str) -> Option<&OsStr> {
        self.var_os(OsStr::new(name))
    }

    /// The value of `name`, if these variables hold it.
    #[must_use]
    pub fn var_os(&self, name: &OsStr) -> Option<&OsStr> {
        self.held
            .iter()
            .find(|(held, _value)| self.spelling.same(held, name))
            .map(|(_held, value)| value.as_os_str())
    }

    /// Whether these variables hold `name` at all.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.var(name).is_some()
    }

    /// The search path a bare program name is resolved on, if these variables name one.
    #[must_use]
    pub fn search_path(&self) -> Option<&OsStr> {
        self.var("PATH")
    }

    /// Gives `name` the value `value`, replacing every spelling of it these variables held.
    pub fn set(&mut self, name: impl Into<OsString>, value: impl Into<OsString>) {
        let name = name.into();
        self.remove_os(&name);
        self.held.push((name, value.into()));
    }

    /// Takes `name` out, however it was spelled.
    pub fn remove(&mut self, name: &str) {
        self.remove_os(OsStr::new(name));
    }

    /// Takes `name` out, however it was spelled.
    pub fn remove_os(&mut self, name: &OsStr) {
        let spelling = self.spelling;
        self.held
            .retain(|(held, _value)| !spelling.same(held, name));
    }

    /// Only the variables `keep` names, however each was spelled.
    #[must_use]
    pub fn only(&self, keep: &[&str]) -> Self {
        let spelling = self.spelling;
        Self {
            spelling,
            held: self
                .held
                .iter()
                .filter(|(held, _value)| {
                    keep.iter()
                        .any(|kept| spelling.same(held, OsStr::new(kept)))
                })
                .cloned()
                .collect(),
        }
    }

    /// Every variable whose name begins with `prefix` under this rule, by its canonical name, in name order.
    #[must_use]
    pub fn prefixed(&self, prefix: &str) -> Vec<(OsString, &OsStr)> {
        self.canonical()
            .into_iter()
            .filter(|(name, _value)| self.spelling.begins(name, prefix))
            .collect()
    }

    /// Every variable by the one spelling the rule gives its name, in name order, so two spellings of one environment digest alike.
    #[must_use]
    pub fn canonical(&self) -> Vec<(OsString, &OsStr)> {
        let mut canonical: Vec<(OsString, &OsStr)> = self
            .held
            .iter()
            .map(|(name, value)| (self.spelling.canonical(name), value.as_os_str()))
            .collect();
        canonical.sort();
        canonical
    }

    /// Each variable as a process is handed it, which is the one way a name leaves this type.
    pub fn for_process(&self) -> impl Iterator<Item = (&OsStr, &OsStr)> {
        self.held
            .iter()
            .map(|(name, value)| (name.as_os_str(), value.as_os_str()))
    }

    /// How many variables these are.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.held.len()
    }

    /// Whether there are none.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.held.is_empty()
    }
}

/// The value `vars` gives `name`, if it has one.
#[must_use]
pub fn var<'a>(vars: &'a [(OsString, OsString)], name: &str) -> Option<&'a OsStr> {
    let wanted = OsStr::new(name);
    vars.iter()
        .find(|(key, _value)| same_name(key, wanted))
        .map(|(_key, value)| value.as_os_str())
}

/// The search path a bare program name is resolved on, if `vars` names one.
#[must_use]
pub fn search_path(vars: &[(OsString, OsString)]) -> Option<OsString> {
    var(vars, "PATH").map(OsStr::to_owned)
}
