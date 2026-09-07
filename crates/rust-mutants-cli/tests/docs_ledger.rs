// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledgers the command line's own pages keep, against the code that is the ledger.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own pages are not themselves tests: a page \
              that cannot be read leaves nothing to assert"
)]

use std::collections::BTreeSet;

fn page(relative: &str) -> String {
    let path = mjutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn every_finding_kind_is_named_on_the_page_that_documents_the_report() {
    let text = page("docs/engine/json-schema.md");
    for kind in rust_mutants_cli::run::FindingKind::ALL {
        assert!(
            text.contains(&format!("`{}`", kind.name())),
            "docs/engine/json-schema.md does not name {}, which a report can carry",
            kind.name()
        );
    }
}

#[test]
fn every_configuration_key_a_reader_may_write_is_on_the_configuration_page() {
    let text = page("docs/engine/configuration.md");
    let default = toml::to_string(&rust_mutants_cli::config::Config::default())
        .expect("the default configuration serialises");
    let mut missing = Vec::new();
    for line in default.lines() {
        let key = if let Some(section) = line.strip_prefix('[') {
            format!("[{section}")
        } else if let Some((name, _)) = line.split_once(' ') {
            name.to_owned()
        } else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        if !text.contains(&key) {
            missing.push(key);
        }
    }
    assert!(
        missing.is_empty(),
        "docs/engine/configuration.md does not document {missing:?}"
    );
}

#[test]
fn every_key_the_page_shows_is_one_the_reader_accepts() {
    let text = page("docs/engine/configuration.md");
    let skeleton: String = text
        .split("```toml")
        .nth(1)
        .expect("the skeleton")
        .split("```")
        .next()
        .expect("the end of it")
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim_end())
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");
    let parsed: Result<rust_mutants_cli::config::Config, _> = toml::from_str(&skeleton);
    assert!(
        parsed.is_ok(),
        "the skeleton on docs/engine/configuration.md is not one the reader accepts: {:?}\n{skeleton}",
        parsed.err()
    );
}

#[test]
fn the_engine_ledger_of_this_repository_is_one_the_reader_accepts() {
    let text = page(".rust-mutants.toml");
    let parsed: Result<rust_mutants_cli::config::Config, _> = toml::from_str(&text);
    assert!(
        parsed.is_ok(),
        "the repository's own ledger is not one the engine reads: {:?}",
        parsed.err()
    );
    let config = parsed.expect("the ledger");
    assert_eq!(
        config.project.packages,
        vec![
            "rust-mutants".to_owned(),
            "rust-mutants-cli".to_owned(),
            "xtask".to_owned(),
        ],
        "the ledger measures the engine, the command line it ships behind, and the audit that \
         re-decides its runs"
    );
    assert_eq!(
        config.mutation.tier,
        rust_mutants::rule::Tier::All,
        "a tool that asks a project for every operator asks itself for them too"
    );
    assert!(
        config.mutation.coverage,
        "the shipped proof layer is asked for"
    );
}

#[test]
fn the_exit_codes_the_page_documents_are_the_ones_the_run_returns() {
    let text = page("docs/engine/json-schema.md");
    let documented: BTreeSet<&str> = ["`0`", "`1`", "`2`", "`130`"]
        .into_iter()
        .filter(|code| text.contains(code))
        .collect();
    assert_eq!(documented.len(), 4, "the page leaves out an exit code");
    for code in [
        rust_mutants_cli::run::EXIT_DETECTED,
        rust_mutants_cli::run::EXIT_UNDETECTED,
        rust_mutants_cli::EXIT_USAGE,
        rust_mutants_cli::run::EXIT_INTERRUPTED,
    ] {
        assert!(
            documented.contains(&format!("`{code}`").as_str()),
            "the page does not document exit {code}"
        );
    }
}
