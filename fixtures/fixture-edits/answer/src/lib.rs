// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A proc-macro crate whose answer is decided while the library that uses it is compiled.

use proc_macro::TokenStream;

/// The answer, as a literal.
#[proc_macro]
pub fn answer(_input: TokenStream) -> TokenStream {
    "42".parse().expect("a literal is a token stream")
}
