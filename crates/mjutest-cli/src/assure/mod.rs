// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The phases of one verification, and what coordinates them.
//!
//! Each phase takes what it needs as arguments and answers with what it
//! observed. Nothing here decides a verdict: the phases observe, the report
//! records, and the audit refuses to publish a claim the observations do not
//! support.

pub mod baseline;
