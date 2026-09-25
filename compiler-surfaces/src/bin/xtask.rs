// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compile the repository tool's public-looking implementation as private binary code.

#![forbid(unsafe_code)]
#![expect(
    unreachable_pub,
    reason = "making the product module private is the point of this compiler surface; dead_code then rejects public-looking apparatus with no production caller"
)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "crate visibility is meaningful in the product crate; including that crate privately is what lets this harness ask which of those names production never reaches"
)]

extern crate self as xtask;

#[path = "../../../xtask/src/lib.rs"]
mod product;
pub(crate) use product::{
    Process, Streams, adrs, deps, devgates, drift, engineaudit, error, fixtures, gates, kaniaudit,
    knobs, lanes, lints, milestones, modelaudit, proofaudit, release, repair, reportdiff, route,
    run_from, sbom, sentinel, shapes, strictjson, surface, wire, work,
};

#[path = "../../../xtask/src/main.rs"]
mod composition_root;

fn main() {
    let composition_root: fn() -> std::process::ExitCode = composition_root::main;
    std::hint::black_box(composition_root);
}
