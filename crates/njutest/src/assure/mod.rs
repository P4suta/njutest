// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The phases of one verification, and what coordinates them.

pub mod baseline;
pub mod crashes;
pub mod deep;
pub mod engine;
pub mod equivalence;
pub mod faults;
pub mod fuzz;
pub mod identity;
pub mod knobs;
pub(crate) mod model;
pub mod mutation;
pub mod repair;
pub mod replay;
pub mod route;
pub mod run;
pub mod sanitize;
pub mod schedule;
pub mod sentinel;
pub mod wire;
