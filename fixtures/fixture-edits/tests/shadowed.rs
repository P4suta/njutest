// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drops a `Gamma` and never calls `untouched`.

#[test]
fn a_gamma_can_be_dropped() {
    let gamma = edits::shadowed::Gamma;
    drop(gamma);
}
