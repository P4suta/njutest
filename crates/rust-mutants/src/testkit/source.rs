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
/// The first place the text stops reading, with what the parser said there, or why it could not be read at all.
pub fn read_through(text: &str, module: &str) -> Result<(), crate::parsing::ReadingError> {
    crate::parsing::apart(|parsing| crate::instrument::read_through(parsing, text, module))?
}

/// How many bytes of source discovery read back to hold the operator swaps of `source`, or nothing once that stopped fitting.
///
/// # Errors
/// The source is not a file discovery reads.
pub fn read_back(source: &str) -> Result<Option<usize>, crate::syntax::SyntaxError> {
    let registry = crate::rule::Registry::canonical();
    let selection = crate::syntax::Selection::tier(&registry, crate::rule::Tier::All);
    crate::parsing::apart(|parsing| {
        crate::syntax::discover_counting(parsing, "src/lib.rs", source.as_bytes(), &selection)
    })
    .map_err(|unread| crate::syntax::SyntaxError::Unread {
        path: "src/lib.rs".to_owned(),
        source: unread,
    })?
    .map(|(_, read)| read)
}

/// How many items of `source` read alone as its file reads them, and the bytes of each that does not, past any byte order mark or shebang.
///
/// # Errors
/// The source does not parse, or could not be read at all.
pub fn items_read_alone(
    source: &str,
) -> Result<(usize, Vec<std::ops::Range<usize>>), crate::syntax::SyntaxError> {
    crate::syntax::items_read_alone(source)
}

/// `source` discovered on a reading thread that may spend only `ceiling` bytes of its locations, which is how a law reaches the refusal a file too large to read would meet.
///
/// # Errors
/// What discovery says, the refusal to read past the ceiling among it.
pub fn discover_within(
    ceiling: usize,
    source: &str,
) -> Result<crate::syntax::FileDiscovery, crate::syntax::SyntaxError> {
    let registry = crate::rule::Registry::canonical();
    let selection = crate::syntax::Selection::tier(&registry, crate::rule::Tier::All);
    crate::parsing::apart_within(ceiling, |parsing| {
        crate::syntax::discover_counting(parsing, "src/lib.rs", source.as_bytes(), &selection)
    })
    .map_err(|unread| crate::syntax::SyntaxError::Unread {
        path: "src/lib.rs".to_owned(),
        source: unread,
    })?
    .map(|(discovery, _)| discovery)
}
