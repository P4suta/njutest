// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test of this crate that needs no toolchain, as one binary rather than one binary per file.

#[path = "cache_sweep.rs"]
mod cache_sweep;
#[path = "cli_contract.rs"]
mod cli_contract;
#[path = "commands_in_process.rs"]
mod commands_in_process;
#[path = "commands_lines.rs"]
mod commands_lines;
#[path = "config.rs"]
mod config;
#[path = "docs_ledger.rs"]
mod docs_ledger;
#[path = "doctor.rs"]
mod doctor;
#[path = "doctor_checks.rs"]
mod doctor_checks;
#[path = "estimate_lines.rs"]
mod estimate_lines;
#[path = "fuzz_seeds.rs"]
mod fuzz_seeds;
#[path = "kept_ledger.rs"]
mod kept_ledger;
#[path = "laws.rs"]
mod laws;
#[path = "lines_around.rs"]
mod lines_around;
#[path = "progress.rs"]
mod progress;
#[path = "projections.rs"]
mod projections;
#[path = "run_lines.rs"]
mod run_lines;
#[path = "run_report.rs"]
mod run_report;
#[path = "sources.rs"]
mod sources;
#[path = "stored_layout.rs"]
mod stored_layout;
#[path = "stored_runs.rs"]
mod stored_runs;
#[path = "trace_reading.rs"]
mod trace_reading;
#[path = "tui.rs"]
mod tui;
