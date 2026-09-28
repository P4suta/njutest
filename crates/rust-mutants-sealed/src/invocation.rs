// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Everything one invocation is a function of, as one value, and the digest of it.

use std::collections::BTreeMap;
use std::num::NonZeroU64;

use crate::digest::{Encoder, SealedDigest};
use crate::error::{EnvironmentFault, PreopenFault, SealedError};
use crate::snapshot::Snapshot;

/// The arguments a guest reads, the program name first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arguments(Vec<String>);

impl Arguments {
    /// The arguments, refusing one a C string cannot carry.
    ///
    /// # Errors
    /// [`SealedError::ArgumentHoldsNul`] for an argument holding a NUL byte.
    pub fn new(values: Vec<String>) -> Result<Self, SealedError> {
        match values.iter().position(|value| value.contains('\0')) {
            Some(index) => Err(SealedError::ArgumentHoldsNul { index }),
            None => Ok(Self(values)),
        }
    }

    /// The arguments, the program name first.
    #[must_use]
    pub fn as_slice(&self) -> &[String] {
        &self.0
    }
}

/// The environment a guest reads, in the order of its names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment(BTreeMap<String, String>);

impl Environment {
    /// The variables, refusing one a guest cannot be given.
    ///
    /// # Errors
    /// [`SealedError::EnvironmentVariable`] for an empty name, a name holding `=` or NUL, a value holding NUL, or a name given twice.
    pub fn new(variables: Vec<(String, String)>) -> Result<Self, SealedError> {
        let mut held = BTreeMap::new();
        for (name, value) in variables {
            let fault = if name.is_empty() {
                Some(EnvironmentFault::EmptyName)
            } else if name.contains('=') {
                Some(EnvironmentFault::NameHoldsEquals)
            } else if name.contains('\0') || value.contains('\0') {
                Some(EnvironmentFault::HoldsNul)
            } else if held.contains_key(&name) {
                Some(EnvironmentFault::Repeated)
            } else {
                None
            };
            if let Some(fault) = fault {
                return Err(SealedError::EnvironmentVariable { name, fault });
            }
            held.insert(name, value);
        }
        Ok(Self(held))
    }

    /// Every variable, in the order of its name.
    pub fn variables(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// The snapshots a guest may reach, each at its guest path, in the order their descriptors are numbered from 3.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preopens(Vec<(String, Snapshot)>);

impl Preopens {
    /// The preopens, refusing a guest path the guest cannot be given.
    ///
    /// # Errors
    /// [`SealedError::Preopen`] for an empty guest path, one holding NUL, or one given twice.
    pub fn new(preopens: Vec<(String, Snapshot)>) -> Result<Self, SealedError> {
        for (at, (path, _snapshot)) in preopens.iter().enumerate() {
            let fault = if path.is_empty() {
                Some(PreopenFault::Empty)
            } else if path.contains('\0') {
                Some(PreopenFault::HoldsNul)
            } else if preopens
                .iter()
                .take(at)
                .any(|(earlier, _snapshot)| earlier == path)
            {
                Some(PreopenFault::Repeated)
            } else {
                None
            };
            if let Some(fault) = fault {
                return Err(SealedError::Preopen {
                    path: path.clone(),
                    fault,
                });
            }
        }
        Ok(Self(preopens))
    }

    /// Every preopen, as its guest path and its snapshot.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Snapshot)> {
        self.0
            .iter()
            .map(|(path, snapshot)| (path.as_str(), snapshot))
    }
}

/// The resource ceilings of an invocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// The most linear memory the guest may hold, in bytes.
    pub memory: u64,
    /// The most bytes of standard output kept; the rest are counted.
    pub stdout: u64,
    /// The most bytes of standard error kept; the rest are counted.
    pub stderr: u64,
    /// The most bytes the overlay may hold: every byte of every file written, and a share for every name made.
    pub overlay: u64,
}

/// How the guest's clocks read: from fixed origins, moved only by fuel spent and by waits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClockPolicy {
    /// What the realtime clock reads before the guest has spent anything, in nanoseconds since the Unix epoch.
    pub realtime_origin: u64,
    /// What the monotonic clock reads before the guest has spent anything, in nanoseconds.
    pub monotonic_origin: u64,
    /// How many nanoseconds each unit of fuel spent moves the clocks, never zero so a wait on time ends.
    pub nanos_per_fuel: NonZeroU64,
}

/// Everything one invocation of a sealed module is a function of, as one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The arguments the guest reads, the program name first.
    pub arguments: Arguments,
    /// The environment the guest reads.
    pub environment: Environment,
    /// The read-only snapshots the guest may reach.
    pub preopens: Preopens,
    /// The seed of the guest's random bytes.
    pub seed: u64,
    /// The fuel the guest may spend before it is stopped.
    pub fuel: u64,
    /// The resource ceilings.
    pub limits: Limits,
    /// How the guest's clocks read.
    pub clock: ClockPolicy,
}

impl Invocation {
    /// The digest of this invocation of the module `module` under the configuration `configuration`.
    pub(crate) fn digest(
        &self,
        module: &SealedDigest,
        configuration: &SealedDigest,
    ) -> SealedDigest {
        let mut encoder = Encoder::new("rust-mutants-sealed/invocation/v1");
        encoder.digest(configuration).digest(module);
        encoder.count(self.arguments.0.len());
        for argument in &self.arguments.0 {
            encoder.text(argument);
        }
        encoder.count(self.environment.0.len());
        for (name, value) in &self.environment.0 {
            encoder.text(name).text(value);
        }
        encoder.count(self.preopens.0.len());
        for (path, snapshot) in &self.preopens.0 {
            encoder.text(path).digest(snapshot.digest());
        }
        encoder
            .number(self.seed)
            .number(self.fuel)
            .number(self.limits.memory)
            .number(self.limits.stdout)
            .number(self.limits.stderr)
            .number(self.limits.overlay)
            .number(self.clock.realtime_origin)
            .number(self.clock.monotonic_origin)
            .number(self.clock.nanos_per_fuel.get());
        encoder.finish()
    }
}
