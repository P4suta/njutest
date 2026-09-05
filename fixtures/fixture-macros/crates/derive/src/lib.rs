// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A proc-macro crate: it runs inside the compiler, so nothing in it is a mutant a test could run. Its four candidates are counted as `proc-macro-crate` skips.

use proc_macro::TokenStream;

/// Expands to nothing.
#[proc_macro_derive(Noop)]
pub fn noop(input: TokenStream) -> TokenStream {
    let n = 1 + 1;
    if n > 1 { TokenStream::new() } else { input }
}
