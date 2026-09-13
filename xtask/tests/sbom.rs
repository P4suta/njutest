// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a release is made of: what the document says, and what it leaves out.

use xtask::sbom::{BOM_FORMAT, PURL_PREFIX, SPEC_VERSION, of};

const METADATA: &str = r#"{
  "packages": [
    {"id": "path+file:///w/crates/a#a@0.1.0", "name": "a", "version": "0.1.0", "license": "MIT OR Apache-2.0"},
    {"id": "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.2", "name": "serde", "version": "1.0.2", "license": "MIT OR Apache-2.0"},
    {"id": "registry+https://github.com/rust-lang/crates.io-index#unlicensed@0.3.0", "name": "unlicensed", "version": "0.3.0"}
  ],
  "workspace_members": ["path+file:///w/crates/a#a@0.1.0"]
}"#;

#[test]
fn the_document_says_what_it_is_and_what_it_is_about() {
    let bom = of(METADATA, ("njutest", "0.1.0")).expect("a bill of materials");
    assert_eq!(bom.format, BOM_FORMAT);
    assert_eq!(bom.spec_version, SPEC_VERSION);
    assert_eq!(bom.version, 1);
    assert_eq!(bom.metadata.component.name, "njutest");
    assert_eq!(bom.metadata.component.version, "0.1.0");
    assert_eq!(bom.metadata.component.purl, "pkg:cargo/njutest@0.1.0");
    assert_eq!(bom.metadata.component.kind, "application");
    assert_eq!(bom.metadata.tools.len(), 1);
}

#[test]
fn a_workspace_member_is_what_is_released_rather_than_what_it_is_made_of() {
    let bom = of(METADATA, ("njutest", "0.1.0")).expect("a bill of materials");
    let names: Vec<&str> = bom
        .components
        .iter()
        .map(|component| component.name.as_str())
        .collect();
    assert_eq!(names, ["serde", "unlicensed"]);
}

#[test]
fn every_component_carries_an_identity_another_tool_can_look_up() {
    let bom = of(METADATA, ("njutest", "0.1.0")).expect("a bill of materials");
    for component in &bom.components {
        assert_eq!(
            component.purl,
            format!("{PURL_PREFIX}{}@{}", component.name, component.version)
        );
        assert_eq!(component.kind, "library");
    }
}

#[test]
fn a_package_that_names_no_licence_is_listed_without_one_rather_than_with_a_guess() {
    let bom = of(METADATA, ("njutest", "0.1.0")).expect("a bill of materials");
    let serde = bom
        .components
        .iter()
        .find(|component| component.name == "serde")
        .expect("serde");
    assert_eq!(
        serde.licenses.first().map(|one| one.expression.clone()),
        Some("MIT OR Apache-2.0".to_owned())
    );
    let unlicensed = bom
        .components
        .iter()
        .find(|component| component.name == "unlicensed")
        .expect("the one with no licence");
    assert!(unlicensed.licenses.is_empty());
}

#[test]
fn metadata_that_is_not_metadata_is_refused() {
    of("not json", ("njutest", "0.1.0")).expect_err("not metadata");
    of("{}", ("njutest", "0.1.0")).expect_err("metadata without packages");
}
