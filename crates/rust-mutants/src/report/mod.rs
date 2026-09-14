// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a run is, as documents a consumer reads: the catalog it proposed and what became of every mutant in it.
//!
//! The engine writes these and the command line renders them. A reader is
//! lenient — an unknown field is a field from a later release and is ignored
//! — and a writer is strict, which the schemas under `schema/` and the tests
//! that validate against them are what say.

pub mod catalog;
pub mod diff;
pub mod evidence;
pub mod explain;
pub mod run;
pub mod stream;
