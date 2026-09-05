// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every error code the runner can report is documented, and every
//! documented code exists.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use mjutest_cli::error::error_codes;

fn documented_codes(prefix: &str) -> BTreeSet<String> {
    let path = mjutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    text.lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, _) = cell.split_once('`')?;
            code.starts_with(prefix).then(|| code.to_owned())
        })
        .collect()
}

#[test]
fn every_runner_error_code_is_documented_and_every_documented_code_exists() {
    let in_code: BTreeSet<String> = error_codes().iter().map(|c| c.code.to_owned()).collect();
    assert!(
        !in_code.is_empty(),
        "the runner must declare its error codes"
    );
    assert_eq!(
        in_code,
        documented_codes("MJ"),
        "docs/errors.md and mjutest_cli::error::error_codes disagree"
    );
}

#[test]
fn error_codes_are_unique_well_formed_and_sorted() {
    let codes: Vec<&str> = error_codes().iter().map(|c| c.code).collect();
    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "codes are listed in code order");
    for code in &codes {
        assert!(
            code.len() == 6
                && code.starts_with("MJ")
                && code.chars().skip(2).all(|c| c.is_ascii_digit()),
            "malformed code {code}"
        );
    }
}

#[test]
fn every_variant_reports_a_declared_code() {
    let declared: BTreeSet<&str> = error_codes().iter().map(|c| c.code).collect();
    let config = mjutest_cli::config::Config::parse("version = 9\n", std::path::Path::new("x"))
        .expect_err("nine is not a version");
    let samples = [
        mjutest_cli::error::RunnerError::Interrupted,
        mjutest_cli::error::RunnerError::from(config),
    ];
    for sample in &samples {
        assert!(
            declared.contains(sample.code().code),
            "{sample:?} reports an undeclared code"
        );
    }
}

#[test]
fn every_configuration_failure_has_a_code_in_the_configuration_area() {
    for kind in mjutest_cli::config::ConfigErrorKind::ALL {
        assert!(
            kind.code().code.starts_with("MJ1"),
            "{kind:?} is not in the configuration area"
        );
        assert!(!kind.code().summary.is_empty());
    }
}
