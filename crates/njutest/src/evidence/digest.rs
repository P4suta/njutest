// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One run's identity.
//! Two runs share it exactly when nothing that could change what the tests say has changed.

use sha2::{Digest as _, Sha256};

use crate::config::Contract;

/// The domain hashed first for a run's identity.
/// The recipe version is in the name: a future recipe becomes `njutest-evidence-v4` so a v3 identity can never be mistaken for a v4 one.
pub const EVIDENCE_DOMAIN: &str = "njutest-evidence-v3";

/// A digest built from named fields.
/// Every field is length-prefixed and preceded by its own name, so no two different lists of values can produce the same number.
#[derive(Debug)]
pub struct Fields {
    hasher: Sha256,
}

impl Fields {
    /// A digest over `domain`, which is hashed before anything else.
    #[must_use]
    pub fn new(domain: &str) -> Self {
        let mut hasher = Sha256::new();
        write(&mut hasher, domain);
        Self { hasher }
    }

    /// Adds one named value.
    pub fn field(&mut self, name: &str, value: &str) -> &mut Self {
        write(&mut self.hasher, name);
        write(&mut self.hasher, value);
        self
    }

    /// Adds a named list, in the order given.
    /// A caller for whom order is not a fact sorts first.
    /// The list is owned before hashing so its exact,
    /// allocation-bounded length is a value rather than an overflowable counter over an adversarial iterator.
    pub fn list<I, S>(&mut self, name: &str, values: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let values: Vec<S> = values.into_iter().collect();
        write(&mut self.hasher, name);
        let mut items = Sha256::new();
        for value in &values {
            write(&mut items, value.as_ref());
        }
        write(&mut self.hasher, &values.len().to_string());
        write(&mut self.hasher, &hex::encode(items.finalize()));
        self
    }

    /// The digest, as sixty-four lowercase hex characters.
    #[must_use]
    pub fn finish(self) -> String {
        hex::encode(self.hasher.finalize())
    }
}

/// Adds one value to a digest as the digest of its bytes, which is a fixed width, so no two different lists of values give one number.
fn write(hasher: &mut Sha256, value: &str) {
    hasher.update(Sha256::digest(value.as_bytes()));
}

/// How much of the workspace a run looked at.
///
/// It is part of the identity: a run that looked at one package established less than one that looked at everything, and the two must never share a cached answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    /// The whole workspace.
    Full,
    /// What changed since a git revision.
    Changed {
        /// The revision the change set was computed against.
        base: String,
    },
    /// Only these packages.
    Scoped {
        /// The packages, as named.
        /// The identity reads them sorted.
        packages: Vec<String>,
    },
}

impl Mode {
    /// The canonical wire name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Changed { .. } => "changed",
            Self::Scoped { .. } => "scoped",
        }
    }

    /// What the mode looked at, beyond its name.
    #[must_use]
    pub fn detail(&self) -> Vec<String> {
        match self {
            Self::Full => Vec::new(),
            Self::Changed { base } => vec![base.clone()],
            Self::Scoped { packages } => packages
                .iter()
                .cloned()
                .collect::<std::collections::BTreeSet<String>>()
                .into_iter()
                .collect(),
        }
    }
}

/// Everything a run's identity is computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inputs {
    /// The digest of every file under verification.
    pub tree: String,
    /// The digest of the fuzz corpora, kept apart from the tree because a corpus grows without the code changing.
    pub corpus: String,
    /// The digest of the resolved dependencies, from the lock file's checksums.
    pub dependencies: String,
    /// The toolchain, as it names itself.
    pub toolchain: String,
    /// The target triple the run happens on.
    pub platform: String,
    /// The digest of the running njutest, because two builds of it may mean two different things by the same answer.
    pub engine: String,
    /// The environment the run selected, as names and values.
    /// Read in a fixed order.
    pub environment: Vec<(String, String)>,
    /// The contract the run answers to.
    pub contract: Contract,
    /// The digest of the effective configuration.
    pub configuration: String,
    /// The arguments the command line gave the test binaries, which the configuration does not carry.
    pub test_args: Vec<String>,
    /// How much of the workspace the run looked at.
    pub mode: Mode,
    /// Which part of the catalog the run judged, as `K/N`, or nothing when it judged every one.
    pub shard: Option<String>,
}

/// The digest of the njutest executable at `program`, which is what every answer a run keeps was decided by.
///
/// # Errors
/// The file could not be read.
pub fn engine_of(program: &std::path::Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(program)?;
    let mut hasher = Sha256::new();
    let mut chunk = vec![0_u8; 1 << 16];
    loop {
        let read = std::io::Read::read(&mut file, &mut chunk)?;
        let Some(held) = chunk.get(..read).filter(|held| !held.is_empty()) else {
            return Ok(hex::encode(hasher.finalize()));
        };
        hasher.update(held);
    }
}

/// The run's identity: the number a later run compares its own against before believing anything an earlier one recorded.
#[must_use]
pub fn identity(inputs: &Inputs) -> String {
    let environment: std::collections::BTreeSet<&(String, String)> =
        inputs.environment.iter().collect();
    let mut fields = Fields::new(EVIDENCE_DOMAIN);
    fields
        .field("tree", &inputs.tree)
        .field("corpus", &inputs.corpus)
        .field("dependencies", &inputs.dependencies)
        .field("toolchain", &inputs.toolchain)
        .field("platform", &inputs.platform)
        .field("engine", &inputs.engine)
        .list(
            "environment",
            environment
                .iter()
                .map(|(name, value)| format!("{name}={value}")),
        )
        .field("contract", contract_name(inputs.contract))
        .field("configuration", &inputs.configuration)
        .list("test-args", &inputs.test_args)
        .field("mode", inputs.mode.name())
        .list("mode-detail", inputs.mode.detail())
        .field("shard", inputs.shard.as_deref().unwrap_or_default());
    fields.finish()
}

const fn contract_name(contract: Contract) -> &'static str {
    match contract {
        Contract::StandardV1 => "standard-v1",
        Contract::DeepV1 => "deep-v1",
        Contract::VerifiedV1 => "verified-v1",
        Contract::WholeV1 => "whole-v1",
    }
}
