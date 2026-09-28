// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every code the sealed host can report is documented with the remedy it carries, and every documented `RS` code exists.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::collections::{BTreeMap, BTreeSet};

use rust_mutants_sealed::{ErrorCode, SealedCode, error_codes};

/// The rows of `docs/errors.md` whose code starts `RS`, by code, as their meaning and remedy.
fn documented() -> BTreeMap<String, (String, String)> {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md is readable");
    text.lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, rest) = cell.split_once("` | ")?;
            let (meaning, remedy) = rest.strip_suffix(" |")?.split_once(" | ")?;
            code.starts_with("RS")
                .then(|| (code.to_owned(), (meaning.to_owned(), remedy.to_owned())))
        })
        .collect()
}

#[test]
fn every_code_is_documented_and_every_documented_code_exists() {
    let declared: BTreeSet<String> = error_codes()
        .iter()
        .map(|code| code.code().to_owned())
        .collect();
    let documented: BTreeSet<String> = documented().into_keys().collect();
    assert!(!declared.is_empty(), "the sealed host declares its codes");
    assert_eq!(
        declared, documented,
        "docs/errors.md and rust_mutants_sealed::error_codes disagree"
    );
}

#[test]
fn codes_are_unique_well_formed_and_in_code_order() {
    let codes: Vec<&str> = error_codes().iter().map(ErrorCode::code).collect();
    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "codes are declared in code order");
    for code in &codes {
        assert!(
            code.len() == 6
                && code.starts_with("RS")
                && code
                    .chars()
                    .skip(2)
                    .all(|character| character.is_ascii_digit()),
            "malformed code {code}"
        );
    }
    assert_eq!(
        codes.len(),
        SealedCode::ALL.len(),
        "one code per failure mode"
    );
}

#[test]
fn every_row_says_what_the_code_says() {
    let documented = documented();
    let mut wrong = Vec::new();
    for code in error_codes() {
        let row = documented.get(code.code());
        let said = row.map(|(meaning, remedy)| (meaning.as_str(), remedy.as_str()));
        let carried = (capitalised(code.summary()), code.remedy());
        if said != Some((carried.0.as_str(), carried.1)) {
            wrong.push(format!(
                "{}: the table says {said:?}, the code carries {carried:?}",
                code.code()
            ));
        }
    }
    assert!(wrong.is_empty(), "{wrong:#?}");
}

#[test]
fn every_code_says_what_to_do_about_it() {
    for code in error_codes() {
        assert!(!code.remedy().is_empty(), "{} names no remedy", code.code());
    }
}

/// `summary` as a sentence: its first letter capitalised and a full stop after it.
fn capitalised(summary: &str) -> String {
    let mut characters = summary.chars();
    let first: String = characters
        .next()
        .map(|first| first.to_uppercase().collect())
        .unwrap_or_default();
    format!("{first}{}.", characters.as_str())
}
