// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Macro invocations are skipped whole; a proc-macro crate is skipped as a crate.

use fixture_macros_derive::Noop;

/// Doubles an expression through a declarative macro.
macro_rules! double {
    ($e:expr) => {
        $e * 2
    };
}

/// Carries the derive, which expands to nothing.
#[derive(Noop)]
pub struct Marker;

/// The candidate inside `double!` is invisible; the tail is a return site.
pub fn f(x: i32) -> i32 {
    double!(x + 1)
}

/// A `println!` is one invocation; the arithmetic beside it is visible.
pub fn g(v: &[i32]) -> usize {
    println!("{}", v.len());
    v.len() + 1
}
