// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test of this crate that needs no toolchain, as one binary rather than one binary per file.

#[path = "annotate.rs"]
mod annotate;
#[path = "branch.rs"]
mod branch;
#[path = "build_identity.rs"]
mod build_identity;
#[path = "built.rs"]
mod built;
#[path = "canonical.rs"]
mod canonical;
#[path = "capdir.rs"]
mod capdir;
#[path = "cargo.rs"]
mod cargo;
#[path = "cargo_config.rs"]
mod cargo_config;
#[path = "cargo_manifest.rs"]
mod cargo_manifest;
#[path = "carry.rs"]
mod carry;
#[path = "catalog.rs"]
mod catalog;
#[path = "census.rs"]
mod census;
#[path = "count.rs"]
mod count;
#[path = "coverage.rs"]
mod coverage;
#[path = "decline.rs"]
mod decline;
#[path = "devkit_environment.rs"]
mod devkit_environment;
#[path = "docs_ledger.rs"]
mod docs_ledger;
#[path = "duration.rs"]
mod duration;
#[path = "equivalence.rs"]
mod equivalence;
#[path = "errors_doc.rs"]
mod errors_doc;
#[path = "execute.rs"]
mod execute;
#[path = "facts.rs"]
mod facts;
#[path = "flatten.rs"]
mod flatten;
#[path = "forbid.rs"]
mod forbid;
#[path = "git.rs"]
mod git;
#[path = "glob.rs"]
mod glob;
#[path = "id.rs"]
mod id;
#[path = "instrument.rs"]
mod instrument;
#[path = "interval.rs"]
mod interval;
#[path = "literal.rs"]
mod literal;
#[path = "metadata.rs"]
mod metadata;
#[path = "outcome.rs"]
mod outcome;
#[path = "outcomes.rs"]
mod outcomes;
#[path = "outcomes_export.rs"]
mod outcomes_export;
#[path = "outside.rs"]
mod outside;
#[path = "parsing.rs"]
mod parsing;
#[path = "probe_form.rs"]
mod probe_form;
#[path = "prove.rs"]
mod prove;
#[path = "reach.rs"]
mod reach;
#[path = "regroup.rs"]
mod regroup;
#[path = "replace.rs"]
mod replace;
#[path = "route.rs"]
mod route;
#[path = "rule.rs"]
mod rule;
#[path = "run.rs"]
mod run;
#[path = "run_report_strict.rs"]
mod run_report_strict;
#[path = "runner.rs"]
mod runner;
#[path = "sentinel.rs"]
mod sentinel;
#[path = "session.rs"]
mod session;
#[path = "shape.rs"]
mod shape;
#[path = "skeleton.rs"]
mod skeleton;
#[path = "snapshot.rs"]
mod snapshot;
#[path = "snapshot_layout.rs"]
mod snapshot_layout;
#[path = "span.rs"]
mod span;
#[path = "splice.rs"]
mod splice;
#[path = "syntax.rs"]
mod syntax;
#[path = "tempowner.rs"]
mod tempowner;
#[path = "touch.rs"]
mod touch;
#[path = "touch_runtime.rs"]
mod touch_runtime;
#[path = "trace.rs"]
mod trace;
#[path = "userdirs.rs"]
mod userdirs;
#[path = "validate.rs"]
mod validate;
#[path = "vars.rs"]
mod vars;
#[path = "wire_names.rs"]
mod wire_names;
#[path = "witness.rs"]
mod witness;
#[path = "work.rs"]
mod work;
#[path = "workspace.rs"]
mod workspace;
