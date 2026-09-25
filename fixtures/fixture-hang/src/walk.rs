// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A loop the run does not mutate, which only a stride of zero keeps going.

// rust-mutants: skip the loop is where a mutation elsewhere never ends, not a site of its own
/// How many strides of `stride` it takes to walk from zero to `n`.
pub(crate) fn strides(n: u32, stride: u32) -> u32 {
    let mut at = 0;
    let mut taken = 0;
    while at < n {
        at += stride;
        taken += 1;
    }
    taken
}
