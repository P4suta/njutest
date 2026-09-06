// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Says what the library decides about the number it is given.

fn main() {
    let said = std::env::args().nth(1).unwrap_or_default();
    let n = said.parse::<i32>().unwrap_or_default();
    println!("{}", fixture_subprocess::decide(n));
}
