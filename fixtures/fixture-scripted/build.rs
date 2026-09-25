// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Puts the answer the data file holds in the compiler's environment, and says it depends on that file alone.

fn main() {
    println!("cargo::rerun-if-changed=answer.txt");
    let answer = std::fs::read_to_string("answer.txt").expect("the data file");
    println!("cargo::rustc-env=FIXTURE_ANSWER={}", answer.trim());
}
