// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Allocation-free decisions of the rust-mutants engine, each a pure function of what it is given.

#![no_std]
#![forbid(unsafe_code)]

pub mod answered;
pub mod claim;
pub mod confinement;
pub mod decline;
pub mod descent;
pub mod evidence;
pub mod group;
pub mod judgement;
pub mod said;
pub mod shape;
pub mod stall;
pub mod step;
pub mod swap;

/// The step machine's own source, which every generated runtime holds as its module `step`, so the runtime spends its allowance by the one definition this crate tests.
pub const STEP_SOURCE: &str = include_str!("step.rs");

#[cfg(test)]
mod tests;

#[cfg(kani)]
mod step_laws;
