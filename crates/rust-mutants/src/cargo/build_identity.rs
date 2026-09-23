// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical selection of the Cargo options this engine controls.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use super::BuildConfig;
use crate::id::HexDigest;

/// Domain separator for the canonical build-input digest.
pub const BUILD_SELECTION_DOMAIN: &str = "rust-mutants-build-selection-v1";

/// A canonical SHA-256 digest specifically naming the Cargo options selected by [`BuildConfig`].
///
/// This is nominally distinct from mutant, catalog, workspace, and cache digests even though all use the same lowercase-hex encoding.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct BuildSelectionDigest(HexDigest);

impl BuildSelectionDigest {
    /// The canonical lowercase spelling.
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }
}

impl std::fmt::Display for BuildSelectionDigest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl Serialize for BuildSelectionDigest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BuildSelectionDigest {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        HexDigest::try_from(value)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

/// Every [`BuildConfig`] field, in a canonical and self-authenticating form.
///
/// This is deliberately a *selection*, not a binary identity: a `None` target is resolved by the host and compiler/toolchain inputs are bound separately by run evidence.
///
/// The fields are private so callers cannot construct a digest that disagrees with the inputs it claims to bind.
/// Deserialization applies the same canonicalization checks as construction from [`BuildConfig`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct BuildSelection {
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
    profile: Option<String>,
    jobs: Option<u32>,
    debug: bool,
    digest: BuildSelectionDigest,
}

impl BuildSelection {
    fn from_fields(mut fields: BuildSelectionFields) -> Self {
        fields.features.sort();
        fields.features.dedup();
        let digest = derived_digest(&fields);
        let BuildSelectionFields {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
        } = fields;
        Self {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
            digest,
        }
    }

    /// The sorted, duplicate-free Cargo feature set.
    #[must_use]
    pub fn features(&self) -> &[String] {
        &self.features
    }

    /// Whether Cargo enables every feature.
    #[must_use]
    pub const fn all_features(&self) -> bool {
        self.all_features
    }

    /// Whether Cargo disables its default feature set.
    #[must_use]
    pub const fn no_default_features(&self) -> bool {
        self.no_default_features
    }

    /// The explicit target triple, or `None` for Cargo's host default.
    #[must_use]
    pub fn target(&self) -> Option<&str> {
        self.target.as_deref()
    }

    /// The explicit Cargo profile, or `None` for the command's default.
    #[must_use]
    pub fn profile(&self) -> Option<&str> {
        self.profile.as_deref()
    }

    /// The requested Cargo job count, or `None` for Cargo's choice.
    #[must_use]
    pub const fn jobs(&self) -> Option<u32> {
        self.jobs
    }

    /// Whether compiler debug information was requested.
    #[must_use]
    pub const fn debug(&self) -> bool {
        self.debug
    }

    /// The domain-separated canonical digest of all seven fields.
    #[must_use]
    pub const fn digest(&self) -> &BuildSelectionDigest {
        &self.digest
    }
}

impl BuildConfig {
    /// Captures every selected build field in canonical form.
    ///
    /// The exhaustive pattern is intentional: adding a field to [`BuildConfig`] is a compile error here until the cache, checkpoint,
    /// and report identity protocol decides how to bind it.
    #[must_use]
    pub fn selection(&self) -> BuildSelection {
        let Self {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
        } = self;
        BuildSelection::from_fields(BuildSelectionFields {
            features: features.clone(),
            all_features: *all_features,
            no_default_features: *no_default_features,
            target: target.clone(),
            profile: profile.clone(),
            jobs: *jobs,
            debug: *debug,
        })
    }
}

impl<'de> Deserialize<'de> for BuildSelection {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = BuildSelectionWire::deserialize(deserializer)?;
        Self::try_from(wire).map_err(serde::de::Error::custom)
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BuildSelectionWire {
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    target: Option<String>,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    profile: Option<String>,
    #[serde(deserialize_with = "crate::strictjson::required_option")]
    jobs: Option<u32>,
    debug: bool,
    digest: BuildSelectionDigest,
}

struct BuildSelectionFields {
    features: Vec<String>,
    all_features: bool,
    no_default_features: bool,
    target: Option<String>,
    profile: Option<String>,
    jobs: Option<u32>,
    debug: bool,
}

#[derive(Debug, thiserror::Error)]
enum BuildSelectionError {
    #[error("build features must be strictly sorted and duplicate-free")]
    FeaturesNotCanonical,
    #[error("build digest {actual} does not match canonical digest {expected}")]
    DigestMismatch {
        expected: BuildSelectionDigest,
        actual: BuildSelectionDigest,
    },
}

impl TryFrom<BuildSelectionWire> for BuildSelection {
    type Error = BuildSelectionError;

    fn try_from(wire: BuildSelectionWire) -> Result<Self, Self::Error> {
        let BuildSelectionWire {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
            digest,
        } = wire;
        if !features
            .iter()
            .zip(features.iter().skip(1))
            .all(|(left, right)| left < right)
        {
            return Err(BuildSelectionError::FeaturesNotCanonical);
        }
        let fields = BuildSelectionFields {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
        };
        let expected = derived_digest(&fields);
        if digest != expected {
            return Err(BuildSelectionError::DigestMismatch {
                expected,
                actual: digest,
            });
        }
        let BuildSelectionFields {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
        } = fields;
        Ok(Self {
            features,
            all_features,
            no_default_features,
            target,
            profile,
            jobs,
            debug,
            digest,
        })
    }
}

fn derived_digest(fields: &BuildSelectionFields) -> BuildSelectionDigest {
    let mut hasher = Sha256::new();
    field(&mut hasher, BUILD_SELECTION_DOMAIN);
    field(&mut hasher, "features");
    field(&mut hasher, &fields.features.len().to_string());
    for feature in &fields.features {
        field(&mut hasher, feature);
    }
    field(&mut hasher, "all-features");
    field(&mut hasher, boolean(fields.all_features));
    field(&mut hasher, "no-default-features");
    field(&mut hasher, boolean(fields.no_default_features));
    optional(&mut hasher, "target", fields.target.as_deref());
    optional(&mut hasher, "profile", fields.profile.as_deref());
    let jobs = fields.jobs.map(|value| value.to_string());
    optional(&mut hasher, "jobs", jobs.as_deref());
    field(&mut hasher, "debug");
    field(&mut hasher, boolean(fields.debug));
    BuildSelectionDigest(HexDigest::finish(hasher))
}

fn optional(hasher: &mut Sha256, name: &str, value: Option<&str>) {
    field(hasher, name);
    match value {
        Some(value) => {
            field(hasher, "some");
            field(hasher, value);
        }
        None => field(hasher, "none"),
    }
}

const fn boolean(value: bool) -> &'static str {
    if value { "true" } else { "false" }
}

/// Appends an architecture-independent decimal byte length, a separator, and the raw bytes.
/// The prefix makes adjacent fields injective without a fallible fixed-width conversion from `usize`.
fn field(hasher: &mut Sha256, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
}
