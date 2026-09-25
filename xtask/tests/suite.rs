// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test of this crate that needs no toolchain, as one binary rather than one binary per file.

#![recursion_limit = "256"]

#[path = "deps.rs"]
mod deps;
#[path = "devgates.rs"]
mod devgates;
#[path = "docflows.rs"]
mod docflows;
#[path = "docs.rs"]
mod docs;
#[path = "dogfood.rs"]
mod dogfood;
#[path = "engineaudit.rs"]
mod engineaudit;
#[path = "errors_doc.rs"]
mod errors_doc;
#[path = "fixtures.rs"]
mod fixtures;
#[path = "fuzz_ledger.rs"]
mod fuzz_ledger;
#[path = "fuzzclippy.rs"]
mod fuzzclippy;
#[path = "gates.rs"]
mod gates;
#[path = "kani.rs"]
mod kani;
#[path = "lints.rs"]
mod lints;
#[path = "milestones.rs"]
mod milestones;
#[path = "pre_push.rs"]
mod pre_push;
#[path = "proofaudit.rs"]
mod proofaudit;
#[path = "release.rs"]
mod release;
#[path = "release_binaries.rs"]
mod release_binaries;
#[path = "remote.rs"]
mod remote;
#[path = "reportdiff.rs"]
mod reportdiff;
#[path = "route.rs"]
mod route;
#[path = "sbom.rs"]
mod sbom;
#[path = "sentinel.rs"]
mod sentinel;
#[path = "shapes.rs"]
mod shapes;
#[path = "suites.rs"]
mod suites;
#[path = "surface.rs"]
mod surface;
#[path = "tasks.rs"]
mod tasks;
#[path = "tracked.rs"]
mod tracked;
#[path = "waiver_key.rs"]
mod waiver_key;
#[path = "wire.rs"]
mod wire;
#[path = "workflows.rs"]
mod workflows;
