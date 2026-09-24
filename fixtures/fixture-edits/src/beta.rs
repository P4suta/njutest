// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A type a test drops, and a function nothing in the tests calls.

/// Something dropped.
pub struct Beta;

/// A function nothing in the tests calls.
pub fn alpha() {}
