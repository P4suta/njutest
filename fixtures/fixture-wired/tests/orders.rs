// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one test that acts on what the orders service said.

#[test]
fn placing_an_order_is_told_what_it_was_called() {
    let base = std::env::var("ORDERS_URL").unwrap_or_default();
    assert_eq!(
        fixture_wired::place(&base).as_deref(),
        Ok("order-1"),
        "the order was placed and the caller was told which one it is"
    );
}
