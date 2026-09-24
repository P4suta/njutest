// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the library decides about the number it is given.

fn main() {
    let given = std::env::args().nth(1).unwrap_or_default();
    let n: i32 = given.parse().unwrap_or_default();
    println!("{}", edits::child::decide(n));
}
