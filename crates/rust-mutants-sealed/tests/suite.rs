// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test of this crate that needs no toolchain, as one binary rather than one binary per file.

#[path = "common.rs"]
pub mod common;
#[path = "errors_doc.rs"]
mod errors_doc;
#[path = "host.rs"]
mod host;
#[path = "invocation.rs"]
mod invocation;
#[path = "laws.rs"]
mod laws;
#[path = "pins.rs"]
mod pins;
#[path = "redirect.rs"]
mod redirect;
#[path = "snapshot.rs"]
mod snapshot;
#[path = "validation.rs"]
mod validation;
#[path = "working.rs"]
mod working;
