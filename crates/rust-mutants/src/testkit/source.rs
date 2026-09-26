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

/// How many bytes of source discovery read back to hold the operator swaps of `source`, or nothing once that stopped fitting.
///
/// # Errors
/// The source is not a file discovery reads.
pub fn read_back(source: &str) -> Result<Option<usize>, crate::syntax::SyntaxError> {
    let registry = crate::rule::Registry::canonical();
    crate::syntax::discover_counting(
        "src/lib.rs",
        source.as_bytes(),
        &crate::syntax::Selection::tier(&registry, crate::rule::Tier::All),
    )
    .map(|(_, read)| read)
}

/// How many items of `source` read alone as its file reads them, and the bytes of each that does not, past any byte order mark or shebang.
///
/// # Errors
/// The source does not parse.
pub fn items_read_alone(source: &str) -> Result<(usize, Vec<std::ops::Range<usize>>), syn::Error> {
    crate::syntax::items_read_alone(source)
}
