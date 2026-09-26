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

/// Whether an instrumented `text` reads as Rust down to what every identity macro of the runtime module `module` holds, which a plain parse of the file never looks inside.
///
/// # Errors
/// The first place the text stops reading, with what the parser said there.
pub fn read_through(text: &str, module: &str) -> Result<(), syn::Error> {
    crate::instrument::read_through(text, module)
}
