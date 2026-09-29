// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Allocation-free decisions of the rust-mutants engine, each a pure function of what it is given.

#![no_std]
#![forbid(unsafe_code)]

pub mod claim;
pub mod confinement;
pub mod decline;
pub mod evidence;
pub mod judgement;
pub mod shape;
pub mod swap;
