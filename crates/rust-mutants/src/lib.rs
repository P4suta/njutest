// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A mutation testing engine for Rust and Cargo.

#![deny(unsafe_code)]

pub mod apparatus;
pub mod canonical;
pub mod capdir;
pub mod cargo;
pub mod carry;
pub mod catalog;
pub mod count;
pub mod coverage;
pub mod discover;
pub mod duration;
pub mod equivalence;
pub mod error;
pub mod execcost;
pub mod execute;
pub mod flatten;
pub mod git;
pub mod glob;
pub mod id;
pub mod instrument;
pub mod interval;
pub mod killers;
pub mod limitation;
pub mod orphan;
pub mod outcome;
pub mod outcomes;
pub mod probe;
pub mod prove;
pub mod reach;
pub mod reclaim;
pub mod replace;
pub mod report;
pub mod rule;
pub mod run;
pub mod runner;
pub mod select;
pub mod sentinel;
pub mod session;
pub mod skeleton;
pub mod snapshot;
pub mod span;
pub mod splice;
#[doc(hidden)]
pub mod strictjson;
pub mod syntax;
pub mod telling;
pub mod tempowner;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
pub mod touch;
pub mod trace;
pub mod userdirs;
pub mod validate;
pub mod vars;
pub mod work;
pub mod workspace;

pub use error::{EngineError, ErrorCode};

/// The version of this engine, as recorded in every catalog and report.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
