// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every code an xtask error carries is documented, and every documented `XT` code exists.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use xtask::error::ERROR_CODES;

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
    let declared: BTreeSet<String> = ERROR_CODES.iter().map(|c| c.code.to_owned()).collect();
    assert_eq!(
        declared,
        documented(),
        "docs/errors.md and xtask::error::ERROR_CODES disagree"
    );
}

#[test]
fn xtask_codes_are_unique_well_formed_and_sorted() {
    let codes: Vec<&str> = ERROR_CODES.iter().map(|c| c.code).collect();
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
    for code in ERROR_CODES {
        let row = format!("| `{}` | {} | {} |", code.code, code.meaning, code.remedy);
        assert!(
            text.lines().any(|line| line == row),
            "docs/errors.md lacks {row}"
        );
    }
}
