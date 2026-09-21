// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[track_caller]
#[expect(
    clippy::panic,
    reason = "this is the integration tests' single required-value assertion boundary"
)]
fn test_some<T>(value: Option<T>, context: &str) -> T {
    match value {
        Some(value) => value,
        None => panic!("{context}: required value is absent"),
    }
}
