// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Dependency direction between the workspace crates.

use std::fmt;

/// Whether an edge is an ordinary (or build) dependency or a dev-dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `[dependencies]` or `[build-dependencies]`.
    Normal,
    /// `[dev-dependencies]`.
    Dev,
}

/// One internal dependency edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The depending crate.
    pub from: String,
    /// The crate it depends on.
    pub to: String,
    /// The kind of dependency.
    pub kind: EdgeKind,
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kind = match self.kind {
            EdgeKind::Normal => "depends on",
            EdgeKind::Dev => "dev-depends on",
        };
        write!(f, "{} {kind} {}", self.from, self.to)
    }
}

/// The rule, for the failure message.
pub const RULE: &str = "The allowed direction is: njutest-cli -> rust-mutants, njutest-cli -> njutest, \
    rust-mutants-cli -> rust-mutants, njutest -> njutest-macros; every crate may dev-depend on \
    njutest-devkit; xtask depends on no workspace crate. Nothing else, in particular nothing from \
    the engine towards the runner.";

const ALLOWED_NORMAL: [(&str, &str); 4] = [
    ("njutest-cli", "rust-mutants"),
    ("njutest-cli", "njutest"),
    ("rust-mutants-cli", "rust-mutants"),
    ("njutest", "njutest-macros"),
];

fn allowed(edge: &Edge) -> bool {
    match edge.kind {
        EdgeKind::Normal => ALLOWED_NORMAL.contains(&(edge.from.as_str(), edge.to.as_str())),
        EdgeKind::Dev => {
            edge.to == "njutest-devkit"
                || edge.from == edge.to
                || ALLOWED_NORMAL.contains(&(edge.from.as_str(), edge.to.as_str()))
        }
    }
}

/// Every edge the rule refuses, in the order given.
#[must_use]
pub fn check(edges: &[Edge]) -> Vec<Edge> {
    edges
        .iter()
        .filter(|edge| !allowed(edge))
        .cloned()
        .collect()
}
