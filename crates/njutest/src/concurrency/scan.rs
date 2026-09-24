// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every place one source file can start a thread, a process, or code the compiler cannot see, read from its syntax and failing closed.

/// What one place in a source can start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, njutest_macros::AllVariants)]
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
    Native,
}

impl Starts {
    /// The name a record gives it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Spawn => "spawn",
            Self::Scope => "scope",
            Self::Parallel => "parallel",
            Self::Runtime => "runtime",
            Self::Native => "native-code",
        }
    }
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
}

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
                into.push(Token::Ident(ident.to_string(), ident.span().start().line));
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

/// Every place in `source`, the file at `path`, that can start a thread, a process, or native code, in source order.
///
/// Read from its tokens, so a macro body and an attribute are read exactly as code is; the parse first holds the file to being Rust.
///
/// # Errors
/// [`ScanError::Unparsable`] when `source` is not Rust this release reads, which is never read as a file that starts nothing.
pub fn scanned(path: &str, source: &str) -> Result<Vec<Found>, ScanError> {
    let refused = |error: &syn::Error| ScanError::Unparsable {
        path: path.to_owned(),
        line: error.span().start().line,
        message: error.to_string(),
    };
    syn::parse_file(source).map_err(|error| refused(&error))?;
    let stream: proc_macro2::TokenStream =
        syn::parse_str(source).map_err(|error| refused(&error))?;
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
