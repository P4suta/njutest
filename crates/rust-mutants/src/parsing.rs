// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where Rust text becomes tokens: on a thread of its own, so every location proc-macro2 records for it ends with that thread.

use std::cell::Cell;
use std::marker::PhantomData;

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};

/// How much of its location space one reading thread may spend: half of what proc-macro2's 32-bit locations address.
const CEILING: usize = 1 << 31;

/// The most times syn lexes one negative literal again while it parses, to split its sign from its digits: once to peek at it and twice more to take it, where a generic argument holds it.
pub const READ_AGAIN: usize = 3;

/// The stack a reading thread runs on, which a long chain of operators needs through the walk, the clone and the drop: reserved rather than committed, so only what a deep file uses is paid for.
pub const STACK: usize = 64 << 20;

/// The deepest a text's groups may nest for a reading to hand them on: each consumer recurses once per group, and the deepest one costs about 5.3 KiB of [`STACK`] a group in a debug build, so this many take about 5 MiB.
pub const NESTING: usize = 1_000;

/// The most tokens a path through a text's trees may pass for a reading to parse it: each can be a frame of the parser, the walk, the clone and the drop, about 2.7 KiB of [`STACK`] at most in a debug build, so this many take about 33 MiB beside [`NESTING`]'s.
pub const CHAIN: usize = 12_288;

/// The right to read Rust text into tokens, which only [`apart`] hands out, on the thread whose locations end with it.
#[derive(Debug)]
pub struct Parsing {
    /// What the reading thread has spent of its location space.
    spent: Cell<usize>,
    /// What it may spend.
    ceiling: usize,
    on_this_thread: PhantomData<*const ()>,
}

/// Which way a text runs past what a reading's stack holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Past {
    /// Its groups nest past [`NESTING`].
    Nesting,
    /// A path through one of its trees passes more than [`CHAIN`] tokens.
    Chain,
}

impl std::fmt::Display for Past {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Nesting => "how deep its groups nest",
            Self::Chain => "how many tokens a path through one of its trees passes",
        })
    }
}

/// Why Rust text could not be read.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ReadingError {
    /// The text is not Rust of the kind asked for.
    #[error("{line}:{column}: {message}")]
    Syntax {
        /// The 1-based line of the first error, counted in the text given.
        line: usize,
        /// The 1-based character column.
        column: usize,
        /// What the parser said.
        message: String,
    },
    /// Reading the text would take the thread's locations past what they address without wrapping.
    #[error(
        "reading {asked} more bytes would cost {charged} of this thread's locations after \
         {spent}, past {ceiling}; the text is refused rather than read to the wrong places"
    )]
    Exhausted {
        /// What the thread had spent.
        spent: usize,
        /// The length of the text asked for.
        asked: usize,
        /// What that text would cost.
        charged: usize,
        /// What a thread may spend.
        ceiling: usize,
    },
    /// The thread a reading runs on could not be started.
    #[error("the thread that reads Rust source could not be started: {source}")]
    ThreadUnavailable {
        /// What the operating system said.
        source: std::io::Error,
    },
    /// The text runs deeper than a reading thread's stack holds, which reading it would end in an overflow that aborts the process.
    #[error(
        "{past}: {found}, past the {most} a reading's stack holds; the text is refused rather \
         than read into a stack overflow"
    )]
    TooDeep {
        /// Which way it runs too deep.
        past: Past,
        /// How far it runs that way.
        found: usize,
        /// How far a reading reads that way.
        most: usize,
    },
}

/// Where text is not Rust, and what the parser said there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotRust {
    /// The 1-based line of the first error, counted in the text given.
    pub line: usize,
    /// The 1-based character column.
    pub column: usize,
    /// What the parser said.
    pub message: String,
}

