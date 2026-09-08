// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! One run's identity. Two runs share it exactly when nothing that could change what the tests say has changed.

use rust_mutants::id::write_length_prefixed;
use sha2::{Digest as _, Sha256};

use crate::config::Contract;

/// The domain hashed first for a run's identity. The recipe version is in the name: a future recipe becomes `mjutest-evidence-v2` so a v1 identity can never be mistaken for a v2 one.
pub const EVIDENCE_DOMAIN: &str = "mjutest-evidence-v2";

/// A digest built from named fields. Every field is length-prefixed and preceded by its own name, so no two different lists of values can produce the same number.
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

    /// Adds a named list, in the order given. A caller for whom order is not a fact sorts first.
    pub fn list<I, S>(&mut self, name: &str, values: I) -> &mut Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        write(&mut self.hasher, name);
        let mut count = 0u32;
        let mut items = Sha256::new();
        for value in values {
            write(&mut items, value.as_ref());
            count = count.saturating_add(1);
        }
        write(&mut self.hasher, &count.to_string());
        write(&mut self.hasher, &hex::encode(items.finalize()));
        self
    }

    /// The digest, as sixty-four lowercase hex characters.
    #[must_use]
    pub fn finish(self) -> String {
        hex::encode(self.hasher.finalize())
    }
}

/// A field too long to length-prefix cannot occur: every value here is a digest, a version banner, a path, or an environment value, and none reaches four gigabytes. Should one ever, the digest absorbs a marker rather than silently dropping the value.
fn write(hasher: &mut Sha256, value: &str) {
    if write_length_prefixed(hasher, value).is_err() {
        hasher.update(b"\xffoverlong\xff");
        hasher.update(Sha256::digest(value.as_bytes()));
    }
}

/// How much of the workspace a run looked at. It is part of the identity: a run that looked at one package established less than one that looked at everything, and the two must never share a cached answer.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
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
        /// The packages, as named. The identity reads them sorted.
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
            Self::Scoped { packages } => {
                let mut sorted = packages.clone();
                sorted.sort_unstable();
                sorted.dedup();
                sorted
            }
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
    /// The environment the run selected, as names and values. Read in a fixed order.
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
    ///
    /// `mode` says how much of the tree was read; this says how much of the
    /// catalog was put to a test, which is a different axis. Without it a run
    /// that judged half a catalog and one that judged all of it have one
    /// identity, and the first is handed the second's answer.
    pub shard: Option<String>,
}

/// The run's identity: the number a later run compares its own against before believing anything an earlier one recorded.
#[must_use]
pub fn identity(inputs: &Inputs) -> String {
    let mut environment: Vec<(String, String)> = inputs.environment.clone();
    environment.sort();
    environment.dedup();
    let mut fields = Fields::new(EVIDENCE_DOMAIN);
    fields
        .field("tree", &inputs.tree)
        .field("corpus", &inputs.corpus)
        .field("dependencies", &inputs.dependencies)
        .field("toolchain", &inputs.toolchain)
        .field("platform", &inputs.platform)
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
    }
}
