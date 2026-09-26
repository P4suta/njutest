// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cargo boundary: locating the toolchain, reading `cargo metadata`, parsing `--message-format=json`, and reading dep-info to learn which files a unit really compiled.

mod build_identity;
mod built;
mod compile;
pub mod config;
mod depinfo;
mod locate;
pub mod manifest;
mod messages;
mod metadata;
mod outside;
mod version;

use std::fmt;
use std::path::Path;

use crate::error::{self, ErrorCode};
use crate::runner::Cancel;
use crate::trace::Recorder;

pub use build_identity::{BUILD_SELECTION_DOMAIN, BuildSelection, BuildSelectionDigest};
pub use built::{BuildDir, LEDGER_NAME, LEDGER_SCHEMA, Member, MemberFile, fingerprint_of};
pub use compile::{
    BuildConfig, Compilation, CompileKind, CompileOptions, Compiled, compile, compile_arguments,
};
pub use depinfo::{
    Emitted, Unit, compile_time_inputs, dep_info_path, emitted_of, env_deps, every_unit_of,
    parse_dep_info, units_of,
};

pub use locate::{
    ForTests, LocateOptions, Selecting, Toolchain, command_failed, resolve_executable,
};
pub use messages::{
    Artifact, BuildScript, CompilerMessage, Diagnostic, DiagnosticSpan, Message, Profile,
    names_file, parse_messages,
};
pub use metadata::{
    DepKind, Dependency, Metadata, MetadataOptions, Node, NodeDep, Package, Resolve, Target,
    metadata_arguments,
};
pub use outside::{Outside, reaching_outside};
pub use version::{VersionInfo, parse_version};

/// Everything a cargo command needs besides its arguments: the toolchain, the directory to run in, the cancellation flag, and the trace.
#[derive(Debug, Clone, Copy)]
pub struct Driver<'a> {
    /// The located toolchain.
    pub toolchain: &'a Toolchain,
    /// The directory every command runs in: the workspace (snapshot) root.
    pub dir: &'a Path,
    /// Cooperative cancellation.
    pub cancel: &'a Cancel,
    /// The trace every command records into.
    pub trace: &'a Recorder,
}

/// The failure modes of this module, each with a stable code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, njutest_macros::AllVariants)]
pub enum CargoErrorKind {
    /// The cargo or rustc executable could not be found.
    ToolchainNotFound,
    /// A `-vV` banner lacks its `release:` or `host:` line.
    VersionUnreadable,
    /// A cargo command could not start or exited unsuccessfully.
    CommandFailed,
    /// `cargo metadata` printed something that is not its document.
    MetadataUnparsable,
    /// A `--message-format=json` line is not a message.
    MessageUnparsable,
    /// A manifest a run must read is there and could not be read or parsed.
    ManifestUnreadable,
    /// A dep-info file has no rule to read.
    DepInfoUnreadable,
    /// An artifact's dep-info file could not be read.
    DepInfoMissing,
    /// The caller cancelled before the command finished, so what it printed says nothing.
    Cancelled,
    /// A target directory's record of what its members were built from could not be read or written, or a unit it names as stale could not be forgotten.
    BuildLedger,
    /// A bare `cargo` from the copy a run measures answers as no toolchain the run can put first on the tests' search path.
    TestsToolchain,
}

impl CargoErrorKind {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::ToolchainNotFound => error::CARGO_TOOLCHAIN_NOT_FOUND,
            Self::VersionUnreadable => error::CARGO_VERSION_UNREADABLE,
            Self::CommandFailed => error::CARGO_COMMAND_FAILED,
            Self::MetadataUnparsable => error::CARGO_METADATA_UNPARSABLE,
            Self::MessageUnparsable => error::CARGO_MESSAGE_UNPARSABLE,
            Self::ManifestUnreadable => error::MANIFEST_UNREADABLE,
            Self::DepInfoUnreadable => error::DEP_INFO_UNREADABLE,
            Self::DepInfoMissing => error::DEP_INFO_MISSING,
            Self::Cancelled => error::INTERRUPTED,
            Self::BuildLedger => error::BUILD_LEDGER_UNREADABLE,
            Self::TestsToolchain => error::TESTS_TOOLCHAIN_UNREACHABLE,
        }
    }
}

/// Every error this module returns.
#[derive(Debug)]
pub struct CargoError {
    kind: CargoErrorKind,
    message: String,
    source: Option<CargoSourceError>,
}

/// What underlies a cargo failure.
#[derive(Debug)]
#[non_exhaustive]
pub enum CargoSourceError {
    /// A document cargo printed could not be read.
    Json(serde_json::Error),
    /// A file could not be read.
    Io(std::io::Error),
    /// A textual tool protocol emitted bytes that are not UTF-8.
    Utf8(std::str::Utf8Error),
}

impl fmt::Display for CargoSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => error.fmt(f),
            Self::Io(error) => error.fmt(f),
            Self::Utf8(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for CargoSourceError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Json(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Utf8(error) => Some(error),
        }
    }
}

impl From<serde_json::Error> for CargoSourceError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<std::io::Error> for CargoSourceError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<std::str::Utf8Error> for CargoSourceError {
    fn from(error: std::str::Utf8Error) -> Self {
        Self::Utf8(error)
    }
}

impl CargoError {
    pub(crate) fn new(kind: CargoErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn with_source(mut self, source: impl Into<CargoSourceError>) -> Self {
        self.source = Some(source.into());
        self
    }

    /// The failure mode.
    #[must_use]
    pub const fn kind(&self) -> CargoErrorKind {
        self.kind
    }

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.kind.code()
    }

    /// The problem in one clause, without the code.
    #[must_use]
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl fmt::Display for CargoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: cargo: {}", self.code().code, self.message)?;
        if let Some(source) = &self.source {
            write!(f, ": {source}")?;
        }
        Ok(())
    }
}

impl std::error::Error for CargoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|source| -> &(dyn std::error::Error + 'static) { source })
    }
}
