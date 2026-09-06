// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A proc-macro crate. What it expands to is decided during the build; the helpers it expands with are ordinary functions its own tests run.

use proc_macro::TokenStream;

/// Expands to nothing.
#[proc_macro_derive(Noop)]
pub fn noop(input: TokenStream) -> TokenStream {
    if repeats(2) > 1 {
        TokenStream::new()
    } else {
        input
    }
}

/// How many times the name would be repeated. A `proc-macro` crate exports only its macros, so this is private, which is what most of such a crate is.
fn repeats(n: usize) -> usize {
    if n > 3 { 3 } else { n }
}

#[cfg(test)]
mod tests {
    #[test]
    fn repeats_is_capped_at_three() {
        assert_eq!(super::repeats(2), 2);
        assert_eq!(super::repeats(9), 3);
    }
}
