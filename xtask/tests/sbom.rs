// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a release is made of: what the document says, and what it leaves out.

use njutest_devkit::result::{OptionState, ResultState, option_state, result_state};
use xtask::sbom::{BOM_FORMAT, Bom, PURL_PREFIX, SPEC_VERSION, of};

const METADATA: &str = r#"{
  "packages": [
    {"id": "path+file:///w/crates/a#a@0.1.0", "name": "a", "version": "0.1.0", "license": "MIT OR Apache-2.0"},
    {"id": "registry+https://github.com/rust-lang/crates.io-index#serde@1.0.2", "name": "serde", "version": "1.0.2", "license": "MIT OR Apache-2.0"},
    {"id": "registry+https://github.com/rust-lang/crates.io-index#unlicensed@0.3.0", "name": "unlicensed", "version": "0.3.0"}
  ],
  "workspace_members": ["path+file:///w/crates/a#a@0.1.0"]
}"#;

fn bom() -> Option<Bom> {
    let document = of(METADATA, ("njutest", "0.1.0"));
    assert_eq!(
        result_state(&document),
        ResultState::Returned,
        "the literal Cargo metadata was refused: {document:?}"
    );
    match document {
        Ok(bom) => Some(bom),
        Err(_already_reported) => None,
    }
}

#[test]
fn the_document_says_what_it_is_and_what_it_is_about() {
    let Some(bom) = bom() else { return };
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
    let Some(bom) = bom() else { return };
    let names: Vec<&str> = bom
        .components
        .iter()
        .map(|component| component.name.as_str())
        .collect();
    assert_eq!(names, ["serde", "unlicensed"]);
}

#[test]
fn every_component_carries_an_identity_another_tool_can_look_up() {
    let Some(bom) = bom() else { return };
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
    let Some(bom) = bom() else { return };
    let serde = bom
        .components
        .iter()
        .find(|component| component.name == "serde");
    assert_eq!(
        option_state(serde),
        OptionState::Present,
        "serde is absent from the bill of materials"
    );
    let Some(serde) = serde else {
        return;
    };
    assert_eq!(
        serde.licenses.first().map(|one| one.expression.clone()),
        Some("MIT OR Apache-2.0".to_owned())
    );
    let unlicensed = bom
        .components
        .iter()
        .find(|component| component.name == "unlicensed");
    assert_eq!(
        option_state(unlicensed),
        OptionState::Present,
        "the unlicensed fixture package is absent"
    );
    let Some(unlicensed) = unlicensed else {
        return;
    };
    assert!(unlicensed.licenses.is_empty());
}

#[test]
fn metadata_that_is_not_metadata_is_refused() {
    for malformed in ["not json", "{}"] {
        let document = of(malformed, ("njutest", "0.1.0"));
        assert_eq!(
            result_state(&document),
            ResultState::Refused,
            "malformed metadata produced {document:?}"
        );
    }
}
