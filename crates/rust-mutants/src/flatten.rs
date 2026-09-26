// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Renders one Rust fragment on a single line, preserving its meaning.

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
    /// The fragment could not be read at all, which says nothing about how it lexes.
    #[error("fragment could not be read: {detail}")]
    Unread {
        /// Why, with the code it carries.
        detail: String,
    },
}

impl FlattenError {
    fn unread(unread: &ReadingError) -> Self {
        Self::Unread {
            detail: format!("{}: {unread}", unread.code().code),
        }
    }
}

/// How a fragment changed when the flattened spelling was tokenized again.
#[derive(Debug, thiserror::Error)]
enum TokenDifferenceError {
    #[error("{expected} tokens re-lexed as {actual}")]
    Count { expected: usize, actual: usize },
    #[error("token {index}: group delimiters differ")]
    Delimiter { index: usize },
    #[error("token {index}: {expected} re-lexed as {actual}")]
    Token {
        index: usize,
        expected: String,
        actual: String,
    },
    #[error("a literal could not be read: {0}")]
    Unread(ReadingError),
}

use proc_macro2::{Delimiter, Literal, TokenStream, TokenTree};

use crate::parsing::{Parsing, ReadingError};

/// Renders `src` on one line.
///
/// # Errors
/// Returns a fragment that does not lex, a literal that cannot be re-spelled,
/// or a postcondition violation.
pub fn flatten(src: &str) -> Result<String, FlattenError> {
    crate::parsing::apart(|parsing| flatten_with(parsing, src))
        .map_err(|unread| FlattenError::unread(&unread))?
}

/// [`flatten`], reading with `parsing` on the thread already reading.
pub(crate) fn flatten_with(parsing: &Parsing, src: &str) -> Result<String, FlattenError> {
    let stream = lex(parsing, src)?;
    let mut leaves = Vec::new();
    collect_leaves(parsing, src, &stream, &mut leaves)?;

    let mut out = String::new();
    let mut previous_end: Option<usize> = None;
    for leaf in &leaves {
        if let Some(end) = previous_end {
            match src.get(end..leaf.start) {
                Some("") if leaf.synthesized(src) => out.push(' '),
                Some("") => {}
                Some(gap) if gap.bytes().all(|byte| byte == b' ' || byte == b'\t') => {
                    out.push_str(gap);
                }
                _ => out.push(' '),
            }
        }
        out.push_str(&leaf.text);
        previous_end = Some(previous_end.map_or(leaf.end, |end| end.max(leaf.end)));
    }

    if let Some(byte) = out.find(['\n', '\r']) {
        return Err(FlattenError::NotFlat { byte });
    }
    let relexed = lex(parsing, &out).map_err(|error| match error {
        FlattenError::Untokenizable { message } => FlattenError::NotIdentical { detail: message },
        other @ (FlattenError::Literal { .. }
        | FlattenError::NotFlat { .. }
        | FlattenError::NotIdentical { .. }
        | FlattenError::Unread { .. }) => other,
    })?;
    same_tokens(parsing, &stream, &relexed).map_err(|error| match error {
        TokenDifferenceError::Unread(unread) => FlattenError::unread(&unread),
        different @ (TokenDifferenceError::Count { .. }
        | TokenDifferenceError::Delimiter { .. }
        | TokenDifferenceError::Token { .. }) => FlattenError::NotIdentical {
            detail: different.to_string(),
        },
    })?;
    Ok(out)
}

fn lex(parsing: &Parsing, src: &str) -> Result<TokenStream, FlattenError> {
    parsing.tokens(src).map_err(|unread| match unread {
        ReadingError::Syntax { message, .. } => FlattenError::Untokenizable { message },
        other @ (ReadingError::Exhausted { .. }
        | ReadingError::ThreadUnavailable { .. }
        | ReadingError::ThreadPanicked
        | ReadingError::Unbudgeted) => FlattenError::unread(&other),
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
    fn synthesized(&self, src: &str) -> bool {
        src.get(self.start..self.end)
            .is_none_or(|text| !text.starts_with(&self.text))
    }
}

fn collect_leaves(
    parsing: &Parsing,
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
                collect_leaves(parsing, src, &group.stream(), leaves)?;
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
                    respell(parsing, &literal)?
                } else {
                    spelled
                };
                leaves.push(leaf_from(src, range, &text));
            }
        }
    }
    Ok(())
}

