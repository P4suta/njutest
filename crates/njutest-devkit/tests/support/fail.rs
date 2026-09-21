// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[track_caller]
#[expect(
    clippy::panic,
    reason = "this is the integration tests' single impossible-branch assertion boundary"
)]
fn test_fail(message: std::fmt::Arguments<'_>) -> ! {
    panic!("{message}")
}
