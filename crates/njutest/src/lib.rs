// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Immutable integration-resource metadata for ordinary Rust tests.

#![forbid(unsafe_code)]

pub use njutest_macros::{AllVariants, integration, unit};

/// The execution resources a test declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ScopeKind {
    /// No externally managed capability.
    Unit,
    /// One or more externally managed capabilities.
    Integration,
}

/// Immutable metadata attached to one test.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Scope {
    kind: ScopeKind,
    capabilities: Vec<String>,
}

/// Why a scope declaration was refused.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum InvalidScope {
    /// `integration` was given no capability at all.
    NoCapabilities,
    /// A capability name was empty or whitespace.
    BlankCapability {
        /// The 0-based position of the blank name in the declaration.
        position: usize,
    },
}

impl std::fmt::Display for InvalidScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoCapabilities => f.write_str("integration requires at least one capability"),
            Self::BlankCapability { position } => {
                write!(
                    f,
                    "integration capability {} must not be blank",
                    position.saturating_add(1)
                )
            }
        }
    }
}

impl std::error::Error for InvalidScope {}

impl Scope {
    /// A test that requires no managed resource.
    #[must_use]
    pub const fn unit() -> Self {
        Self {
            kind: ScopeKind::Unit,
            capabilities: Vec::new(),
        }
    }

    /// A test that requires one or more capabilities. Names are trimmed and deduplicated in first-seen order.
    ///
    /// # Errors
    /// Refuses an empty list and a blank name.
    pub fn integration<I, S>(capabilities: I) -> Result<Self, InvalidScope>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut values: Vec<String> = Vec::new();
        let mut declared_any = false;
        for (position, capability) in capabilities.into_iter().enumerate() {
            declared_any = true;
            let name = capability.as_ref().trim();
            if name.is_empty() {
                return Err(InvalidScope::BlankCapability { position });
            }
            if !values.iter().any(|seen| seen == name) {
                values.push(name.to_owned());
            }
        }
        if !declared_any {
            return Err(InvalidScope::NoCapabilities);
        }
        Ok(Self {
            kind: ScopeKind::Integration,
            capabilities: values,
        })
    }

    /// The kind of scope.
    #[must_use]
    pub const fn kind(&self) -> ScopeKind {
        self.kind
    }

    /// Every managed resource this scope declares, in declaration order.
    #[must_use]
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }
}
