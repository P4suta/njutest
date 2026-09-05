// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The probe pass: which mutants a test infected, and what that licenses a run to skip.
//!
//! A kill needs two things to happen: the test has to reach the mutation, and
//! the mutated state has to propagate to something the test asserts. Coverage
//! answers the first. The probe answers the second, for the mutations where it
//! can be answered without running the mutant at all: a `return` replacement
//! whose replacement equals what the function already returns on this input
//! cannot have changed anything the test could see.
//!
//! A test that never infected a mutant cannot have killed it, so it may be
//! discharged from that mutant's reaching set. The evidence is an append-only
//! log the probe runtime writes, and it is read fail-closed: anything that is
//! not exactly the document says nothing at all.

pub mod form;
pub mod log;
pub mod runtime;
