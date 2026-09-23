// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Infallible construction of owned presentation text.

use std::fmt::Arguments;

/// Appends formatted text through `String`'s infallible growth API.
///
/// `fmt::Write` exposes a fallible result even when its concrete writer is a `String`.
/// Converting the arguments to owned text first keeps the impossible error out of every renderer instead of teaching callers to discard it.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the compiler-surface harness re-exports this crate-visible boundary"
)]
pub(crate) fn append(output: &mut String, arguments: Arguments<'_>) {
    output.push_str(&arguments.to_string());
}

/// Appends formatted text followed by a newline.
#[expect(
    clippy::redundant_pub_crate,
    reason = "the compiler-surface harness re-exports this crate-visible boundary"
)]
pub(crate) fn line(output: &mut String, arguments: Arguments<'_>) {
    append(output, arguments);
    output.push('\n');
}
