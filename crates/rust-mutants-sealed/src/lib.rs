// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A deterministic WASI preview1 host and a wasmtime runner, so one guest invocation is a pure function of content-addressed inputs.

#![forbid(unsafe_code)]
#![expect(
    clippy::redundant_pub_crate,
    reason = "the modules are private and share items with one another; `pub` in their place would be refused by `unreachable_pub`, which the workspace denies"
)]

mod abi;
mod digest;
mod error;
mod host;
mod imports;
mod interrupt;
mod invocation;
mod random;
mod redirect;
mod runner;
mod snapshot;
mod spelling;
mod strictjson;
mod transcript;
mod transcripts;
mod validate;

pub use abi::Errno;
pub use digest::SealedDigest;
pub use error::{
    EntryFault, EnvironmentFault, ErrorCode, ImportFault, Invariant, MemoryFault, PreopenFault,
    RuntimeStep, SealedCode, SealedError, SnapshotFault, WorkingFault, error_codes,
};
pub use imports::{IMPORT_MODULE, WasiFunction};
pub use interrupt::Interrupt;
pub use invocation::{
    Arguments, ClockPolicy, Environment, Invocation, Limits, Preopen, Preopens, START_LINK_ARGS,
    Start,
};
pub use redirect::{Redirect, Redirected, names_std_env, redirected};
pub use runner::{CompilerTier, ModuleOwner, SealedModule, SealedRunner, WASMTIME_VERSION};
pub use snapshot::{Snapshot, SnapshotBuilder};
pub use transcript::{
    Captured, Denials, OverlayEntry, OverlayState, Refusal, RefusalReason, SealedStop, Transcript,
    TrapKind,
};
pub use transcripts::{
    Compilation, Counted, LAYOUT as TRANSCRIPTS_LAYOUT, Reuse, SCHEMA as TRANSCRIPTS_SCHEMA, Spent,
    Transcripts,
};
