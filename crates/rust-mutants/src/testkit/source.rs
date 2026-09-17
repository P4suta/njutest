// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Source text a test needs in a shape it does not want to keep a copy of.

/// The same source with every line ending written the other way.
#[must_use]
pub fn crlf(source: &str) -> String {
    source.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// The same source with every line ending written the usual way.
#[must_use]
pub fn lf(source: &str) -> String {
    source.replace("\r\n", "\n")
}
