// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Source text a test needs in a shape it does not want to keep a copy of.

/// The same source with every line ending written the other way.
///
/// A CRLF variant is derived rather than committed. A committed one is a
/// second copy of the same program that an editor, a checkout, or a careless
/// rewrite can quietly change, and then the test proves the two files agree
/// rather than that the engine handles both endings. Derived, the variant is
/// the original by construction and the only thing left to prove is the
/// engine's part.
#[must_use]
pub fn crlf(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// The same source with every line ending written the usual way.
#[must_use]
pub fn lf(source: &str) -> String {
    source.replace("\r\n", "\n")
}
