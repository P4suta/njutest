// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a release is made of, as a bill of materials a reader's own tools understand.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The specification this document answers to.
pub const SPEC_VERSION: &str = "1.5";

/// The document's own kind.
pub const BOM_FORMAT: &str = "CycloneDX";

/// What a package's identity looks like in this ecosystem.
pub const PURL_PREFIX: &str = "pkg:cargo/";

/// One bill of materials.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Bom {
    /// The document's kind.
    #[serde(rename = "bomFormat")]
    pub format: String,
    /// The specification version.
    pub spec_version: String,
    /// This document's own version, which is one: it is written once per release.
    pub version: u32,
    /// What the document is about.
    pub metadata: Metadata,
    /// Every package the build resolved, in name and version order.
    pub components: Vec<Component>,
}

/// What the document is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    /// What wrote it.
    pub tools: Vec<Tool>,
    /// The thing being released.
    pub component: Component,
}

/// What wrote the document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// Who publishes it.
    pub vendor: String,
    /// What it is called.
    pub name: String,
    /// Which version of it.
    pub version: String,
}

/// One package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub struct Component {
    /// Always `library` here: what a release ships is built from libraries.
    #[serde(rename = "type")]
    pub kind: String,
    /// The package name.
    pub name: String,
    /// The package version.
    pub version: String,
    /// The package's identity, which another tool can look up.
    pub purl: String,
    /// What it may be used under, when the manifest says.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub licenses: Vec<License>,
}

/// One licence a package names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct License {
    /// The expression, verbatim from the manifest.
    pub expression: String,
}

/// The little of `cargo metadata` this needs.
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    #[serde(default)]
    workspace_members: Vec<String>,
    #[serde(flatten)]
    #[expect(
        dead_code,
        reason = "foreign protocol additions are retained for inspection"
    )]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct CargoPackage {
    id: String,
    name: String,
    version: String,
    #[serde(default)]
    license: Option<String>,
    #[serde(flatten)]
    #[expect(
        dead_code,
        reason = "foreign protocol additions are retained for inspection"
    )]
    external_fields: std::collections::BTreeMap<String, serde_json::Value>,
}

/// Why Cargo's metadata cannot be turned into a bill of materials.
#[derive(Debug, thiserror::Error)]
#[error("cargo metadata: {source}")]
pub struct SbomError {
    /// The metadata document was not the shape Cargo promises.
    #[source]
    source: serde_json::Error,
}

impl crate::error::Coded for SbomError {
    fn code(&self) -> crate::error::XtCode {
        crate::error::XtCode::SbomMetadata
    }
}

/// The bill of materials of the tree `metadata` describes, for the release named `about`.
///
/// # Errors
/// Returns what is wrong with the metadata document.
pub fn of(metadata: &str, about: (&str, &str)) -> Result<Bom, SbomError> {
    let (name, version) = about;
    let read: CargoMetadata =
        crate::strictjson::decode_str(metadata).map_err(|source| SbomError { source })?;
    let members: BTreeSet<&str> = read.workspace_members.iter().map(String::as_str).collect();
    let mut components: Vec<Component> = read
        .packages
        .iter()
        .filter(|package| !members.contains(package.id.as_str()))
        .map(component)
        .collect();
    components
        .sort_by(|left, right| (&left.name, &left.version).cmp(&(&right.name, &right.version)));
    components.dedup();
    Ok(Bom {
        format: BOM_FORMAT.to_owned(),
        spec_version: SPEC_VERSION.to_owned(),
        version: 1,
        metadata: Metadata {
            tools: vec![Tool {
                vendor: "njutest contributors".to_owned(),
                name: "cargo xtask sbom".to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
            }],
            component: Component {
                kind: "application".to_owned(),
                name: name.to_owned(),
                version: version.to_owned(),
                purl: format!("{PURL_PREFIX}{name}@{version}"),
                licenses: vec![License {
                    expression: "MIT OR Apache-2.0".to_owned(),
                }],
            },
        },
        components,
    })
}

fn component(package: &CargoPackage) -> Component {
    Component {
        kind: "library".to_owned(),
        name: package.name.clone(),
        version: package.version.clone(),
        purl: format!("{PURL_PREFIX}{}@{}", package.name, package.version),
        licenses: package
            .license
            .iter()
            .map(|expression| License {
                expression: expression.clone(),
            })
            .collect(),
    }
}
