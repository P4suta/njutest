// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Infallible construction of owned presentation text.

use std::fmt::Arguments;

/// Appends formatted text through [`String`]'s infallible growth API.
///
/// Output to an external stream must instead preserve its `io::Result` all the
/// way to the composition root.
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
