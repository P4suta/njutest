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
//!
//! ```no_run
//! use rust_mutants::runner::Cancel;
//! use rust_mutants::session::{PrepareOptions, Request};
//! use rust_mutants::workspace::{OpenOptions, Workspace};
//!
//! # fn main() -> Result<(), rust_mutants::EngineError> {
//! let cancel = Cancel::new();
//! // Nothing below reads the process environment: what a run sees is what
//! // its caller passed.
//! let workspace = Workspace::open(
//!     std::path::Path::new("."),
//!     OpenOptions {
//!         env: std::env::vars_os().collect(),
//!         temp_directory: std::env::temp_dir(),
//!         locked: true,
//!         offline: true,
//!         ..OpenOptions::default()
//!     },
//!     &cancel,
//! )?;
//!
//! // `prepare` consumes the workspace, so a mutant cannot be executed
//! // against a tree that was never instrumented.
//! let session = workspace.prepare(&PrepareOptions::default(), &cancel)?;
//! for mutant in session.catalog().mutants() {
//!     println!("{}  {}", mutant.display_id, mutant.candidate.rule);
//! }
//! for rejection in session.rejections() {
//!     println!("{} refused: {}", rejection.display_id, rejection.diagnostic);
//! }
//!
//! if let Some(first) = session.accepted().first() {
//!     let mutant = session.catalog().by_index(*first).expect("an accepted mutant");
//!     let result = session.exec(
//!         &Request {
//!             mutant: mutant.id.clone(),
//!             ..Request::default()
//!         },
//!         &cancel,
//!     )?;
//!     println!("{} {}", mutant.display_id, result.outcome.name());
//! }
//!
//! // Anything a test wrote into the tree every later mutant is measured
//! // against.
//! assert!(session.changes()?.is_empty());
//! session.close()?;
//! # Ok(())
//! # }
//! ```

// `deny`, not `forbid`: the process supervisor and the advisory lock need
// Windows FFI in two modules, each carrying its own `#[expect(unsafe_code)]`
// with the reason. Every other module is unsafe-free.
#![deny(unsafe_code)]

pub mod cargo;
pub mod catalog;
pub mod discover;
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