impl ReadingError {
    /// Where and why the text is not Rust, or this failure back when the text could not be read at all.
    ///
    /// # Errors
    /// The text could not be read at all, which says nothing about whether it is Rust.
    pub fn syntax(self) -> Result<NotRust, Self> {
        match self {
            Self::Syntax {
                line,
                column,
                message,
            } => Ok(NotRust {
                line,
                column,
                message,
            }),
            unreadable @ (Self::Exhausted { .. }
            | Self::ThreadUnavailable { .. }
            | Self::TooDeep { .. }) => Err(unreadable),
        }
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Syntax { .. } => crate::error::READING_SYNTAX,
            Self::Exhausted { .. } => crate::error::READING_EXHAUSTED,
            Self::ThreadUnavailable { .. } => crate::error::READING_THREAD,
            Self::TooDeep { .. } => crate::error::READING_TOO_DEEP,
        }
    }

    fn of(error: &syn::Error) -> Self {
        Self::at(error.span().start(), error.to_string())
    }

    /// A syntax error at `start`, or the refusal to read past what a location addresses where its column cannot be counted from one.
    fn at(start: proc_macro2::LineColumn, message: String) -> Self {
        match start.column.checked_add(1) {
            Some(column) => Self::Syntax {
                line: start.line,
                column,
                message,
            },
            None => Self::Exhausted {
                spent: start.column,
                asked: 0,
                charged: 0,
                ceiling: CEILING,
            },
        }
    }
}

/// How far a text runs, read from its tokens without recursing: how deep its groups nest, and the most tokens a path through one of its trees passes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Depth {
    /// How many groups the deepest token sits inside.
    pub nesting: usize,
    /// The most tokens a path through the text's trees passes: at every group it enters, each token of the run the group sits in.
    pub chain: usize,
}

impl Depth {
    /// How far `tokens` run, a run ending where a list the parser keeps flat moves on and an attribute counting apart from the run it sits in (ADR 0045).
    #[must_use]
    pub fn of(tokens: &TokenStream) -> Self {
        measured(tokens).depth
    }

    /// Whether a reading that hands on what `built` says reads text this deep.
    ///
    /// # Errors
    /// The way the text runs past what a reading's stack holds.
    const fn admitted(self, built: Built) -> Result<(), ReadingError> {
        if self.nesting > NESTING {
            return Err(ReadingError::TooDeep {
                past: Past::Nesting,
                found: self.nesting,
                most: NESTING,
            });
        }
        match built {
            Built::Tree if self.chain > CHAIN => Err(ReadingError::TooDeep {
                past: Past::Chain,
                found: self.chain,
                most: CHAIN,
            }),
            Built::Tree | Built::Tokens => Ok(()),
        }
    }
}

/// How far a text runs, and how much of its location space syn spends lexing some of it again while it parses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Measure {
    depth: Depth,
    again: usize,
}

/// What one walk over `tokens` finds: how far they run, and every negative number syn will lex again, [`READ_AGAIN`] times at most, at its length with its sign and the position left after it.
fn measured(tokens: &TokenStream) -> Measure {
    let mut root = Level::reading(tokens);
    let mut open: Vec<(Opened, Level)> = Vec::new();
    let (mut nesting, mut again) = (0_usize, 0_usize);
    loop {
        let level = match open.last_mut() {
            Some((_, level)) => level,
            None => &mut root,
        };
        let Some(tree) = level.tokens.next() else {
            let Some((opened, finished)) = open.pop() else {
                return Measure {
                    depth: Depth {
                        nesting,
                        chain: root.chain(),
                    },
                    again,
                };
            };
            let parent = match open.last_mut() {
                Some((_, parent)) => parent,
                None => &mut root,
            };
            parent.closed(opened, finished.chain());
            continue;
        };
        if level.last == Last::ClosedBrace && !continues(&tree) {
            level.end_run();
        }
        match tree {
            TokenTree::Group(group) => {
                let opened = match group.delimiter() {
                    Delimiter::Brace => Opened::Brace,
                    Delimiter::Bracket if matches!(level.last, Last::Hash | Last::HashBang) => {
                        Opened::Attribute
                    }
                    Delimiter::Parenthesis | Delimiter::Bracket | Delimiter::None => Opened::Other,
                };
                open.push((opened, Level::reading(&group.stream())));
                nesting = nesting.max(open.len());
            }
            TokenTree::Punct(punct) => level.punct(&punct),
            TokenTree::Literal(literal) => {
                if level.last == Last::Minus {
                    let spelled = literal.to_string();
                    if spelled.starts_with(|first: char| first.is_ascii_digit()) {
                        again = again.saturating_add(
                            READ_AGAIN.saturating_mul(spelled.len().saturating_add(2)),
                        );
                    }
                }
                level.link();
            }
            TokenTree::Ident(_) => level.link(),
        }
    }
}

