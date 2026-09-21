// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The integration resources a run starts, and what they may tell a test.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
#[cfg(feature = "testkit")]
use std::path::Path;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::Resource;
use crate::error::{self, ErrorCode};
use crate::provider::{InstanceId, Process, ProviderError, Request};

/// The variables a run composes for itself, which a provider may therefore not offer.
pub const RESERVED_NAMES: [&str; 6] = [
    "CARGO",
    "CARGO_TARGET_DIR",
    "RUSTC",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "LLVM_PROFILE_FILE",
];

/// The prefixes a run composes for itself. A provider that sets one of these decides what every test process measures.
pub const RESERVED_PREFIXES: [&str; 5] = ["NJUTEST_", "RUST_MUTANTS_", "CARGO_", "TMP", "TEMP"];

/// Why a resource could not be used.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ResourceError {
    /// The provider could not be used.
    #[error(transparent)]
    Provider(#[from] ProviderError),
    /// The provider offered a variable the run composes itself.
    #[error(
        "{}: the provider of {capability:?} offered {name}, which a run composes itself",
        error::RESOURCE_ENVIRONMENT_REFUSED.code
    )]
    EnvironmentRefused {
        /// The capability whose provider offered it.
        capability: String,
        /// The variable it offered.
        name: String,
    },
    /// The primary resource failure and the failure to clean up its provider.
    #[error("{primary}; cleanup also failed: {cleanup}")]
    CleanupAfterFailure {
        /// The failure that caused the resource operation to stop.
        primary: Box<Self>,
        /// Why the provider could not then be ended safely.
        cleanup: ProviderError,
    },
    /// Starting one resource failed, then releasing resources already started
    /// for the same run also produced one or more failures.
    #[error("{primary}; releasing earlier resources also failed: {cleanup:?}")]
    ReleaseAfterFailure {
        /// The failure that prevented the requested resource from starting.
        primary: Box<Self>,
        /// Every failure from the bounded reverse-order release.
        cleanup: Vec<Self>,
    },
}

impl ResourceError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        match self {
            Self::Provider(inner) => inner.code(),
            Self::EnvironmentRefused { .. } => error::RESOURCE_ENVIRONMENT_REFUSED,
            Self::CleanupAfterFailure { primary, .. }
            | Self::ReleaseAfterFailure { primary, .. } => primary.code(),
        }
    }
}

/// One capability a run holds, and what its tests see because of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lease {
    /// The capability the resource provides.
    pub capability: String,
    /// The instance the provider named.
    pub instance: InstanceId,
    /// What the tests of this run see, in name order.
    pub environment: Vec<(String, String)>,
}

/// One live provider, and the lease it stands for.
#[derive(Debug)]
struct Live {
    process: Process,
    lease: Lease,
    timeout: Duration,
}

/// Where the providers run, and what they may see of this process's environment.
#[derive(Debug, Clone)]
pub struct Where {
    /// The directory a provider runs in.
    pub dir: PathBuf,
    /// The environment this run was given, from which a provider sees only what its configuration names.
    pub env: Vec<(OsString, OsString)>,
}

/// Every resource a run holds, started on demand and stopped together.
#[derive(Debug)]
pub struct Manager {
    live: Vec<Live>,
    sequence: u32,
    place: Where,
}

/// How long a release spends stopping providers before it kills what is left.
///
/// Stopping asks each provider politely and then kills it, both bounded by
/// that resource's own timeout. A run holding several wedged providers would
/// spend that twice per provider before it could exit, so the whole release
/// is bounded too: what it does not reach politely is killed at once and
/// reported, rather than making the run wait for something it is leaving
/// anyway.
const RELEASE_BUDGET: Duration = Duration::from_secs(30);

impl Drop for Manager {
    /// Stops whatever is still running, because nothing outside this process will.
    ///
    /// A provider is a child process holding something the operating system
    /// owns — a database, a container, a port. Every early exit between
    /// starting one and releasing it would otherwise leave it running with
    /// nothing naming it, and a build that fails leaves one behind per
    /// attempt.
    fn drop(&mut self) {
        let refusals = self.release();
        for refusal in refusals {
            drop(refusal);
        }
    }
}

impl Manager {
    /// A manager that starts providers in `place`.
    #[must_use]
    pub const fn new(place: Where) -> Self {
        Self {
            live: Vec::new(),
            sequence: 0,
            place,
        }
    }

    fn next_sequence(&mut self) -> Result<u32, ProviderError> {
        self.sequence = self.sequence.checked_add(1).ok_or_else(|| {
            ProviderError::new(
                crate::provider::ProviderErrorKind::Protocol,
                "the provider request sequence is exhausted",
            )
        })?;
        Ok(self.sequence)
    }

    /// What every test of this run sees because of the resources it holds, in name order.
    #[must_use]
    #[cfg(any(test, feature = "testkit"))]
    #[cfg(feature = "testkit")]
    pub fn environment(&self) -> Vec<(String, String)> {
        let mut all: BTreeMap<String, String> = BTreeMap::new();
        for live in &self.live {
            for (name, value) in &live.lease.environment {
                all.insert(name.clone(), value.clone());
            }
        }
        all.into_iter().collect()
    }

