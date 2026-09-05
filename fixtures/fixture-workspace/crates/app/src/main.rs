// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A binary crate: its integration test runs it through `CARGO_BIN_EXE_`.

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let numbers: Vec<i32> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let total = fixture_core::total(&numbers);
    let clamped = fixture_core::clamp(total, 0, 100);
    println!("{clamped}");
    if clamped >= 100 {
        std::process::exit(2);
    }
}
