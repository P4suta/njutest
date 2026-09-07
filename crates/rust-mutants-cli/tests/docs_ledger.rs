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

#[test]
fn every_schema_the_engine_ships_is_named_on_the_page_that_documents_them() {
    let directory = mjutest_devkit::paths::workspace_root().join("schema");
    let text = page("docs/engine/json-schema.md");
    let mut shipped = BTreeSet::new();
    let entries = std::fs::read_dir(&directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()));
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let json = std::path::Path::new(&name)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("json"));
        if name.starts_with("rust-mutants-") && json {
            shipped.insert(name);
        }
    }
    assert!(!shipped.is_empty(), "the engine ships schemas");
    let missing: Vec<&String> = shipped
        .iter()
        .filter(|name| !text.contains(&format!("schema/{name}")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/engine/json-schema.md names no schema/ path for {missing:?}"
    );

    let named: BTreeSet<String> = text
        .match_indices("schema/rust-mutants-")
        .filter_map(|(at, _)| {
            let rest = text.get(at.checked_add("schema/".len())?..)?;
            let end = rest.find(|character: char| {
                !character.is_ascii_alphanumeric() && character != '-' && character != '.'
            })?;
            rest.get(..end).map(ToOwned::to_owned)
        })
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .collect();
    let unshipped: Vec<&String> = named
        .iter()
        .filter(|name| !shipped.contains(*name))
        .collect();
    assert!(
        unshipped.is_empty(),
        "docs/engine/json-schema.md names schemas that are not under schema/: {unshipped:?}"
    );
}

#[test]
fn every_reason_a_mutant_did_not_run_is_one_the_schema_and_the_pages_name() {
    let schema = std::fs::read_to_string(
        mjutest_devkit::paths::workspace_root().join("schema/rust-mutants-run-report-v1.json"),
    )
    .unwrap_or_else(|error| panic!("the run report schema: {error}"));
    let started = page("docs/engine/getting-started.md");
    for reason in rust_mutants_cli::run::NotRunReason::ALL {
        let name = reason.name();
        assert_eq!(
            rust_mutants_cli::run::NotRunReason::parse(name),
            Some(reason),
            "{name} does not read back"
        );
        assert!(
            schema.contains(&format!("\"{name}\"")),
            "schema/rust-mutants-run-report-v1.json does not admit not_run_reason {name}"
        );
        assert!(
            started.contains(&format!("`{name}`")),
            "docs/engine/getting-started.md does not say what {name} means"
        );
    }
}

#[test]
fn the_url_a_sarif_result_sends_a_reader_to_is_this_project() {
    let manifest = std::fs::read_to_string(
        mjutest_devkit::paths::workspace_root().join("crates/rust-mutants-cli/Cargo.toml"),
    )
    .unwrap_or_else(|error| panic!("the manifest: {error}"));
    assert!(
        manifest.contains("repository.workspace = true") || manifest.contains("repository ="),
        "the crate takes its repository from somewhere: {manifest}"
    );
    let workspace =
        std::fs::read_to_string(mjutest_devkit::paths::workspace_root().join("Cargo.toml"))
            .unwrap_or_else(|error| panic!("the workspace manifest: {error}"));
    assert!(
        workspace.contains(rust_mutants_cli::report::sarif::INFORMATION),
        "a URL invented for a report is one nobody can follow: {}",
        rust_mutants_cli::report::sarif::INFORMATION
    );
}
