// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test of this crate that needs no toolchain, as one binary rather than one binary per file.

#[path = "baseline.rs"]
mod baseline;
#[path = "bundle.rs"]
mod bundle;
#[path = "cache.rs"]
mod cache;
#[path = "changed.rs"]
mod changed;
#[path = "checkpoint.rs"]
mod checkpoint;
#[path = "commands_without_a_run.rs"]
mod commands_without_a_run;
#[path = "concurrency_explore.rs"]
mod concurrency_explore;
#[path = "concurrency_proof.rs"]
mod concurrency_proof;
#[path = "concurrency_scan.rs"]
mod concurrency_scan;
#[path = "config.rs"]
mod config;
#[path = "configured.rs"]
mod configured;
#[path = "coverage.rs"]
mod coverage;
#[path = "crash_decided.rs"]
mod crash_decided;
#[path = "deep.rs"]
mod deep;
#[path = "derive.rs"]
mod derive;
#[path = "dialled.rs"]
mod dialled;
#[path = "disposition.rs"]
mod disposition;
#[path = "docs_ledger.rs"]
mod docs_ledger;
#[path = "doctor_probe.rs"]
mod doctor_probe;
#[path = "documented_commands.rs"]
mod documented_commands;
#[path = "drift.rs"]
mod drift;
#[path = "equivalence.rs"]
mod equivalence;
#[path = "errors_doc.rs"]
mod errors_doc;
#[path = "evidence_digest.rs"]
mod evidence_digest;
#[path = "evidence_key.rs"]
mod evidence_key;
#[path = "evidence_store.rs"]
mod evidence_store;
#[path = "evidence_tree.rs"]
mod evidence_tree;
#[path = "fault_decided.rs"]
mod fault_decided;
#[path = "fuzz.rs"]
mod fuzz;
#[path = "fuzz_seeds.rs"]
mod fuzz_seeds;
#[path = "gallery.rs"]
mod gallery;
#[path = "git.rs"]
mod git;
#[path = "guard.rs"]
mod guard;
#[path = "hollow.rs"]
mod hollow;
#[path = "identity.rs"]
mod identity;
#[path = "identity_environment.rs"]
mod identity_environment;
#[path = "interpose.rs"]
mod interpose;
#[path = "item_spec.rs"]
mod item_spec;
#[path = "kept.rs"]
mod kept;
#[path = "knobs.rs"]
mod knobs;
#[path = "laws.rs"]
mod laws;
#[path = "libtest_options.rs"]
mod libtest_options;
#[path = "limitations.rs"]
mod limitations;
#[path = "lsp.rs"]
mod lsp;
#[path = "matrix.rs"]
mod matrix;
#[path = "measured_sources.rs"]
mod measured_sources;
#[path = "moved.rs"]
mod moved;
#[path = "mutation_evidence.rs"]
mod mutation_evidence;
#[path = "next.rs"]
mod next;
#[path = "plan_failures.rs"]
mod plan_failures;
#[path = "presentation.rs"]
mod presentation;
#[path = "projections.rs"]
mod projections;
#[path = "prove.rs"]
mod prove;
#[path = "provider_process.rs"]
mod provider_process;
#[path = "reach_schema.rs"]
mod reach_schema;
#[path = "recording.rs"]
mod recording;
#[path = "remedies.rs"]
mod remedies;
#[path = "repair.rs"]
mod repair;
#[path = "replace.rs"]
mod replace;
#[path = "report.rs"]
mod report;
#[path = "report_json.rs"]
mod report_json;
#[path = "report_lines.rs"]
mod report_lines;
#[path = "report_merge.rs"]
mod report_merge;
#[path = "report_schema_arms.rs"]
mod report_schema_arms;
#[path = "reports_store.rs"]
mod reports_store;
#[path = "resource.rs"]
mod resource;
#[path = "review.rs"]
mod review;
#[path = "route.rs"]
mod route;
#[path = "run_decisions.rs"]
mod run_decisions;
#[path = "run_id.rs"]
mod run_id;
#[path = "rustflags.rs"]
mod rustflags;
#[path = "sanitize.rs"]
mod sanitize;
#[path = "schedule.rs"]
mod schedule;
#[path = "scope.rs"]
mod scope;
#[path = "scratch.rs"]
mod scratch;
#[path = "sentinel.rs"]
mod sentinel;
#[path = "settle.rs"]
mod settle;
#[path = "soundness.rs"]
mod soundness;
#[path = "spec.rs"]
mod spec;
#[path = "trace.rs"]
mod trace;
#[path = "trace_command.rs"]
mod trace_command;
#[path = "ui.rs"]
mod ui;
#[path = "watch.rs"]
mod watch;
#[path = "why.rs"]
mod why;
#[path = "why_cli.rs"]
mod why_cli;
#[path = "why_page.rs"]
mod why_page;
#[path = "wire.rs"]
mod wire;
#[path = "wire_gallery.rs"]
mod wire_gallery;
#[path = "wire_names.rs"]
mod wire_names;
#[path = "wire_phase.rs"]
mod wire_phase;
