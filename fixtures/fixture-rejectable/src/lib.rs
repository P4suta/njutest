// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Three candidates the compiler refuses, and several it accepts. Which is
//! which is a fact about the program, established by compiling it: nothing
//! in the engine decides it in advance.

/// A type with no `Default`, so `return-default` cannot compile here.
#[derive(Debug, PartialEq, Eq)]
pub struct Label(pub &'static str);

/// `String + &str` is addition; `String - &str` is not subtraction.
/// `add-to-sub` is refused with E0369.
pub fn greet(name: String) -> String {
    name + "!"
}

/// A range swap changes the type: `Range<usize>` is not `RangeInclusive`.
/// `range-to-inclusive` is refused with E0308.
pub fn window(n: usize) -> std::ops::Range<usize> {
    0..n
}

/// `value * 0` swapped to `value / 0` compiles: this compiler only refuses
/// a division whose operands it can evaluate, and `value` is a run-time
/// value. The mutant is accepted and dies at run time instead, which is the
/// difference between "the compiler refused it" and "a test caught it".
pub fn erase(value: i32) -> i32 {
    value * 0
}

/// `Label` has no `Default`, so `return-default` is refused with E0277.
pub fn label() -> Label {
    Label("here")
}

/// Every candidate here compiles.
pub fn arithmetic(a: i32, b: i32) -> i32 {
    if a > b { a - b } else { b - a }
}
