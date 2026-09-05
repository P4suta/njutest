// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cargo boundary: locating the toolchain, reading `cargo metadata`,
//! parsing `--message-format=json`, and reading dep-info to learn which
//! files a unit really compiled.
//!
//! Everything here is a small serde shape over what cargo and rustc print,
//! read fail-closed: a line that is not a message, a metadata document
//! without its packages, or a version banner without its `release:` is an
//! error naming what was wrong, never a guess. The module never reads the
//! process environment: the composition root hands it the search path and
//! the environment a child should see, and the snapshot directory the
//! commands run in decides which rustup toolchain answers.

mod compile;
mod depinfo;
mod locate;
mod messages;
mod metadata;
mod version;

use std::fmt;
use std::path::Path;

use crate::error::{self, ErrorCode};
use crate::runner::Cancel;
use crate::trace::Recorder;

pub use compile::{CompileKind, CompileOptions, Compiled, compile};
pub use depinfo::{Unit, dep_info_path, parse_dep_info, units_of};

pub use locate::{LocateOptions, Toolchain, command_failed, resolve_executable};
pub use messages::{
    Artifact, CompilerMessage, Diagnostic, DiagnosticSpan, Message, Profile, parse_messages,
};
pub use metadata::{Metadata, MetadataOptions, Package, Target};
pub use version::{VersionInfo, parse_version};

/// Everything a cargo command needs besides its arguments: the toolchain,
/// the directory to run in, the cancellation flag, and the trace.
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    /// A dep-info file has no rule to read.
    DepInfoUnreadable,
    /// An artifact's dep-info file could not be read.
    DepInfoMissing,
}

impl CargoErrorKind {
    /// Every kind, in code order.
    pub const ALL: [Self; 7] = [
        Self::ToolchainNotFound,
        Self::VersionUnreadable,
        Self::CommandFailed,
        Self::MetadataUnparsable,
        Self::MessageUnparsable,
        Self::DepInfoUnreadable,
        Self::DepInfoMissing,
    ];

    /// The stable code of this failure.
    #[must_use]
    pub const fn code(self) -> ErrorCode {
        match self {
            Self::ToolchainNotFound => error::CARGO_TOOLCHAIN_NOT_FOUND,
            Self::VersionUnreadable => error::CARGO_VERSION_UNREADABLE,
            Self::CommandFailed => error::CARGO_COMMAND_FAILED,
            Self::MetadataUnparsable => error::CARGO_METADATA_UNPARSABLE,
            Self::MessageUnparsable => error::CARGO_MESSAGE_UNPARSABLE,
            Self::DepInfoUnreadable => error::DEP_INFO_UNREADABLE,
            Self::DepInfoMissing => error::DEP_INFO_MISSING,
        }
    }
}

/// Every error this module returns.
#[derive(Debug)]
pub struct CargoError {
    kind: CargoErrorKind,
    message: String,
    source: Option<Box<dyn std::error::Error + Send + Sync + 'static>>,
}

impl CargoError {
    pub(crate) fn new(kind: CargoErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            source: None,
        }
    }

    pub(crate) fn with_source(
        mut self,
        source: impl std::error::Error + Send + Sync + 'static,
    ) -> Self {
        self.source = Some(Box::new(source));
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
            .as_deref()
            .map(|source| -> &(dyn std::error::Error + 'static) { source })
    }
}
