// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A submodule in its own file.

pub fn sum(xs: &[i32]) -> i32 {
    let mut acc = 0;
    for x in xs {
        acc += *x;
    }
    acc
}
