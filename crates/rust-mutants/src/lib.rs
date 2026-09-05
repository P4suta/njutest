// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutation testing engine for Rust and Cargo.
//!
//! rust-mutants is the fourth engine in a family — ocaml-mutants,
//! gleam-mutants, go-mutants — that shares one architecture: the source
//! workspace is read-only; every compilable mutant of a file lives dormant
//! behind a guard in one disposable snapshot; one environment variable
//! selects the active mutant per test process; mutant identities are stable
//! content hashes; configuration is strict; reports are honest.
//!
//! The library API is what an assurance runner such as `mjutest` drives:
//! open a workspace, prepare a session, execute mutants against test targets,
//! and read the probe measurements the session took.

#![forbid(unsafe_code)]

pub mod catalog;
pub mod error;
pub mod flatten;
pub mod glob;
pub mod id;
pub mod interval;
pub mod outcome;
pub mod rule;
pub mod span;
pub mod splice;

pub use error::{EngineError, ErrorCode};

/// The version of this engine, as recorded in every catalog and report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
