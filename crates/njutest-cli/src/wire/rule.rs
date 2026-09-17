// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything a run may ask of one exchange, as a set the compiler closes.

use serde::{Deserialize, Serialize};

/// One thing a run may ask of one exchange on one seam.
///
/// A closed set rather than a name, because the catalogue that proposes a
/// question and the interposer that puts it are two lists that would otherwise
/// drift: a name added to one and not the other used to leave a question that
/// went past untouched while the report said it had been put. The fallback
/// that caught it is gone, because the state it caught cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Rule {
    /// Cut the answer short, as a connection that died mid-body does.
    TruncateResponse,
    /// Hold the answer back past what the caller waits for.
    DelayResponse,
    /// Drop the connection with nothing said back.
    DropConnection,
    /// Deliver the same request twice, as a retry after a lost answer does.
    ReplayRequest,
    /// Answer with what the exchange before it got, as a replica behind the writer does.
    StaleResponse,
    /// Answer 500 where the upstream answered otherwise.
    StatusServerError,
    /// Answer 404 where the upstream answered otherwise.
    StatusNotFound,
}

impl Rule {
    /// Every question there is.
    pub const ALL: [Self; 7] = [
        Self::TruncateResponse,
        Self::DelayResponse,
        Self::DropConnection,
        Self::ReplayRequest,
        Self::StaleResponse,
        Self::StatusServerError,
        Self::StatusNotFound,
    ];

    /// What any exchange licenses, because putting it needs nothing read.
    pub const UNPARSED: [Self; 4] = [
        Self::TruncateResponse,
        Self::DelayResponse,
        Self::DropConnection,
        Self::ReplayRequest,
    ];

    /// What an exchange something came before on the same seam licenses.
    pub const PRECEDED: [Self; 1] = [Self::StaleResponse];

    /// What an exchange whose answer was read licenses.
    pub const PARSED: [Self; 2] = [Self::StatusServerError, Self::StatusNotFound];

    /// The name a recording and a report record.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::TruncateResponse => "truncate-response",
            Self::DelayResponse => "delay-response",
            Self::DropConnection => "drop-connection",
            Self::ReplayRequest => "replay-request",
            Self::StaleResponse => "stale-response",
            Self::StatusServerError => "status-server-error",
            Self::StatusNotFound => "status-not-found",
        }
    }

    /// One sentence saying what a run would do, which is what a reader is told.
    #[must_use]
    pub const fn asks(self) -> &'static str {
        match self {
            Self::TruncateResponse => {
                "cut the answer short, as a connection that died mid-body does"
            }
            Self::DelayResponse => "hold the answer back past what the caller waits for",
            Self::DropConnection => "drop the connection with nothing said back",
            Self::ReplayRequest => {
                "deliver the same request twice, as a retry after a lost answer does"
            }
            Self::StaleResponse => {
                "answer with what the exchange before it got, as a replica behind the writer does"
            }
            Self::StatusServerError => "answer 500 where the upstream answered otherwise",
            Self::StatusNotFound => "answer 404 where the upstream answered otherwise",
        }
    }

    /// The status line this asks for, where it asks for one.
    #[must_use]
    pub const fn restates(self) -> Option<(u16, &'static str)> {
        match self {
            Self::StatusServerError => Some((500, "Internal Server Error")),
            Self::StatusNotFound => Some((404, "Not Found")),
            Self::TruncateResponse
            | Self::DelayResponse
            | Self::DropConnection
            | Self::ReplayRequest
            | Self::StaleResponse => None,
        }
    }

    /// The question of that name, or nothing where it names none.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|one| one.name() == name)
    }
}
