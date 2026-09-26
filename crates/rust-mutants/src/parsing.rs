// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where Rust text becomes tokens: on a thread of its own, so every location proc-macro2 records for it ends with that thread.

use std::cell::Cell;
use std::marker::PhantomData;

/// How much of its location space one reading thread may spend: half of what proc-macro2's 32-bit locations address.
const CEILING: usize = 1 << 31;

/// What one byte of text costs a thread's location space, covering the literals syn lexes a second time and the gap left between texts.
const CHARGE: usize = 3;

/// The stack a reading thread runs on, which a long chain of operators needs through the walk, the clone and the drop: reserved rather than committed, so only what a deep file uses is paid for.
pub const STACK: usize = 64 << 20;

thread_local! {
    /// What this thread has spent of its location space and may spend, or nothing where it is not a reading thread.
    static BUDGET: Cell<Option<Budget>> = const { Cell::new(None) };
}

/// What a reading thread has spent of its location space, and what it may.
#[derive(Debug, Clone, Copy)]
struct Budget {
    spent: usize,
    ceiling: usize,
}

/// The right to read Rust text into tokens, which only [`apart`] hands out, on the thread whose locations end with it.
#[derive(Debug)]
pub struct Parsing {
    on_this_thread: PhantomData<*const ()>,
    #[expect(
        dead_code,
        reason = "held for what it prevents: a right that cannot be copied is lent by reference, and every lending is visible"
    )]
    lent: Lent,
}

/// What makes the right one that is lent rather than copied.
#[derive(Debug)]
struct Lent;

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
    /// The reading thread panicked.
    #[error("the thread that reads Rust source panicked")]
    ThreadPanicked,
    /// Text was read on a thread no budget was set for, which only a defect in this module can cause.
    #[error("Rust source was read on a thread that is not a reading thread")]
    Unbudgeted,
}

impl ReadingError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> crate::error::ErrorCode {
        match self {
            Self::Syntax { .. } => crate::error::READING_SYNTAX,
            Self::Exhausted { .. } => crate::error::READING_EXHAUSTED,
            Self::ThreadUnavailable { .. } | Self::ThreadPanicked | Self::Unbudgeted => {
                crate::error::READING_THREAD
            }
        }
    }

    fn of(error: &syn::Error) -> Self {
        Self::at(error.span().start(), error.to_string())
    }

    const fn at(start: proc_macro2::LineColumn, message: String) -> Self {
        Self::Syntax {
            line: start.line,
            column: match start.column.checked_add(1) {
                Some(column) => column,
                None => start.column,
            },
            message,
        }
    }
}

impl Parsing {
    /// `text` read as one `T`, all of it.
    ///
    /// # Errors
    /// The text is not one `T`, or reading it would spend this thread's locations past its ceiling.
    pub fn read<T: syn::parse::Parse>(&self, text: &str) -> Result<T, ReadingError> {
        self.charge(text)?;
        syn::parse_str::<T>(text).map_err(|error| ReadingError::of(&error))
    }

    /// `text` read by `parser`, all of it.
    ///
    /// # Errors
    /// The parser refuses the text, or reading it would spend this thread's locations past its ceiling.
    pub fn read_with<P: syn::parse::Parser>(
        &self,
        parser: P,
        text: &str,
    ) -> Result<P::Output, ReadingError> {
        self.charge(text)?;
        parser
            .parse_str(text)
            .map_err(|error| ReadingError::of(&error))
    }

    /// `text` as a file, with any byte order mark and shebang line taken off first as rustc takes them.
    ///
    /// # Errors
    /// The text is not a file, or reading it would spend this thread's locations past its ceiling.
    pub fn file(&self, text: &str) -> Result<syn::File, ReadingError> {
        self.charge(text)?;
        syn::parse_file(text).map_err(|error| ReadingError::of(&error))
    }

    /// `text` lexed into tokens.
    ///
    /// # Errors
    /// The text does not lex, or reading it would spend this thread's locations past its ceiling.
    pub fn tokens(&self, text: &str) -> Result<proc_macro2::TokenStream, ReadingError> {
        self.charge(text)?;
        text.parse::<proc_macro2::TokenStream>()
            .map_err(|error| ReadingError::at(error.span().start(), error.to_string()))
    }

    /// Charges `text` to this thread's locations before a byte of it is read.
    #[expect(
        clippy::unused_self,
        reason = "the capability is the proof that this thread is a reading thread; charging without one would charge a thread nothing bounds"
    )]
    fn charge(&self, text: &str) -> Result<(), ReadingError> {
        BUDGET.with(|budget| {
            let Some(Budget { spent, ceiling }) = budget.get() else {
                return Err(ReadingError::Unbudgeted);
            };
            let charged = text
                .len()
                .checked_mul(CHARGE)
                .and_then(|cost| cost.checked_add(1));
            match charged.and_then(|cost| spent.checked_add(cost)) {
                Some(after) if after <= ceiling => {
                    budget.set(Some(Budget {
                        spent: after,
                        ceiling,
                    }));
                    Ok(())
                }
                Some(_) | None => Err(ReadingError::Exhausted {
                    spent,
                    asked: text.len(),
                    charged: match charged {
                        Some(cost) => cost,
                        None => text.len(),
                    },
                    ceiling,
                }),
            }
        })
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

    fn join(self) -> Result<T, ReadingError> {
        self.0.join().map_err(|_panic| ReadingError::ThreadPanicked)
    }
}

/// Runs `work` with the right to read Rust text, on a thread of its own whose locations end with it, or on the reading thread it is already on.
///
/// # Errors
/// The thread could not be started or panicked.
pub fn apart<T: Send>(work: impl FnOnce(&Parsing) -> T + Send) -> Result<T, ReadingError> {
    apart_within(CEILING, work)
}

/// [`apart`], with `ceiling` bytes of location space for a new thread to spend.
pub(crate) fn apart_within<T: Send>(
    ceiling: usize,
    work: impl FnOnce(&Parsing) -> T + Send,
) -> Result<T, ReadingError> {
    if BUDGET.with(Cell::get).is_some() {
        return Ok(work(&Parsing {
            on_this_thread: PhantomData,
            lent: Lent,
        }));
    }
    std::thread::scope(|scope| {
        ReadingThread::launch(scope, move || {
            BUDGET.with(|budget| budget.set(Some(Budget { spent: 0, ceiling })));
            work(&Parsing {
                on_this_thread: PhantomData,
                lent: Lent,
            })
        })?
        .join()
    })
}