/// What a reading hands on, which decides what its text may run deep enough to break.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Built {
    /// Tokens alone, which every consumer walks once per group.
    Tokens,
    /// A tree the parser builds from them, which the parser and everything after it walk once per link of a chain too.
    Tree,
}

/// Whether `tree`, standing right after a closing brace, goes on with what the brace closed rather than starting the next element of a list.
fn continues(tree: &TokenTree) -> bool {
    match tree {
        TokenTree::Group(_) => true,
        TokenTree::Punct(punct) => punct.as_char() != '#',
        TokenTree::Ident(ident) => ident == "as" || ident == "else",
        TokenTree::Literal(_) => false,
    }
}

/// How a group sits among its parent's tokens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Opened {
    /// The brackets of an attribute, which the parser keeps apart from the run around them.
    Attribute,
    /// A brace, after which a list the parser keeps flat may move on.
    Brace,
    /// Any other group, which is one token of the run it sits in.
    Other,
}

/// The last token of a level that bears on what the next one is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    /// None yet, or a separator.
    Nothing,
    /// The `#` that may open an attribute.
    Hash,
    /// The `#!` that may open an inner attribute.
    HashBang,
    /// A group in braces.
    ClosedBrace,
    /// A `-`, which syn reads together with a number right after it.
    Minus,
    /// Anything else.
    Other,
}

/// One group being measured: its tokens, and the runs read of them so far.
struct Level {
    tokens: std::iter::Peekable<proc_macro2::token_stream::IntoIter>,
    last: Last,
    run: usize,
    held: usize,
    longest: usize,
}

impl Level {
    fn reading(tokens: &TokenStream) -> Self {
        Self {
            tokens: tokens.clone().into_iter().peekable(),
            last: Last::Nothing,
            run: 0,
            held: 0,
            longest: 0,
        }
    }

    /// The most tokens a path through this level passes, its current run ended.
    const fn chain(&self) -> usize {
        let current = self.run.saturating_add(self.held);
        if current > self.longest {
            current
        } else {
            self.longest
        }
    }

    const fn end_run(&mut self) {
        self.longest = self.chain();
        self.run = 0;
        self.held = 0;
        self.last = Last::Nothing;
    }

    const fn link(&mut self) {
        self.run = self.run.saturating_add(1);
        self.last = Last::Other;
    }

    fn punct(&mut self, punct: &proc_macro2::Punct) {
        match punct.as_char() {
            ';' | ',' => self.end_run(),
            '=' if punct.spacing() == Spacing::Joint
                && matches!(self.tokens.peek(), Some(TokenTree::Punct(next)) if next.as_char() == '>') =>
            {
                self.tokens.next();
                self.end_run();
            }
            '#' => self.last = Last::Hash,
            '!' if self.last == Last::Hash => self.last = Last::HashBang,
            '-' => {
                self.link();
                self.last = Last::Minus;
            }
            _ => self.link(),
        }
    }

    /// Takes in a group this level holds, which ran `chain` tokens deep and was opened as `opened`.
    const fn closed(&mut self, opened: Opened, chain: usize) {
        if chain > self.held {
            self.held = chain;
        }
        match opened {
            Opened::Attribute => self.last = Last::Other,
            Opened::Brace => {
                self.run = self.run.saturating_add(1);
                self.last = Last::ClosedBrace;
            }
            Opened::Other => self.link(),
        }
    }
}

/// `text` split as `syn::parse_file` splits it: any byte order mark dropped, and a shebang line, which is not Rust, set apart from the Rust after it.
fn shebang_and_rust(text: &str) -> (Option<&str>, &str) {
    let content = match text.strip_prefix('\u{feff}') {
        Some(rest) => rest,
        None => text,
    };
    let Some(after) = content.strip_prefix("#!") else {
        return (None, content);
    };
    if past_whitespace(after).starts_with('[') {
        return (None, content);
    }
    match content.find('\n') {
        Some(newline) => {
            let (shebang, rest) = content.split_at(newline);
            (Some(shebang), rest)
        }
        None => (Some(content), ""),
    }
}