/// A leaf spelled from its own source bytes when they are that token, and from the token's canonical text otherwise — which is what a token the lexer synthesized from a doc comment gets.
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

/// Re-spells a literal that carries a line break as the escaped literal of the same value: the value is what the compiler sees, CRLF normalized to LF and a backslash continuation folded away.
fn respell(parsing: &Parsing, literal: &Literal) -> Result<String, FlattenError> {
    let spelled = literal.to_string();
    let refuse = || FlattenError::Literal {
        literal: spelled.clone(),
    };
    let parsed: syn::Lit = match parsing.read(&spelled) {
        Ok(parsed) => parsed,
        Err(ReadingError::Syntax { .. }) => return Err(refuse()),
        Err(
            unread @ (ReadingError::Exhausted { .. }
            | ReadingError::ThreadUnavailable { .. }
            | ReadingError::ThreadPanicked
            | ReadingError::Unbudgeted),
        ) => return Err(FlattenError::unread(&unread)),
    };
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

/// Whether two streams are the same tokens: same shape, same identifiers and punctuation, and literals of the same value.
fn same_tokens(
    parsing: &Parsing,
    want: &TokenStream,
    got: &TokenStream,
) -> Result<(), TokenDifferenceError> {
    let want: Vec<TokenTree> = want.clone().into_iter().collect();
    let got: Vec<TokenTree> = got.clone().into_iter().collect();
    if want.len() != got.len() {
        return Err(TokenDifferenceError::Count {
            expected: want.len(),
            actual: got.len(),
        });
    }
    for (index, (a, b)) in want.iter().zip(&got).enumerate() {
        match (a, b) {
            (TokenTree::Group(x), TokenTree::Group(y)) => {
                if x.delimiter() != y.delimiter() {
                    return Err(TokenDifferenceError::Delimiter { index });
                }
                same_tokens(parsing, &x.stream(), &y.stream())?;
            }
            (TokenTree::Ident(x), TokenTree::Ident(y)) if x == y => {}
            (TokenTree::Punct(x), TokenTree::Punct(y)) if x.as_char() == y.as_char() => {}
            (TokenTree::Literal(x), TokenTree::Literal(y))
                if same_literal(parsing, x, y).map_err(TokenDifferenceError::Unread)? => {}
            (a, b) => {
                return Err(TokenDifferenceError::Token {
                    index,
                    expected: a.to_string(),
                    actual: b.to_string(),
                });
            }
        }
    }
    Ok(())
}

fn same_literal(parsing: &Parsing, a: &Literal, b: &Literal) -> Result<bool, ReadingError> {
    let (x, y) = (a.to_string(), b.to_string());
    if x == y {
        return Ok(true);
    }
    let read = |text: &str| match parsing.read::<syn::Lit>(text) {
        Ok(literal) => Ok(Some(literal)),
        Err(ReadingError::Syntax { .. }) => Ok(None),
        Err(unread) => Err(unread),
    };
    Ok(match (read(&x)?, read(&y)?) {
        (Some(syn::Lit::Str(x)), Some(syn::Lit::Str(y))) => {
            x.value().replace("\r\n", "\n") == y.value()
        }
        (Some(syn::Lit::ByteStr(x)), Some(syn::Lit::ByteStr(y))) => {
            normalize_crlf(&x.value()) == y.value()
        }
        (Some(syn::Lit::CStr(x)), Some(syn::Lit::CStr(y))) => {
            normalize_crlf(x.value().to_bytes()) == y.value().to_bytes()
        }
        _ => false,
    })
}
