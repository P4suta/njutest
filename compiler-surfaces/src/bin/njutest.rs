// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compile the runner's public-looking implementation as private binary code.

#![forbid(unsafe_code)]
#![expect(
    unreachable_pub,
    reason = "making the product module private is the point of this compiler surface; dead_code then rejects public-looking apparatus with no production caller"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate visibility is meaningful in the product crate; including that crate privately is what lets this harness ask which of those names production never reaches"
)]

extern crate self as njutest;

#[path = "../../../crates/njutest/src/lib.rs"]
mod product;
pub(crate) use product::{
    VERSION, app, asked, assure, build, cache, checkpoint, cli, config, coverage, ended, error,
    evidence, git, interruptible, kept, limitation, naming, presentation, provider, reach, repair,
    report, resource, run_from, run_id, rustflags, scratch, soundness, strictjson, targets, text,
    trace, ui, watch, why, wire,
};

#[path = "../../../crates/njutest/src/bin/cargo-njutest/main.rs"]
mod cargo_alias;
#[path = "../../../crates/njutest/src/main.rs"]
mod primary;

fn main() {
    let composition_roots: [fn() -> std::process::ExitCode; 2] = [primary::main, cargo_alias::main];
    std::hint::black_box(composition_roots);
}