/// `text` past the whitespace and the comments that are not documentation at its start, as syn skips them to tell a shebang from an inner attribute.
fn past_whitespace(text: &str) -> &str {
    let mut rest = text;
    loop {
        if rest.starts_with("//")
            && (!rest.starts_with("///") || rest.starts_with("////"))
            && !rest.starts_with("//!")
        {
            rest = match rest.split_once('\n') {
                Some((_comment, after)) => after,
                None => return "",
            };
            continue;
        }
        if let Some(after) = rest.strip_prefix("/**/") {
            rest = after;
            continue;
        }
        if rest.starts_with("/*")
            && (!rest.starts_with("/**") || rest.starts_with("/***"))
            && !rest.starts_with("/*!")
        {
            match past_block_comment(rest) {
                Some(after) => {
                    rest = after;
                    continue;
                }
                None => return rest,
            }
        }
        match rest.chars().next() {
            Some(ch) if ch.is_whitespace() || ch == '\u{200e}' || ch == '\u{200f}' => {
                rest = rest.get(ch.len_utf8()..).unwrap_or_default();
            }
            Some(_) | None => return rest,
        }
    }
}

/// `text` past the block comment it opens, which nests, or nothing where the comment never closes.
fn past_block_comment(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    let mut depth = 0_usize;
    let mut at = 0_usize;
    while let Some(pair) = bytes.get(at..at.saturating_add(2)) {
        match pair {
            b"/*" => {
                depth = depth.saturating_add(1);
                at = at.saturating_add(2);
            }
            b"*/" => {
                depth = depth.saturating_sub(1);
                at = at.saturating_add(2);
                if depth == 0 {
                    return text.get(at..);
                }
            }
            _ => at = at.saturating_add(1),
        }
    }
    None
}

impl Parsing {
    /// `text` read as one `T`, all of it.
    ///
    /// # Errors
    /// The text is not one `T`, runs deeper than a reading's stack holds, or reading it would spend this thread's locations past its ceiling.
    pub fn read<T: syn::parse::Parse>(&self, text: &str) -> Result<T, ReadingError> {
        let tokens = self.lexed(text, Built::Tree)?;
        syn::parse2::<T>(tokens).map_err(|error| ReadingError::of(&error))
    }

    /// `text` read by `parser`, all of it.
    ///
    /// # Errors
    /// The parser refuses the text, the text runs deeper than a reading's stack holds, or reading it would spend this thread's locations past its ceiling.
    pub fn read_with<P: syn::parse::Parser>(
        &self,
        parser: P,
        text: &str,
    ) -> Result<P::Output, ReadingError> {
        let tokens = self.lexed(text, Built::Tree)?;
        parser
            .parse2(tokens)
            .map_err(|error| ReadingError::of(&error))
    }

    /// `text` as a file, with any byte order mark and shebang line taken off first as rustc takes them.
    ///
    /// # Errors
    /// The text is not a file, runs deeper than a reading's stack holds, or reading it would spend this thread's locations past its ceiling.
    pub fn file(&self, text: &str) -> Result<syn::File, ReadingError> {
        let (shebang, rust) = shebang_and_rust(text);
        let tokens = self.lexed(rust, Built::Tree)?;
        let mut file: syn::File = syn::parse2(tokens).map_err(|error| ReadingError::of(&error))?;
        file.shebang = shebang.map(str::to_owned);
        Ok(file)
    }

    /// `text` lexed into tokens.
    ///
    /// # Errors
    /// The text does not lex, its groups nest deeper than a reading's stack holds, or reading it would spend this thread's locations past its ceiling.
    pub fn tokens(&self, text: &str) -> Result<TokenStream, ReadingError> {
        self.lexed(text, Built::Tokens)
    }

    /// `tokens` this reading lexed, read as one `T`, all of it.
    ///
    /// # Errors
    /// The tokens are not one `T`, run deeper than a reading's stack holds, or what syn lexes of them again would spend this thread's locations past its ceiling.
    pub(crate) fn read_tokens<T: syn::parse::Parse>(
        &self,
        tokens: TokenStream,
    ) -> Result<T, ReadingError> {
        self.read_tokens_with(T::parse, tokens)
    }

