// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#[track_caller]
#[expect(
    clippy::panic,
    reason = "non-UTF-8 process output is a typed protocol failure in these tests"
)]
fn test_utf8<'a>(bytes: &'a [u8], context: &str) -> &'a str {
    match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(error) => panic!("{context}: {error}"),
    }
}
