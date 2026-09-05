// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutation testing engine for Rust and Cargo.

#![deny(unsafe_code)]

pub mod cargo;
pub mod catalog;
pub mod discover;
pub mod duration;
pub mod error;
pub mod execute;
pub mod flatten;
pub mod glob;
pub mod id;
pub mod instrument;
pub mod interval;
pub mod outcome;
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
