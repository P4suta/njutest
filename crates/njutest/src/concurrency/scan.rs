// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every place one source file can start a thread, a process, or code the compiler cannot see, read from its syntax and failing closed.

/// What one place in a source can start.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
    njutest_macros::AllVariants,
)]
#[serde(rename_all = "kebab-case")]
pub enum Starts {
    /// A call or method whose name starts with `spawn`, on any receiver: a thread, a task, a scoped thread, or a process.
    Spawn,
    /// A call whose path ends in `scope`, which is how a scoped thread or task group is opened.
    Scope,
    /// Rayon, crossbeam, a thread pool, or a parallel iterator, named anywhere.
    Parallel,
    /// An attribute or builder that starts an asynchronous runtime, whose workers are threads.
    Runtime,
    /// An `extern` block or a direct thread binding: code the compiler does not see, which can start threads without a token here saying so.
    #[serde(rename = "native-code")]
    Native,
}

/// One place a file can start something, and what.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Found {
    /// The 1-based line.
    pub line: usize,
    /// What it can start.
    pub what: Starts,
    /// The name it was found by, as written.
    pub by: String,
}

/// Why a file could not be scanned.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScanError {
    /// It is not Rust this release reads.
    #[error("{path}:{line}: not Rust this release reads: {message}")]
    Unparsable {
        /// The file.
        path: String,
        /// The 1-based line.
        line: usize,
        /// What the parser said.
        message: String,
    },
    /// Its groups nest deeper than the scan reads, which building and dropping its token tree could not survive on every stack.
    #[error("{path}: groups nest deeper than {limit}, which the scan does not read")]
    TooDeep {
        /// The file.
        path: String,
        /// The deepest nesting the scan reads.
        limit: usize,
    },
}

/// The deepest nesting of groups the scan reads; a deeper file is unread, never parsed.
pub const MAX_DEPTH: usize = 128;

/// One token of a file, flattened so a rule can look at its neighbours: what a group opens with is kept, what is inside it follows.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    /// A name, with its 1-based line.
    Ident(String, usize),
    /// One punctuation character.
    Punct(char),
    /// The start of a group of this delimiter.
    Open(proc_macro2::Delimiter),
    /// Anything else: a literal, or the end of a group.
    Other,
}

fn flattened(stream: proc_macro2::TokenStream, into: &mut Vec<Token>) {
    for tree in stream {
        match tree {
            proc_macro2::TokenTree::Ident(ident) => {
                let line = ident.span().start().line;
                into.push(Token::Ident(
                    syn::ext::IdentExt::unraw(&ident).to_string(),
                    line,
                ));
            }
            proc_macro2::TokenTree::Punct(punct) => into.push(Token::Punct(punct.as_char())),
            proc_macro2::TokenTree::Group(group) => {
                into.push(Token::Open(group.delimiter()));
                flattened(group.stream(), into);
                into.push(Token::Other);
            }
            proc_macro2::TokenTree::Literal(_) => into.push(Token::Other),
        }
    }
}

/// Whether `source` asks the compiler to read another file as code: an `include!` however it is spaced or delimited, or a source that does not read as tokens, which says nothing about what it includes.
#[must_use]
pub fn includes_code(source: &str) -> bool {
    if nesting(source) > MAX_DEPTH {
        return true;
    }
    let Ok(stream) = <proc_macro2::TokenStream as std::str::FromStr>::from_str(source) else {
        return true;
    };
    let mut tokens = Vec::new();
    flattened(stream, &mut tokens);
    tokens
        .windows(2)
        .any(|pair| matches!(pair, [Token::Ident(name, _), Token::Punct('!')] if name == "include"))
}

/// The names that are always something that can start one, wherever they appear.
const PARALLEL: [&str; 6] = [
    "rayon",
    "crossbeam",
    "threadpool",
    "ThreadPool",
    "ThreadPoolBuilder",
    "into_par_iter",
];

