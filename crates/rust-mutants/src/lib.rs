// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutation testing engine for Rust and Cargo.

#![deny(unsafe_code)]

pub mod cargo;
pub mod catalog;
pub mod coverage;
pub mod discover;
pub mod duration;
pub mod equivalence;
pub mod error;
pub mod execute;
pub mod flatten;
pub mod git;
pub mod glob;
pub mod id;
pub mod instrument;
pub mod interval;
pub mod outcome;
pub mod probe;
pub mod prove;
pub mod reach;
pub mod rule;
pub mod runner;
pub mod session;
pub mod snapshot;
pub mod span;
pub mod splice;
pub mod syntax;
pub mod tempowner;
pub mod trace;
pub mod validate;
pub mod workspace;

pub use error::{EngineError, ErrorCode};

/// The version of this engine, as recorded in every catalog and report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
