// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compile the engine CLI's public-looking implementation as private binary code.

#![forbid(unsafe_code)]
#![expect(
    unreachable_pub,
    reason = "making the product module private is the point of this compiler surface; dead_code then rejects public-looking apparatus with no production caller"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate visibility is meaningful in the product crate; including that crate privately is what lets this harness ask which of those names production never reaches"
)]

extern crate self as rust_mutants_cli;

#[path = "../../../crates/rust-mutants-cli/src/lib.rs"]
mod product;
pub(crate) use product::Streams;
pub(crate) use product::{
    Composition, EXIT_USAGE, Environment, app, cli, config, diagnostics, ended, error, exit_codes,
    filesystem, interruptible, kept, outcomes, report, run, run_from_compiled, settings, stream,
    strictjson, text, tui, ui,
};

#[path = "../../../crates/rust-mutants-cli/src/bin/cargo-rust-mutants/main.rs"]
mod cargo_alias;
#[path = "../../../crates/rust-mutants-cli/src/main.rs"]
mod primary;

fn main() {
    let composition_roots: [fn() -> std::process::ExitCode; 2] = [primary::main, cargo_alias::main];
    std::hint::black_box(composition_roots);
}