/// The crates whose `main` and `test` attributes start a runtime with worker threads.
const RUNTIMES: [&str; 5] = ["tokio", "actix_rt", "actix_web", "async_std", "smol_potat"];

/// What the name at `at` can start, read with its neighbours.
fn classified(tokens: &[Token], at: usize, name: &str) -> Option<Starts> {
    let after_path = |offset: usize| {
        at.checked_sub(offset)
            .and_then(|before| tokens.get(before))
            .is_some_and(|token| *token == Token::Punct(':'))
    };
    let next = tokens.get(at.saturating_add(1));
    if name.starts_with("spawn") {
        return Some(Starts::Spawn);
    }
    if name == "scope"
        && (after_path(1) || matches!(next, Some(Token::Open(proc_macro2::Delimiter::Parenthesis))))
    {
        return Some(Starts::Scope);
    }
    if PARALLEL.contains(&name) || name.starts_with("par_") {
        return Some(Starts::Parallel);
    }
    if name == "new_multi_thread" {
        return Some(Starts::Runtime);
    }
    if (name == "main" || name == "test") && after_path(1) && after_path(2) {
        let crate_name = at.checked_sub(3).and_then(|before| tokens.get(before));
        if let Some(Token::Ident(named, _)) = crate_name
            && RUNTIMES.contains(&named.as_str())
        {
            return Some(Starts::Runtime);
        }
    }
    if name == "pthread_create" {
        return Some(Starts::Native);
    }
    if name == "extern"
        && !matches!(next, Some(Token::Ident(crate_word, _)) if crate_word == "crate")
    {
        return Some(Starts::Native);
    }
    None
}

/// How deep the brackets of `source` nest outside its comments and literals, found in one pass without building anything that recurses.
fn nesting(source: &str) -> usize {
    let bytes = source.as_bytes();
    let (mut at, mut depth, mut deepest) = (0_usize, 0_usize, 0_usize);
    while let Some(&byte) = bytes.get(at) {
        at = match byte {
            b'(' | b'[' | b'{' => {
                depth = depth.saturating_add(1);
                deepest = deepest.max(depth);
                at.saturating_add(1)
            }
            b')' | b']' | b'}' => {
                depth = depth.saturating_sub(1);
                at.saturating_add(1)
            }
            b'/' if bytes.get(at.saturating_add(1)) == Some(&b'/') => {
                past(bytes, at, |rest| rest.first() == Some(&b'\n'))
            }
            b'/' if bytes.get(at.saturating_add(1)) == Some(&b'*') => past_comment(bytes, at),
            b'"' => past_quoted(bytes, at.saturating_add(1), b'"'),
            b'\'' => past_char(bytes, at),
            b'r' | b'b' | b'c' => past_prefixed(bytes, at),
            _ => at.saturating_add(1),
        };
    }
    deepest
}

/// The index just past the first place at or after `at` where `ends` holds, or the end.
fn past(bytes: &[u8], at: usize, ends: impl Fn(&[u8]) -> bool) -> usize {
    let mut at = at;
    while let Some(rest) = bytes.get(at..) {
        if rest.is_empty() || ends(rest) {
            return at.saturating_add(1);
        }
        at = at.saturating_add(1);
    }
    at
}

/// The index just past the block comment opening at `at`, which nests.
fn past_comment(bytes: &[u8], at: usize) -> usize {
    let (mut at, mut open) = (at.saturating_add(2), 1_usize);
    while let Some(pair) = bytes.get(at..at.saturating_add(2)) {
        match pair {
            b"/*" => {
                open = open.saturating_add(1);
                at = at.saturating_add(2);
            }
            b"*/" => {
                open = open.saturating_sub(1);
                at = at.saturating_add(2);
                if open == 0 {
                    return at;
                }
            }
            _ => at = at.saturating_add(1),
        }
    }
    bytes.len()
}

