// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Actual compiler observations whose input identity and independent purpose survive verified reuse.

use serde::{Deserialize, Serialize};

use super::{CargoError, CargoErrorKind, Witness};
use crate::runner::{RunResult, Spec};
use crate::trace::ExecRecord;

/// The complete compilation input identity, or the reason no complete identity can be claimed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "identity",
    rename_all = "kebab-case",
    deny_unknown_fields
)]
pub enum InputIdentity {
    /// Every source, dependency, toolchain, configuration and observed environment input is bound.
    Complete(String),
    /// An actual compiler is required because this input graph cannot be completely bound.
    Unbound(String),
}

impl InputIdentity {
    /// The complete key, when this graph has one.
    #[must_use]
    pub fn complete_key(&self) -> Option<&str> {
        match self {
            Self::Complete(key) => Some(key),
            Self::Unbound(_) => None,
        }
    }

    pub(super) fn detail(&self) -> &str {
        match self {
            Self::Complete(key) | Self::Unbound(key) => key,
        }
    }
}

/// Why an actual compiler process was required.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CompilerPurpose {
    /// Establishing the products of a complete input graph.
    Products,
    /// Independently compiling restored inputs to test reproducibility against an earlier producer.
    IndependentControl,
}

/// The retained original actual process and raw streams that established compiler products.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, try_from = "RecordedObservation")]
pub struct CompilerObservation {
    id: String,
    identity: InputIdentity,
    purpose: CompilerPurpose,
    exec: ExecRecord,
    leader: u32,
    stdout_digest: String,
    stderr: Vec<u8>,
    stderr_digest: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordedObservation {
    id: String,
    identity: InputIdentity,
    purpose: CompilerPurpose,
    exec: ExecRecord,
    leader: u32,
    stdout_digest: String,
    stderr: Vec<u8>,
    stderr_digest: String,
}

impl TryFrom<RecordedObservation> for CompilerObservation {
    type Error = std::io::Error;

    fn try_from(record: RecordedObservation) -> Result<Self, Self::Error> {
        let mut held = Self {
            id: record.id,
            identity: record.identity,
            purpose: record.purpose,
            exec: record.exec,
            leader: record.leader,
            stdout_digest: record.stdout_digest,
            stderr: record.stderr,
            stderr_digest: record.stderr_digest,
        };
        if !held.verifies_output() {
            return Err(std::io::Error::other("unverified original compiler output"));
        }
        held.exec.output.clone_from(&held.stderr);
        Ok(held)
    }
}

impl CompilerObservation {
    pub(super) fn actual(
        (spec, result): (&Spec, &RunResult),
        identity: InputIdentity,
        witness: Witness,
    ) -> Result<Self, CargoError> {
        let unavailable = |message: String| CargoError::new(CargoErrorKind::CommandFailed, message);
        let leader = result
            .leader
            .filter(|leader| *leader != 0)
            .ok_or_else(|| unavailable("compiler products have no actual producer".to_owned()))?;
        let id = crate::execute::fresh_nonce()
            .map_err(|source| unavailable(format!("compiler observation identity: {source}")))?;
        let mut exec =
            ExecRecord::of(spec, result).map_err(|source| unavailable(source.to_string()))?;
        exec.output_bytes =
            u64::try_from(result.output.len()).map_err(|source| unavailable(source.to_string()))?;
        if !result.output.is_empty() {
            exec.output_sha256 = Some(crate::id::digest(&result.output));
        }
        Ok(Self {
            id,
            identity,
            purpose: match witness {
                Witness::Any => CompilerPurpose::Products,
                Witness::Compiler => CompilerPurpose::IndependentControl,
            },
            exec,
            leader,
            stdout_digest: crate::id::digest(&result.stdout),
            stderr: result.output.clone(),
            stderr_digest: crate::id::digest(&result.output),
        })
    }

    pub(super) fn verifies(&self, stdout: &[u8], key: &str, exit: i32) -> bool {
        self.identity.complete_key() == Some(key)
            && self.id.len() == 32
            && self.id.bytes().all(|byte| byte.is_ascii_hexdigit())
            && self.leader != 0
            && self.stdout_digest == crate::id::digest(stdout)
            && self.verifies_output()
            && self.exec.output == self.stderr
            && self.exec.error.is_none()
            && matches!(self.exec.stopped,
                crate::execute::Stopped::Exited { exit: crate::runner::ProcessExit::Code(code) }
                if code == exit)
    }

    fn verifies_output(&self) -> bool {
        u64::try_from(self.stderr.len()).is_ok_and(|bytes| bytes == self.exec.output_bytes)
            && self.stderr_digest == crate::id::digest(&self.stderr)
            && !self.exec.output_truncated
            && self.exec.output_path.is_none()
            && match self.exec.output_sha256.as_deref() {
                Some(digest) => digest == self.stderr_digest,
                None => self.stderr.is_empty(),
            }
    }

    /// The unique identity of the original actual producer, retained unchanged on reuse.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The exact complete graph this actual process was asked to compile.
    #[must_use]
    pub const fn identity(&self) -> &InputIdentity {
        &self.identity
    }

    /// The purpose that required this actual process.
    #[must_use]
    pub const fn purpose(&self) -> CompilerPurpose {
        self.purpose
    }

    /// The raw stderr retained from this same compiler producer.
    #[must_use]
    pub fn stderr(&self) -> &[u8] {
        &self.stderr
    }
}
