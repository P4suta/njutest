// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every xtask error carries a code, every code it carries is documented, and every documented `XT` code exists.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use xtask::error::XtCode;

fn documented() -> BTreeSet<String> {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    text.lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, _) = cell.split_once('`')?;
            code.starts_with("XT").then(|| code.to_owned())
        })
        .collect()
}

#[test]
fn every_xtask_code_is_documented_and_every_documented_code_exists() {
    let declared: BTreeSet<String> = XtCode::ALL.iter().map(|c| c.code().to_owned()).collect();
    assert_eq!(
        declared,
        documented(),
        "docs/errors.md and xtask::error::XtCode disagree"
    );
}

#[test]
fn xtask_codes_are_unique_well_formed_and_sorted() {
    let codes: Vec<&str> = XtCode::ALL.iter().map(|c| c.code()).collect();
    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "codes are listed in code order");
    for code in &codes {
        assert!(
            code.len() == 6
                && code.starts_with("XT")
                && code.chars().skip(2).all(|c| c.is_ascii_digit()),
            "malformed code {code}"
        );
    }
}

#[test]
fn every_documented_row_says_what_the_code_says() {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    for code in XtCode::ALL {
        let row = format!(
            "| `{}` | {} | {} |",
            code.code(),
            code.meaning(),
            code.remedy()
        );
        assert!(
            text.lines().any(|line| line == row),
            "docs/errors.md lacks {row}"
        );
    }
}

include!("support/uncoded.rs");

#[test]
fn an_error_type_with_no_code_is_found_by_its_derive_its_impl_or_its_name() {
    let specimen = vec![(
        "xtask/src/specimen.rs".to_owned(),
        "#[derive(Debug, thiserror::Error)]\n\
         pub enum PlantedError {\n    #[error(\"planted\")]\n    Planted,\n}\n\
         #[derive(Debug, thiserror::Error)]\n\
         #[error(\"contradicted\")]\n\
         pub struct Contradiction;\n\
         #[derive(Debug)]\n\
         pub(crate) enum NamedError {\n    Named,\n}\n\
         #[derive(Debug, thiserror::Error)]\n\
         pub enum CodedError {\n    #[error(\"coded\")]\n    Coded,\n}\n\
         impl crate::error::Coded for CodedError {\n    \
             fn code(&self) -> crate::error::XtCode {\n        \
                 crate::error::XtCode::GateRefused\n    }\n}\n\
         mod inner {\n    #[derive(Debug, thiserror::Error)]\n    \
             #[error(\"inner\")]\n    pub struct InnerError;\n}\n\
         #[derive(Debug)]\n\
         pub struct Failure;\n\
         impl std::fmt::Display for Failure {\n    \
             fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n        \
                 f.write_str(\"failed\")\n    }\n}\n\
         impl std::error::Error for Failure {}\n"
            .to_owned(),
    )];
    assert_eq!(
        uncoded(&specimen),
        vec![
            "xtask/src/specimen.rs: Contradiction",
            "xtask/src/specimen.rs: Failure",
            "xtask/src/specimen.rs: InnerError",
            "xtask/src/specimen.rs: NamedError",
            "xtask/src/specimen.rs: PlantedError",
        ],
        "an error is found by what it derives, by an `Error` impl written by hand, or by what it \
         is named, wherever in the file it is, and only the one given a code is left out"
    );
}
