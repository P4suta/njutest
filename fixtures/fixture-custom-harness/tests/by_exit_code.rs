// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A test target with `harness = false`: it prints what it likes and says what it found by exiting.

fn main() {
    let checks = [
        ("eleven is above", fixture_custom_harness::above(11)),
        ("ten is not", !fixture_custom_harness::above(10)),
        ("zero is not", !fixture_custom_harness::above(0)),
    ];
    let mut failed = 0;
    for (what, held) in checks {
        if held {
            println!("held: {what}");
        } else {
            println!("broken: {what}");
            failed += 1;
        }
    }
    if failed > 0 {
        std::process::exit(1);
    }
}
