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
            "mjutest-cli".to_owned(),
            "xtask".to_owned(),
        ],
        "the ledger measures every package of this workspace: the engine, the command line it \
         ships behind, the runner built on it, and the audit that re-decides its runs. A \
         repository that says it measures itself and leaves half of itself out is one whose \
         scope does not match its claim"
    );
    assert_eq!(
        config.mutation.tier,
        rust_mutants::rule::Tier::All,
        "a tool that asks a project for every operator asks itself for them too"
    );
    assert!(
        config.mutation.touch,
        "the shipped proof layer is asked for: the guards, which cost the run nothing it was \
         not already spending"
    );
}

#[test]
fn the_exit_codes_the_page_documents_are_the_ones_the_run_returns() {
    let printed = rust_mutants_cli::exit_codes();
    let returned: BTreeSet<u8> = printed
        .lines()
        .skip(1)
        .filter_map(|line| line.split_whitespace().next())
        .filter_map(|code| code.parse().ok())
        .collect();
    assert_eq!(
        returned.len(),
        5,
        "every code a run can end with is in the table `--help` prints: {printed}"
    );

    for (page_name, anchor) in [
        (
            "docs/engine/json-schema.md",
            "`exit_code` is the one the process returned",
        ),
        (
            "docs/engine/command-line.md",
            "## What a run's exit code says",
        ),
    ] {
        let text = page(page_name);
        let from = text
            .find(anchor)
            .unwrap_or_else(|| panic!("{page_name} no longer says {anchor:?}"));
        let rest = text.get(from..).unwrap_or_default();
        let to = rest
            .get(anchor.len()..)
            .and_then(|after| after.find("\n## "));
        let table = to.map_or(rest, |end| {
            rest.get(..end.saturating_add(anchor.len())).unwrap_or(rest)
        });
        for code in &returned {
            assert!(
                table.contains(&format!("`{code}`")) || table.contains(&format!("| {code} |")),
                "{page_name} does not document exit {code}, which a run returns: a person \
                 whose script saw it has nowhere to look it up"
            );
        }
        for invented in [3u8, 4, 5, 101, 131, 142] {
            let named = table.contains(&format!("`{invented}`"))
                || table.contains(&format!("| {invented} |"));
            assert!(
                !named,
                "{page_name} documents exit {invented} and no run returns it, so a reader \
                 waits for an exit that never comes"
            );
        }
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

#[test]
fn every_schema_this_workspace_ships_is_one_a_test_holds_a_real_document_to() {
    let root = mjutest_devkit::paths::workspace_root();
    let entries =
        std::fs::read_dir(root.join("schema")).unwrap_or_else(|error| panic!("schema: {error}"));
    let shipped: BTreeSet<String> = entries
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| {
            std::path::Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
        })
        .collect();
    assert!(!shipped.is_empty(), "this workspace ships schemas");

    let mut suites = Vec::new();
    for crate_name in ["mjutest-cli", "rust-mutants-cli", "rust-mutants", "mjutest"] {
        let directory = root.join("crates").join(crate_name).join("tests");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|one| one == "rs") {
                suites.push(std::fs::read_to_string(entry.path()).unwrap_or_default());
            }
        }
    }
    assert!(suites.len() > 20, "the suites are read: {}", suites.len());

    let unheld: Vec<&String> = shipped
        .iter()
        .filter(|name| !suites.iter().any(|suite| suite.contains(name.as_str())))
        .collect();
    assert!(
        unheld.is_empty(),
        "a schema nothing validates a document against is a promise nobody keeps: a \
         reader writing a parser for it finds out what it really says from the first \
         document that will not fit. {unheld:?} is named by no test in this workspace"
    );
}

#[test]
fn every_finding_kind_a_report_can_carry_is_one_a_test_names() {
    use rust_mutants_cli::run::FindingKind;

    let suites = suites();
    let unnamed: Vec<&str> = FindingKind::ALL
        .into_iter()
        .filter(|kind| !suites.contains(kind.name()) && !suites.contains(&format!("{kind:?}")))
        .map(FindingKind::name)
        .collect();
    assert!(
        unnamed.is_empty(),
        "a kind no test names is one no run has ever been seen to report, and a kind no \
         run can report is a row in the reader's table that never appears — declared, \
         named, documented, and made by nothing: {unnamed:?}"
    );
}

/// Every test of this workspace, as one text.
fn suites() -> String {
    let root = mjutest_devkit::paths::workspace_root();
    let mut read = String::new();
    for crate_name in ["mjutest-cli", "rust-mutants-cli", "rust-mutants", "mjutest"] {
        let directory = root.join("crates").join(crate_name).join("tests");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|one| one == "rs") {
                read.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
                read.push('\n');
            }
        }
    }
    assert!(read.len() > 100_000, "the suites are read: {}", read.len());
    read
}
