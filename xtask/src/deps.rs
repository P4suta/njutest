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
pub const RULE: &str = "The allowed direction is: njutest -> rust-mutants, \
    rust-mutants-cli -> rust-mutants; every crate may use the dependency-free compiler declarations \
    in njutest-macros and may dev-depend on njutest-devkit. Apart from that, xtask depends on no \
    workspace crate. compiler-surfaces may depend on the engine only to compile the two incidental \
    CLIs as private modules. Nothing else, in particular nothing from the engine towards the runner.";

const ALLOWED_NORMAL: [(&str, &str); 3] = [
    ("njutest", "rust-mutants"),
    ("rust-mutants-cli", "rust-mutants"),
    ("compiler-surfaces", "rust-mutants"),
];

/// Dependencies whose API turns typed failures into an open, downcast-based bag.
const ERASED_ERRORS: [&str; 4] = ["anyhow", "eyre", "color-eyre", "miette"];

/// Procedural macros that can manufacture an owned trait object after the repository's source gate has inspected the unexpanded input.
const OWNED_DYN_GENERATORS: [&str; 3] = ["async-trait", "async-recursion", "typetag"];

fn allowed(edge: &Edge) -> bool {
    if edge.to == "njutest-macros" {
        return true;
    }
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

/// Every direct dependency whose API or expansion defeats a repository type invariant.
///
/// Dependency names are canonical package names from Cargo metadata, so renaming one in a manifest does not hide it from this gate.
#[must_use]
pub fn prohibited_direct_dependencies<'a>(
    dependencies: impl IntoIterator<Item = (&'a str, &'a str)>,
) -> Vec<String> {
    let mut refused: Vec<String> = dependencies
        .into_iter()
        .filter_map(|(package, dependency)| {
            if ERASED_ERRORS.contains(&dependency) {
                Some(format!(
                    "{package} depends on {dependency}, which erases error variants behind downcasts"
                ))
            } else if OWNED_DYN_GENERATORS.contains(&dependency) {
                Some(format!(
                    "{package} depends on {dependency}, which generates owned trait objects after source inspection"
                ))
            } else {
                None
            }
        })
        .collect();
    refused.sort();
    refused.dedup();
    refused
}
