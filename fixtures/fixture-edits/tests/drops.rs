// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Drops a `Beta` and never calls `alpha`.

#[test]
fn a_beta_can_be_dropped() {
    let beta = edits::beta::Beta;
    drop(beta);
}
