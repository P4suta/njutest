// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A `#![no_std]` crate: the v1 runtime needs std, so the whole crate is a
//! `no-std-crate` skip with its two candidates counted.

#![no_std]

/// Adds two numbers.
pub fn add(a: u32, b: u32) -> u32 {
    a + b
}
