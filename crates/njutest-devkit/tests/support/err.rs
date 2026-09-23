// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[track_caller]
#[expect(
    clippy::panic,
    reason = "this is the integration tests' single negative-result assertion boundary"
)]
fn test_err<T: std::fmt::Debug, E>(result: Result<T, E>, context: &str) -> E {
    match result {
        Ok(value) => panic!("{context}: unexpectedly succeeded with {value:?}"),
        Err(error) => error,
    }
}
