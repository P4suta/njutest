// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Renders one Rust fragment on a single line, preserving its meaning.
//!
//! The guard forms splice a mutated copy of a statement into the same line
//! the original occupies, so the copy has to fit on one line however the
//! author wrote it — a call broken across five lines, a raw string holding a
//! here-document, a comment in the middle of a condition. The
//! line-preservation invariant is what makes this necessary.
//!
//! The mechanism is byte folding checked by re-lexing, not pretty-printing.
//! The fragment is lexed once; every token is reproduced from its own bytes,
//! and the whitespace between two tokens is kept verbatim when it holds no
//! line break and no comment, and becomes one space otherwise. Two kinds of
//! token are re-spelled, each a literal that carries a line break inside
//! itself: a raw string, byte string, or C string spanning lines becomes the
//! escaped literal of the same value, and a string continued across lines
//! with a trailing backslash becomes the same value on one line. Comments are
//! dropped; a doc comment, which the lexer reads as a `#[doc = …]` attribute,
//! is rendered as that attribute.
//!
//! The output contains no `\n` and no `\r`, and re-lexes to the token stream
//! it was rendered from. Both are verified on every call rather than trusted,
//! because a folding bug would otherwise produce a plausible-looking mutant
//! that compiles as a different program.

/// Why a fragment could not be flattened.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum FlattenError {
    /// The fragment does not lex as Rust.
    #[error("fragment does not tokenize: {message}")]
    Untokenizable {
        /// The lexer's message.
        message: String,
    },
    /// A literal carrying a line break could not be re-spelled.
    #[error("literal {literal:?} could not be re-spelled on one line")]
    Literal {
        /// The literal as written.
        literal: String,
    },
    /// The output still holds a line break: a bug in this module.
    #[error("flattened fragment still contains a line break at byte {byte}")]
    NotFlat {
        /// The offset of the line break.
        byte: usize,
    },
    /// The output re-lexes differently from the input: a bug in this module.
    #[error("flattened fragment re-tokenizes differently: {detail}")]
    NotIdentical {
        /// What differed.
        detail: String,
    },
}

use std::str::FromStr as _;

use proc_macro2::{Delimiter, Literal, TokenStream, TokenTree};

/// Renders `src` on one line.
///
/// # Errors
///
/// Returns a fragment that does not lex, a literal that cannot be re-spelled,
/// or a postcondition violation.
pub fn flatten(src: &str) -> Result<String, FlattenError> {
    let stream = lex(src)?;
    let mut leaves = Vec::new();
    collect_leaves(src, &stream, &mut leaves)?;

    let mut out = String::new();
    let mut previous_end: Option<usize> = None;
    for leaf in &leaves {
        if let Some(end) = previous_end {
            match src.get(end..leaf.start) {
                // A doc comment's `#` written where the comment was would
                // touch the token before it and change that token's
                // spacing, which is a different stream.
                Some("") if leaf.synthesized(src) => out.push(' '),
                Some("") => {}
                Some(gap) if gap.bytes().all(|byte| byte == b' ' || byte == b'\t') => {
                    out.push_str(gap);
                }
                // A line break, a comment, or a synthesized token whose span
                // overlaps its neighbour: one space keeps the tokens apart.
                _ => out.push(' '),
            }
        }
        out.push_str(&leaf.text);
        previous_end = Some(previous_end.map_or(leaf.end, |end| end.max(leaf.end)));
    }

    // Postconditions. Both failures are bugs in this module rather than
    // anything the caller did, and both are checked because the cost is one
    // lex of a statement and the cost of not checking is a silently
    // miscompiled mutant.
    if let Some(byte) = out.find(['\n', '\r']) {
        return Err(FlattenError::NotFlat { byte });
    }
    let relexed = lex(&out).map_err(|error| FlattenError::NotIdentical {
        detail: error.to_string(),
    })?;
    same_tokens(&stream, &relexed).map_err(|detail| FlattenError::NotIdentical { detail })?;
    Ok(out)
}

fn lex(src: &str) -> Result<TokenStream, FlattenError> {
    TokenStream::from_str(src).map_err(|error| FlattenError::Untokenizable {
        message: error.to_string(),
    })
}

/// One token as it will be written, with the byte range it came from.
struct Leaf {
    start: usize,
    end: usize,
    text: String,
}

impl Leaf {
    /// Whether the token is not what the source says at that place.
    ///
    /// A doc comment is the case that matters: the lexer expands `/// x`
    /// into `#[doc = " x"]`, so a leaf whose source begins with `/` is
    /// written as `#`. Writing that `#` where the comment was would put it
    /// against the token before it and change that token's spacing, which
    /// is a different stream — so an empty gap before one of these still
    /// becomes a space. A re-spelled string literal is synthesized too,
    /// and costs at most one space nobody can see.
    fn synthesized(&self, src: &str) -> bool {
        src.get(self.start..self.end)
            .is_none_or(|text| !text.starts_with(&self.text))
    }
}