/// The index just past the literal whose body starts at `at` and ends at an unescaped `close`.
fn past_quoted(bytes: &[u8], at: usize, close: u8) -> usize {
    let mut at = at;
    while let Some(&byte) = bytes.get(at) {
        if byte == b'\\' {
            at = at.saturating_add(2);
        } else if byte == close {
            return at.saturating_add(1);
        } else {
            at = at.saturating_add(1);
        }
    }
    bytes.len()
}

/// The index just past the character literal at `at`, or just past the quote where it opens a lifetime.
fn past_char(bytes: &[u8], at: usize) -> usize {
    let body = at.saturating_add(1);
    if bytes.get(body) == Some(&b'\\') {
        return past_quoted(bytes, body, b'\'');
    }
    let width = bytes
        .get(body)
        .map_or(1, |&lead| match lead.leading_ones() {
            2 => 2,
            3 => 3,
            4 => 4,
            _ => 1,
        });
    if bytes.get(body.saturating_add(width)) == Some(&b'\'') {
        return body.saturating_add(width).saturating_add(1);
    }
    body
}

/// The index just past the byte, C or raw string literal at `at`, or just past `at` where the letter starts a name.
fn past_prefixed(bytes: &[u8], at: usize) -> usize {
    if at
        .checked_sub(1)
        .and_then(|before| bytes.get(before))
        .is_some_and(|&before| before.is_ascii_alphanumeric() || before == b'_')
    {
        return at.saturating_add(1);
    }
    let mut next = at.saturating_add(1);
    if matches!(bytes.get(at), Some(b'b' | b'c')) && bytes.get(next) == Some(&b'r') {
        next = next.saturating_add(1);
    }
    let raw = bytes.get(next.saturating_sub(1)) == Some(&b'r');
    let hashes = bytes.get(next..).map_or(0, |rest| {
        rest.iter().take_while(|&&byte| byte == b'#').count()
    });
    let quote = next.saturating_add(hashes);
    match bytes.get(quote) {
        Some(b'"') if raw => {
            let mut closing = vec![b'"'];
            closing.extend(std::iter::repeat_n(b'#', hashes));
            past(bytes, quote.saturating_add(1), |rest| {
                rest.starts_with(&closing)
            })
            .saturating_add(hashes)
        }
        Some(b'"') if hashes == 0 => past_quoted(bytes, quote.saturating_add(1), b'"'),
        Some(b'\'') if hashes == 0 && bytes.get(at) == Some(&b'b') => past_char(bytes, quote),
        _ => at.saturating_add(1),
    }
}

/// Every place in `source`, the file at `path`, that can start a thread, a process, or native code, in source order.
///
/// Read from its tokens, so a macro body and an attribute are read exactly as code is; the lexer first holds the file to being Rust's tokens, and nothing parses it further, since no rule reads more than tokens and a parser recurses through chains no limit here can bound.
///
/// # Errors
/// [`ScanError::Unparsable`] when `source` is not Rust's tokens, and [`ScanError::TooDeep`] when its brackets nest deeper than [`MAX_DEPTH`]; neither is ever read as a file that starts nothing.
pub fn scanned(path: &str, source: &str) -> Result<Vec<Found>, ScanError> {
    if nesting(source) > MAX_DEPTH {
        return Err(ScanError::TooDeep {
            path: path.to_owned(),
            limit: MAX_DEPTH,
        });
    }
    let stream =
        <proc_macro2::TokenStream as std::str::FromStr>::from_str(source).map_err(|error| {
            ScanError::Unparsable {
                path: path.to_owned(),
                line: error.span().start().line,
                message: error.to_string(),
            }
        })?;
    let mut tokens = Vec::new();
    flattened(stream, &mut tokens);
    Ok(tokens
        .iter()
        .enumerate()
        .filter_map(|(at, token)| {
            let Token::Ident(name, line) = token else {
                return None;
            };
            classified(&tokens, at, name).map(|what| Found {
                line: *line,
                what,
                by: name.clone(),
            })
        })
        .collect())
}
