// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Prints twice its argument.

fn main() {
    let n: u32 = std::env::args()
        .nth(1)
        .and_then(|text| text.parse().ok())
        .unwrap_or_default();
    println!("{}", fixture_cleared_under_mutant::doubled(n));
}