    /// `tokens` this reading lexed, read by `parser`, all of them.
    ///
    /// # Errors
    /// The parser refuses the tokens, they run deeper than a reading's stack holds, or what syn lexes of them again would spend this thread's locations past its ceiling.
    pub(crate) fn read_tokens_with<P: syn::parse::Parser>(
        &self,
        parser: P,
        tokens: TokenStream,
    ) -> Result<P::Output, ReadingError> {
        self.admitted(&tokens, Built::Tree)?;
        parser
            .parse2(tokens)
            .map_err(|error| ReadingError::of(&error))
    }

    /// `text` lexed, and measured before anything recurses through it, for a reading that hands on what `built` says.
    fn lexed(&self, text: &str, built: Built) -> Result<TokenStream, ReadingError> {
        self.charge(text.len(), text.len().checked_add(1))?;
        let tokens = text
            .parse::<TokenStream>()
            .map_err(|error| ReadingError::at(error.span().start(), error.to_string()))?;
        self.admitted(&tokens, built)?;
        Ok(tokens)
    }

    /// Whether `tokens` run no deeper than a reading holds, and, where a tree is built of them, the charge for what syn lexes of them again, taken before it does.
    fn admitted(&self, tokens: &TokenStream, built: Built) -> Result<(), ReadingError> {
        let measure = measured(tokens);
        measure.depth.admitted(built)?;
        match built {
            Built::Tree => self.charge(measure.again, Some(measure.again)),
            Built::Tokens => Ok(()),
        }
    }

    /// Charges `cost` of this thread's locations for reading `asked` bytes, before any of them is lexed: exactly what proc-macro2 takes, a position for every character and one left after the text.
    fn charge(&self, asked: usize, cost: Option<usize>) -> Result<(), ReadingError> {
        let spent = self.spent.get();
        match cost.and_then(|cost| spent.checked_add(cost)) {
            Some(after) if after <= self.ceiling => {
                self.spent.set(after);
                Ok(())
            }
            Some(_) | None => Err(ReadingError::Exhausted {
                spent,
                asked,
                charged: match cost {
                    Some(cost) => cost,
                    None => asked,
                },
                ceiling: self.ceiling,
            }),
        }
    }
}

/// The sole owner of the one thread a reading runs on.
/// Consuming `join` is the only way out of the scope, so the thread and every location it recorded have ended before the answer is handed back.
struct ReadingThread<'scope, T>(std::thread::ScopedJoinHandle<'scope, T>);

impl<'scope, T: Send + 'scope> ReadingThread<'scope, T> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        work: impl FnOnce() -> T + Send + 'scope,
    ) -> Result<Self, ReadingError> {
        std::thread::Builder::new()
            .name("rust-mutants-read".to_owned())
            .stack_size(STACK)
            .spawn_scoped(scope, work)
            .map(Self)
            .map_err(|source| ReadingError::ThreadUnavailable { source })
    }

    /// The answer, or the reading's panic raised again here, so a panic in a reading is a panic in its caller under every profile and nothing stands in for it.
    fn join(self) -> T {
        match self.0.join() {
            Ok(answer) => answer,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}

/// Runs `work` with the right to read Rust text, on a thread of its own whose locations end with it.
///
/// # Errors
/// The thread could not be started; a panic in `work` is raised again in the caller.
pub fn apart<T: Send>(work: impl FnOnce(&Parsing) -> T + Send) -> Result<T, ReadingError> {
    apart_within(CEILING, work)
}

/// [`apart`], with `ceiling` bytes of location space for the thread to spend.
pub(crate) fn apart_within<T: Send>(
    ceiling: usize,
    work: impl FnOnce(&Parsing) -> T + Send,
) -> Result<T, ReadingError> {
    std::thread::scope(|scope| {
        ReadingThread::launch(scope, move || {
            work(&Parsing {
                spent: Cell::new(0),
                ceiling,
                on_this_thread: PhantomData,
            })
        })
        .map(ReadingThread::join)
    })
}
