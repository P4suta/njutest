// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A benchmark with no harness: something the build produces and no run executes.

fn main() {
    let mut total = 0;
    for n in 0..1000 {
        total += fixture_targets::whole(n);
    }
    println!("{total}");
}
