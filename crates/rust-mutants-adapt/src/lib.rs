// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Pure adapters of the rust-mutants engine: the text it writes and the records it reads, each a function of what it is given.

#![forbid(unsafe_code)]

pub mod claim;
pub mod confinement;
pub mod decline;
pub mod guard;
pub mod swap;
