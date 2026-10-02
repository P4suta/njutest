// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rust text lexed by syn on the calling thread: the one door the repository's gates have to syn's own readers, which they may use because one run reads the tree once and ends (ADR 0045).

#![expect(
    clippy::disallowed_methods,
    reason = "this module is the one door the repository's gates have to syn's readers, which the workspace refuses everywhere else"
)]

/// `text` read as a file, as syn reads one, on the calling thread.
///
/// # Errors
/// The text is not a Rust file.
pub fn file(text: &str) -> Result<syn::File, syn::Error> {
    syn::parse_file(text)
}

/// `text` read as one `T`, all of it, on the calling thread.
///
/// # Errors
/// The text is not one `T`.
pub fn parse<T: syn::parse::Parse>(text: &str) -> Result<T, syn::Error> {
    syn::parse_str(text)
}