    /// The leases the run holds, in the order they were started.
    #[must_use]
    pub fn leases(&self) -> Vec<&Lease> {
        self.live.iter().map(|live| &live.lease).collect()
    }

    /// Starts `capability` and holds it until the manager is released.
    ///
    /// # Errors
    /// Every failure of the provider, and an environment a run composes
    /// itself.
    pub fn start(
        &mut self,
        capability: &str,
        resource: &Resource,
    ) -> Result<&Lease, ResourceError> {
        if let Some(position) = self
            .live
            .iter()
            .position(|live| live.lease.capability == capability)
        {
            let found = self.live.get(position).map(|live| &live.lease);
            return found.ok_or_else(|| {
                ResourceError::from(ProviderError::new(
                    crate::provider::ProviderErrorKind::Protocol,
                    format!("the lease of {capability:?} went missing"),
                ))
            });
        }
        let sequence = self.next_sequence()?;
        let mut process = Process::start(
            &resource.command,
            &self.place.dir,
            &visible(&self.place.env, &resource.environment),
        )?;
        let answered = process.ask(&Request::start(capability, sequence), resource.timeout);
        let answered = match answered {
            Ok(answered) => answered,
            Err(refusal) => {
                return match process.end(resource.timeout) {
                    Ok(()) => Err(refusal.into()),
                    Err(cleanup) => Err(refusal.with_cleanup(cleanup).into()),
                };
            }
        };
        let (instance, offered) = answered.into_parts();
        let environment = match admissible(capability, &offered) {
            Ok(environment) => environment,
            Err(refusal) => {
                return match process.end(resource.timeout) {
                    Ok(()) => Err(refusal),
                    Err(cleanup) => Err(ResourceError::CleanupAfterFailure {
                        primary: Box::new(refusal),
                        cleanup,
                    }),
                };
            }
        };
        let lease = Lease {
            capability: capability.to_owned(),
            instance,
            environment,
        };
        self.live.push(Live {
            process,
            lease,
            timeout: resource.timeout,
        });
        self.live.last().map(|live| &live.lease).ok_or_else(|| {
            ResourceError::from(ProviderError::new(
                crate::provider::ProviderErrorKind::Protocol,
                format!("the lease of {capability:?} went missing"),
            ))
        })
    }

    /// Stops everything, in the reverse of the order it was started, and says what would not stop.
    pub fn release(&mut self) -> Vec<ResourceError> {
        let mut refusals = Vec::new();
        let started = Instant::now();
        while let Some(live) = self.live.pop() {
            if started.elapsed() >= RELEASE_BUDGET {
                refusals.push(ResourceError::from(ProviderError::new(
                    crate::provider::ProviderErrorKind::Timeout,
                    format!(
                        "{:?} was killed rather than asked to stop: the release ran out of \
                         the time it was given before reaching it",
                        live.lease.capability
                    ),
                )));
                let Live { process, .. } = live;
                if let Err(refusal) = process.end(Duration::ZERO) {
                    refusals.push(refusal.into());
                }
                continue;
            }
            let Live {
                mut process,
                lease,
                timeout,
            } = live;
            match self.next_sequence() {
                Ok(sequence) => {
                    if let Err(refusal) = process.ask(
                        &Request::stop(&lease.capability, &lease.instance, sequence),
                        timeout,
                    ) {
                        refusals.push(refusal.into());
                    }
                }
                Err(refusal) => refusals.push(refusal.into()),
            }
            if let Err(refusal) = process.end(timeout) {
                refusals.push(refusal.into());
            }
        }
        refusals
    }
}

/// What a provider may see of this run's environment: exactly the names its configuration lists.
#[must_use]
pub fn visible(env: &[(OsString, OsString)], allowed: &[String]) -> Vec<(OsString, OsString)> {
    env.iter()
        .filter(|(name, _)| {
            rust_mutants::vars::same_name(name, OsStr::new("PATH"))
                || allowed
                    .iter()
                    .any(|wanted| rust_mutants::vars::same_name(name, OsStr::new(wanted)))
        })
        .cloned()
        .collect()
}

/// What a provider offered, refused if any of it is a variable the run composes itself.
///
/// # Errors
/// [`ResourceError::EnvironmentRefused`] for the first reserved name.
pub fn admissible(
    capability: &str,
    offered: &BTreeMap<String, String>,
) -> Result<Vec<(String, String)>, ResourceError> {
    for name in offered.keys() {
        if RESERVED_NAMES.contains(&name.as_str())
            || RESERVED_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        {
            return Err(ResourceError::EnvironmentRefused {
                capability: capability.to_owned(),
                name: name.clone(),
            });
        }
    }
    Ok(offered
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect())
}

/// Where providers run when a run has a scratch of its own.
#[must_use]
#[cfg(feature = "testkit")]
pub fn place(dir: &Path, env: &[(OsString, OsString)]) -> Where {
    Where {
        dir: dir.to_path_buf(),
        env: env.to_vec(),
    }
}
