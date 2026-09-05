// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every error code the runner can report is documented, and every documented
//! code exists.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, and asserts with panics"
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
fn every_variant_reports_a_declared_code() {
    let declared: BTreeSet<&str> = error_codes().iter().map(|c| c.code).collect();
    let samples = [mjutest_cli::error::RunnerError::Interrupted];
    for sample in &samples {
        assert!(
            declared.contains(sample.code().code),
            "{sample:?} reports an undeclared code"
        );
    }
}
