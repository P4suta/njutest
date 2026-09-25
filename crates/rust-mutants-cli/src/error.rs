// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the command line itself can fail at, each with a stable code documented in `docs/errors.md`.

use std::path::{Path, PathBuf};

use rust_mutants::EngineError;
use rust_mutants::error::{self, ErrorCode};

use crate::config::ConfigError;

/// Why a command could not do what it was asked.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CliError {
    /// The engine could not do it.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The configuration could not be used.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The premises needed to audit a report could not be retained completely.
    #[error(transparent)]
    Evidence(#[from] rust_mutants::report::evidence::EvidenceError),
    /// A trace summary could not represent the recording exactly.
    #[error("{}: {source}", error::REPORT_MISSING.code)]
    TraceSummary {
        /// The exact counter or duration that did not fit.
        #[source]
        source: rust_mutants::trace::summary::SummaryError,
    },
    /// A route's projected test count or duration cannot be represented exactly.
    #[error("{}: {source}", error::REPORT_MISSING.code)]
    RouteAccounting {
        /// The exact route accounting invariant that failed.
        #[from]
        source: rust_mutants::session::RouteAccountingError,
    },
    /// A run's work ledger cannot represent its counts exactly.
    #[error("{}: {source}", error::REPORT_MISSING.code)]
    WorkAccounting {
        /// The exact work-ledger invariant that failed.
        #[from]
        source: rust_mutants::work::WorkError,
    },
    /// Discovery produced a candidate that cannot mint a stable identity.
    #[error("{}: {source}", error::CANDIDATE_INVALID.code)]
    InvalidCandidate {
        /// The invariant the candidate broke.
        #[source]
        source: rust_mutants::catalog::CandidateError,
    },
    /// A candidate byte sequence cannot cross a textual report boundary.
    #[error(
        "{}: mutation {mutant} has non-UTF-8 {field} bytes: {source}",
        error::CANDIDATE_INVALID.code
    )]
    CandidateTextNotUtf8 {
        /// The stable mutation identity.
        mutant: String,
        /// Which candidate field failed.
        field: &'static str,
        /// The exact UTF-8 validation failure.
        #[source]
        source: std::str::Utf8Error,
    },
    /// A candidate's source position cannot be represented exactly.
    #[error("{}: {source}", error::CANDIDATE_INVALID.code)]
    CandidatePosition {
        /// The exact source-position invariant that failed.
        #[from]
        source: rust_mutants::syntax::PositionError,
    },
    /// The process environment already selects a mutant.
    #[error(
        "{}: {name} is already set; a run composes the activation itself, and nothing a test \
         process said under an inherited one would be about this run",
        error::ENVIRONMENT_RESERVED.code
    )]
    EnvironmentReserved {
        /// The variable that was already set.
        name: String,
    },
    /// No stored run report answers to what was asked for.
    #[error("{}: {message}", error::REPORT_MISSING.code)]
    ReportMissing {
        /// What was looked for, and where.
        message: String,
    },
    /// The directory of stored runs could not be enumerated completely.
    #[error(
        "{}: reading stored runs under {}: {source}",
        error::REPORT_MISSING.code,
        path.display()
    )]
    StoredRunsUnreadable {
        /// The directory being enumerated.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A stored-run pointer or directory name is present but not a canonical run identity.
    #[error(
        "{}: stored run metadata at {} is invalid: {message}",
        error::REPORT_MISSING.code,
        path.display()
    )]
    StoredRunCorrupt {
        /// The metadata or directory.
        path: PathBuf,
        /// Why it cannot be trusted.
        message: String,
    },
    /// The outcome cache could not be enumerated completely.
    #[error(
        "{}: reading outcome cache under {}: {source}",
        error::CACHE_UNREADABLE.code,
        path.display()
    )]
    CacheUnreadable {
        /// The directory being enumerated.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// The ledger of deliberately retained temporary directories could not be read exactly.
    #[error(
        "{}: reading kept-directory ledger at {}: {source}",
        error::CACHE_UNREADABLE.code,
        path.display()
    )]
    KeptLedgerUnreadable {
        /// The ledger file.
        path: PathBuf,
        /// The operating system or decoding failure.
        #[source]
        source: std::io::Error,
    },
    /// A file the command would write is already there.
    #[error(
        "{}: {} is already there; pass --force to overwrite it",
        error::FILE_EXISTS.code,
        path.display()
    )]
    FileExists {
        /// The file that is in the way.
        path: PathBuf,
    },
    /// A shard is not one.
    #[error("{}: {source}", error::CONFIG_INVALID.code)]
    Shard {
        /// What is wrong with it.
        #[source]
        source: crate::run::ShardError,
    },
    /// A change set git could not be asked for.
    #[error(
        "{}: git could not be asked what differs from {base} in {}; a run that could not see \
         what changed is not a run that saw nothing change",
        error::CHANGE_SET_UNAVAILABLE.code,
        root.display()
    )]
    ChangeSetUnavailable {
        /// The tree that was asked about.
        root: PathBuf,
        /// The revision it was compared against.
        base: String,
    },
    /// A flag was given a value it cannot take.
    #[error(
        "{}: {flag} cannot take {value:?}; write {expected}",
        error::CONFIG_INVALID.code
    )]
    InvalidValue {
        /// The flag.
        flag: String,
        /// What it was given.
        value: String,
        /// What it takes.
        expected: String,
    },
    /// A platform path cannot cross a textual configuration or report boundary exactly.
    #[error("{}: {context}: {source}", error::CONFIG_INVALID.code)]
    PathNotUtf8 {
        /// Which boundary required exact text.
        context: &'static str,
        /// The exact platform path that could not be represented.
        #[source]
        source: rust_mutants::id::SlashedPathError,
    },
    /// A source file the report names is not the one the run measured, or is not there at all.
    #[error(
        "{}: {path} {why}, so the mutation cannot be shown as a change",
        error::SOURCE_UNREADABLE.code
    )]
    SourceUnreadable {
        /// The workspace-relative path the report names.
        path: String,
        /// Which of the two things is wrong with it.
        why: String,
    },
    /// A source file retained for a text report is not valid UTF-8.
    #[error(
        "{}: {path} is not valid UTF-8 and cannot be projected as source text: {source}",
        error::SOURCE_UNREADABLE.code
    )]
    SourceTextNotUtf8 {
        /// The workspace-relative source path.
        path: String,
        /// The exact UTF-8 validation failure.
        #[source]
        source: std::str::Utf8Error,
    },
    /// A file the command had to write could not be written.
    #[error("{}: writing {}: {source}", error::WRITE_FAILED.code, path.display())]
    WriteFailed {
        /// The file that could not be written.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A command's output stream refused bytes for a reason other than its reader closing it.
    #[error("{}: writing command output: {source}", error::WRITE_FAILED.code)]
    OutputFailed {
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A streamed document could not be encoded before it reached the output stream.
    #[error("{}: encoding command output: {source}", error::WRITE_FAILED.code)]
    OutputEncodingFailed {
        /// The encoder's reason.
        #[source]
        source: serde_json::Error,
    },
    /// A report projection cannot represent one of the run document's coordinates exactly.
    #[error(
        "{}: {projection} cannot represent {field} exactly",
        error::REPORT_MISSING.code
    )]
    ProjectionOverflow {
        /// The projection whose numeric domain is too small.
        projection: &'static str,
        /// The coordinate that did not fit.
        field: &'static str,
    },
    /// The owned preparation thread could not be started.
    #[error("{}: cannot start the preparation worker: {source}", error::WRITE_FAILED.code)]
    PreparationStartFailed {
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// The owned preparation thread panicked before it was joined.
    #[error(
        "{}: the preparation worker panicked before it was joined",
        error::WRITE_FAILED.code
    )]
    PreparationPanicked,
    /// A continuous integration host was asked for that the environment does not provide.
    #[error(
        "{}: --host {host} was asked for, and this is not a {host} step that names where a step reports",
        error::CI_HOST_UNAVAILABLE.code
    )]
    CiHostUnavailable {
        /// The host, as `--host` spells it.
        host: &'static str,
    },
    /// The workspace root is not inside the checkout the host places annotations in.
    #[error(
        "{}: {} is not inside the checkout {}, so no annotation could name a file the host finds",
        error::CI_ROOT_OUTSIDE_CHECKOUT.code,
        root.display(),
        checkout.display()
    )]
    CiRootOutsideCheckout {
        /// The workspace root.
        root: PathBuf,
        /// The checkout the host names.
        checkout: PathBuf,
    },
    /// A path the placement of annotations depends on could not be resolved.
    #[error(
        "{}: resolving {}: {source}",
        error::CI_ROOT_OUTSIDE_CHECKOUT.code,
        path.display()
    )]
    CiPathUnresolved {
        /// The workspace root or the checkout.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A file the host named for a step's summary or outputs could not be appended to.
    #[error("{}: appending to {}: {source}", error::CI_SINK_UNWRITABLE.code, path.display())]
    CiSinkUnwritable {
        /// The file the host named.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
}

impl From<crate::run::ShardError> for CliError {
    fn from(source: crate::run::ShardError) -> Self {
        Self::Shard { source }
    }
}

impl CliError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Engine(inner) => inner.code(),
            Self::Config(inner) => inner.code(),
            Self::Evidence(_)
            | Self::WriteFailed { .. }
            | Self::OutputFailed { .. }
            | Self::OutputEncodingFailed { .. }
            | Self::PreparationStartFailed { .. }
            | Self::PreparationPanicked => error::WRITE_FAILED,
            Self::TraceSummary { .. }
            | Self::RouteAccounting { .. }
            | Self::WorkAccounting { .. }
            | Self::ReportMissing { .. }
            | Self::StoredRunsUnreadable { .. }
            | Self::StoredRunCorrupt { .. }
            | Self::ProjectionOverflow { .. } => error::REPORT_MISSING,
            Self::InvalidCandidate { .. }
            | Self::CandidateTextNotUtf8 { .. }
            | Self::CandidatePosition { .. } => error::CANDIDATE_INVALID,
            Self::EnvironmentReserved { .. } => error::ENVIRONMENT_RESERVED,
            Self::CiHostUnavailable { .. } => error::CI_HOST_UNAVAILABLE,
            Self::CiRootOutsideCheckout { .. } | Self::CiPathUnresolved { .. } => {
                error::CI_ROOT_OUTSIDE_CHECKOUT
            }
            Self::CiSinkUnwritable { .. } => error::CI_SINK_UNWRITABLE,
            Self::CacheUnreadable { .. } | Self::KeptLedgerUnreadable { .. } => {
                error::CACHE_UNREADABLE
            }
            Self::FileExists { .. } => error::FILE_EXISTS,
            Self::Shard { .. } | Self::InvalidValue { .. } | Self::PathNotUtf8 { .. } => {
                error::CONFIG_INVALID
            }
            Self::ChangeSetUnavailable { .. } => error::CHANGE_SET_UNAVAILABLE,
            Self::SourceUnreadable { .. } | Self::SourceTextNotUtf8 { .. } => {
                error::SOURCE_UNREADABLE
            }
        }
    }

    /// A file the report names that this tree does not hold.
    #[must_use]
    pub fn absent(path: &str, root: &Path) -> Self {
        Self::SourceUnreadable {
            path: path.to_owned(),
            why: format!("is not under {}", root.display()),
        }
    }

    /// A file this tree holds that is not the one the run measured.
    #[must_use]
    pub fn moved_on(path: &str) -> Self {
        Self::SourceUnreadable {
            path: path.to_owned(),
            why: "changed since the run".to_owned(),
        }
    }

    /// A source file the command selected but the operating system would not yield.
    #[must_use]
    pub fn unreadable(path: &str, source: &std::io::Error) -> Self {
        Self::SourceUnreadable {
            path: path.to_owned(),
            why: format!("could not be read: {source}"),
        }
    }

    /// The failure of writing `path`.
    #[must_use]
    pub fn writing(path: &Path, source: std::io::Error) -> Self {
        Self::WriteFailed {
            path: path.to_path_buf(),
            source,
        }
    }
}

impl From<rust_mutants::catalog::CandidateError> for CliError {
    fn from(source: rust_mutants::catalog::CandidateError) -> Self {
        Self::InvalidCandidate { source }
    }
}
