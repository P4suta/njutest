// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One file two members compile, each into a crate of its own.

/// Whether `n` is within the inclusive bound.
pub fn within(n: i32, bound: i32) -> bool {
    n <= bound
}

/// `n`, brought up to the floor.
pub fn at_least(n: i32, floor: i32) -> i32 {
    if n < floor { floor } else { n }
}
