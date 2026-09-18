// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one test that speaks to the health service and asserts nothing about what came back.

#[test]
fn the_health_check_is_made() {
    fixture_wired::ping(&std::env::var("HEALTH_URL").unwrap_or_default());
}