fn collect_leaves(
    src: &str,
    stream: &TokenStream,
    leaves: &mut Vec<Leaf>,
) -> Result<(), FlattenError> {
    for tree in stream.clone() {
        match tree {
            TokenTree::Group(group) => {
                let (open, close) = match group.delimiter() {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    leaves.push(leaf_from(src, group.span_open().byte_range(), open));
                }
                collect_leaves(src, &group.stream(), leaves)?;
                if !close.is_empty() {
                    leaves.push(leaf_from(src, group.span_close().byte_range(), close));
                }
            }
            TokenTree::Ident(ident) => {
                leaves.push(leaf_from(
                    src,
                    ident.span().byte_range(),
                    &ident.to_string(),
                ));
            }
            TokenTree::Punct(punct) => {
                leaves.push(leaf_from(
                    src,
                    punct.span().byte_range(),
                    &punct.as_char().to_string(),
                ));
            }
            TokenTree::Literal(literal) => {
                let range = literal.span().byte_range();
                let spelled = literal.to_string();
                let text = if spelled.contains(['\n', '\r']) {
                    respell(&literal)?
                } else {
                    spelled
                };
                leaves.push(leaf_from(src, range, &text));
            }
        }
    }
    Ok(())
}

/// A leaf spelled from its own source bytes when they are that token, and
/// from the token's canonical text otherwise — which is what a token the
/// lexer synthesized from a doc comment gets.
fn leaf_from(src: &str, range: std::ops::Range<usize>, canonical: &str) -> Leaf {
    let text = match src.get(range.clone()) {
        Some(bytes) if bytes == canonical => bytes.to_owned(),
        _ => canonical.to_owned(),
    };
    Leaf {
        start: range.start,
        end: range.end,
        text,
    }
}

/// Re-spells a literal that carries a line break as the escaped literal of
/// the same value: the value is what the compiler sees, CRLF normalized to
/// LF and a backslash continuation folded away.
fn respell(literal: &Literal) -> Result<String, FlattenError> {
    let spelled = literal.to_string();
    let refuse = || FlattenError::Literal {
        literal: spelled.clone(),
    };
    let parsed: syn::Lit = syn::parse_str(&spelled).map_err(|_error| refuse())?;
    match parsed {
        syn::Lit::Str(text) => Ok(Literal::string(&text.value().replace("\r\n", "\n")).to_string()),
        syn::Lit::ByteStr(bytes) => {
            let value = bytes.value();
            let normalized = normalize_crlf(&value);
            Ok(Literal::byte_string(&normalized).to_string())
        }
        syn::Lit::CStr(text) => {
            let value = text.value();
            let normalized = normalize_crlf(value.to_bytes());
            let c_string = std::ffi::CString::new(normalized).map_err(|_nul| refuse())?;
            Ok(Literal::c_string(&c_string).to_string())
        }
        _ => Err(refuse()),
    }
}

fn normalize_crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().peekable();
    while let Some(&byte) = iter.next() {
        if byte == b'\r' && iter.peek() == Some(&&b'\n') {
            continue;
        }
        out.push(byte);
    }
    out
}

/// Whether two streams are the same tokens: same shape, same identifiers and
/// punctuation, and literals of the same value.
fn same_tokens(want: &TokenStream, got: &TokenStream) -> Result<(), String> {
    let want: Vec<TokenTree> = want.clone().into_iter().collect();
    let got: Vec<TokenTree> = got.clone().into_iter().collect();
    if want.len() != got.len() {
        return Err(format!("{} tokens re-lexed as {}", want.len(), got.len()));
    }
    for (index, (a, b)) in want.iter().zip(&got).enumerate() {
        match (a, b) {
            (TokenTree::Group(x), TokenTree::Group(y)) => {
                if x.delimiter() != y.delimiter() {
                    return Err(format!("token {index}: group delimiters differ"));
                }
                same_tokens(&x.stream(), &y.stream())?;
            }
            (TokenTree::Ident(x), TokenTree::Ident(y)) if x == y => {}
            (TokenTree::Punct(x), TokenTree::Punct(y)) if x.as_char() == y.as_char() => {}
            (TokenTree::Literal(x), TokenTree::Literal(y)) if same_literal(x, y) => {}
            (a, b) => return Err(format!("token {index}: {a} re-lexed as {b}")),
        }
    }
    Ok(())
}

fn same_literal(a: &Literal, b: &Literal) -> bool {
    let (x, y) = (a.to_string(), b.to_string());
    if x == y {
        return true;
    }
    match (
        syn::parse_str::<syn::Lit>(&x),
        syn::parse_str::<syn::Lit>(&y),
    ) {
        (Ok(syn::Lit::Str(x)), Ok(syn::Lit::Str(y))) => {
            x.value().replace("\r\n", "\n") == y.value()
        }
        (Ok(syn::Lit::ByteStr(x)), Ok(syn::Lit::ByteStr(y))) => {
            normalize_crlf(&x.value()) == y.value()
        }
        (Ok(syn::Lit::CStr(x)), Ok(syn::Lit::CStr(y))) => {
            normalize_crlf(x.value().to_bytes()) == y.value().to_bytes()
        }
        _ => false,
    }
}
