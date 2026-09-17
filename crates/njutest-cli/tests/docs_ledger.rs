// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledgers the runner's own pages keep, against the code that is the ledger.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own pages are not themselves tests: a page \
              that cannot be read leaves nothing to assert"
)]

use njutest_cli::config::Config;
use njutest_cli::report::{Decision, FindingKind};

fn page(relative: &str) -> String {
    let path = njutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn every_way_a_run_can_choose_and_every_reason_it_widened_is_on_the_report_page() {
    let text = page("docs/report-v1.md");
    let mut words: Vec<&str> = rust_mutants::session::Route::GRANULARITIES.to_vec();
    for fallback in rust_mutants::session::Fallback::ALL {
        words.push(fallback.name());
    }
    let missing: Vec<&str> = words
        .into_iter()
        .filter(|word| !text.contains(&format!("`{word}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "a survivor is a claim about the targets a run chose, so the page names \
         every way it can choose and every reason it gave up narrowing. A word the \
         page does not carry is one a reader finds in a record and cannot look up: \
         {missing:?}"
    );
}

#[test]
fn the_ways_the_page_says_a_mutation_is_decided_are_the_ways_there_are() {
    let text = page("docs/report-v1.md");
    let listed: Vec<(String, Vec<String>)> = text
        .lines()
        .skip_while(|line| !line.starts_with("| column | what decided it |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| {
            let mut cells = line.split('|').skip(1);
            let column = cells.next()?.trim().trim_matches('`').to_owned();
            let outcomes = cells
                .nth(1)?
                .split(',')
                .map(|outcome| outcome.trim().trim_matches('`').to_owned())
                .collect();
            Some((column, outcomes))
        })
        .collect();

    let columns: Vec<&str> = listed.iter().map(|(column, _)| column.as_str()).collect();
    let ours: Vec<&str> = Decision::ALL.iter().map(|one| one.name()).collect();
    assert_eq!(
        columns, ours,
        "the page's table is the one a reader adds up to check the verdict, so it \
         holds one row per way a mutation can be decided, in the order the model \
         lists them"
    );

    let mut paged: Vec<(String, String)> = listed
        .iter()
        .flat_map(|(column, outcomes)| {
            outcomes
                .iter()
                .map(move |outcome| (outcome.clone(), column.clone()))
        })
        .collect();
    paged.sort();
    let mut ours: Vec<(String, String)> = Decision::OUTCOMES
        .iter()
        .map(|&(outcome, decision)| (outcome.to_owned(), decision.name().to_owned()))
        .collect();
    ours.sort();
    assert_eq!(
        paged, ours,
        "and every outcome a report can record is on exactly one row of it: an \
         outcome the page forgets is one a reader cannot tell the standing of, and \
         one it puts on two rows is one they would count twice"
    );
}

#[test]
fn the_kinds_the_page_lists_are_the_kinds_there_are_and_it_says_which_are_defects() {
    let text = page("docs/report-v1.md");
    let listed: Vec<(String, bool)> = text
        .lines()
        .skip_while(|line| !line.starts_with("| `kind` |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| {
            let mut cells = line.split('|').skip(1);
            let name = cells.next()?.trim().trim_matches('`').to_owned();
            let defect = cells.nth(1)?.trim() == "yes";
            Some((name, defect))
        })
        .collect();
    assert_eq!(
        listed.len(),
        FindingKind::ALL.len(),
        "the table of kinds is read and holds one row per kind: {listed:?}"
    );
    let mut wrong = Vec::new();
    for (name, defect) in listed {
        let Some(kind) = FindingKind::ALL.into_iter().find(|one| one.name() == name) else {
            wrong.push(format!("{name} is no kind a report can carry"));
            continue;
        };
        if kind.is_defect() != defect {
            wrong.push(format!(
                "{name}: the page says {}, the report says {}",
                if defect { "a defect" } else { "not a defect" },
                if kind.is_defect() {
                    "a defect"
                } else {
                    "not a defect"
                }
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the last column is the one a reader acts on first -- a defect is a fault in \
         their code and the rest are gaps in what was established -- and a verdict is \
         decided from the same answer. A page that says one and a run that says the \
         other sends a reader to the wrong half of their work: {wrong:?}"
    );
}

#[test]
fn a_kind_a_report_carries_is_the_name_it_is_written_under() {
    for kind in FindingKind::ALL {
        assert_eq!(
            serde_json::to_value(kind).ok(),
            Some(serde_json::Value::String(kind.name().to_owned())),
            "a kind is written into the report by one rule and read out of the code by \
             another, and the two are the same word or the page below documents a name \
             no report carries"
        );
    }
}

#[test]
fn every_finding_kind_is_named_on_the_page_that_documents_the_report() {
    let text = page("docs/report-v1.md");
    let missing: Vec<&str> = FindingKind::ALL
        .into_iter()
        .map(FindingKind::name)
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/report-v1.md does not name these, and a report can carry every one of \
         them: a consumer that meets a kind the page does not have has no way to learn \
         what it is claiming. {missing:?}"
    );
}

#[test]
fn every_configuration_key_a_reader_may_write_is_on_the_configuration_page() {
    let text = page("docs/configuration.md");
    let default = toml::to_string(&Config::default()).expect("the defaults serialise");
    let missing: Vec<String> = default
        .lines()
        .filter_map(|line| {
            line.strip_prefix('[').map_or_else(
                || line.split_once(' ').map(|(name, _rest)| name.to_owned()),
                |section| Some(format!("[{section}")),
            )
        })
        .filter(|key| !key.is_empty() && !documented(&text, key))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/configuration.md does not document {missing:?}, and a key nobody wrote \
         down is a key nobody sets"
    );
}

/// Whether the page shows `key`, which for a section of named tables is a table with a name in it.
fn documented(text: &str, key: &str) -> bool {
    text.contains(key)
        || key
            .strip_suffix(']')
            .is_some_and(|section| text.contains(&format!("{section}.")))
}

#[test]
fn every_key_the_configuration_page_shows_is_one_the_reader_accepts() {
    let text = page("docs/configuration.md");
    let skeleton: String = text
        .split("```toml")
        .nth(1)
        .expect("the skeleton the page shows")
        .split("```")
        .next()
        .expect("the end of it")
        .lines()
        .map(|line| line.split('#').next().unwrap_or_default().trim_end())
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<&str>>()
        .join("\n");
    let parsed: Result<Config, _> = toml::from_str(&skeleton);
    assert!(
        parsed.is_ok(),
        "a reader who copies what the page shows gets a file the run refuses, which is \
         the page teaching somebody to write a configuration that does not work: {:?}\n\
         {skeleton}",
        parsed.err()
    );
}

#[test]
fn the_exit_codes_the_page_lists_are_the_ones_a_run_can_carry() {
    let printed = njutest_cli::cli::exit_codes();
    let text = page("docs/report-v1.md");
    let table: String = text
        .lines()
        .skip_while(|line| !line.starts_with("| Code |"))
        .take_while(|line| line.starts_with('|'))
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(
        !table.is_empty(),
        "docs/report-v1.md has no exit code table"
    );

    for line in printed.lines().skip(1) {
        let (code, names) = line.trim().split_once(' ').expect("a code and its names");
        let listed = table
            .lines()
            .find(|row| row.starts_with(&format!("| {code} |")))
            .unwrap_or_else(|| panic!("docs/report-v1.md has no row for exit code {code}"));
        for name in names
            .split(',')
            .map(str::trim)
            .filter(|name| !name.is_empty())
        {
            let quoted = format!("`{name}`");
            assert!(
                listed.contains(&quoted) || listed.contains(name),
                "a run exits {code} carrying {name}, and the page's row for {code} does \
                 not say so. The help a person reads is printed from the verdicts \
                 themselves; a page written beside it is the copy that goes wrong: \
                 {listed}"
            );
        }
    }

    let named: Vec<&str> = table
        .lines()
        .flat_map(|row| row.split('`').skip(1).step_by(2))
        .collect();
    let invented: Vec<&&str> = named
        .iter()
        .filter(|name| !printed.contains(**name))
        .collect();
    assert!(
        invented.is_empty(),
        "and a name the page lists that no run carries is a promise nothing keeps: a \
         reader waits for an exit that never comes. {invented:?}"
    );
}

#[test]
fn every_shape_a_recording_can_hold_is_a_row_on_the_page_that_documents_it() {
    let text = page("docs/trace-v1.md");
    let missing: Vec<&str> = njutest_cli::testkit::every_payload()
        .iter()
        .map(njutest_cli::trace::Payload::type_name)
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "a recording is what an audit re-derives a run's proofs from, and a shape the \
         page does not list is one a reader meets with nothing to look it up by: \
         {missing:?}"
    );
}

#[test]
fn every_type_the_page_lists_is_a_shape_a_recording_can_hold() {
    let text = page("docs/trace-v1.md");
    let held: std::collections::BTreeSet<&str> = njutest_cli::testkit::every_payload()
        .iter()
        .map(njutest_cli::trace::Payload::type_name)
        .collect();
    let listed: Vec<String> = text
        .lines()
        .skip_while(|line| !line.starts_with("| Type |"))
        .skip(2)
        .take_while(|line| line.starts_with('|'))
        .filter_map(|line| line.split('|').nth(1).map(str::trim))
        .flat_map(|cell| {
            cell.split(',')
                .map(|name| name.trim().trim_matches('`').to_owned())
                .collect::<Vec<String>>()
        })
        .collect();
    assert!(listed.len() >= 10, "the table of types is read: {listed:?}");
    let invented: Vec<&String> = listed
        .iter()
        .filter(|name| !held.contains(name.as_str()))
        .collect();
    assert!(
        invented.is_empty(),
        "a type the page lists that no recording can hold is a shape a reader waits for \
         and an audit looks for: the page is what says a recording is complete, so a row \
         nothing writes is a hole nobody sees. {invented:?}"
    );
}

#[test]
fn every_reason_a_route_can_give_for_believing_nothing_is_on_that_page_too() {
    let text = page("docs/trace-v1.md");
    let missing: Vec<&'static str> = njutest_cli::testkit::every_refusal()
        .iter()
        .map(njutest_cli::evidence::store::Refusal::name)
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "reuse is a layer, and a run that did the work again says which of these is why. \
         A word the page does not carry is one a reader counts and cannot name: \
         {missing:?}"
    );
}

#[test]
fn every_finding_kind_a_report_can_carry_is_one_a_test_names() {
    let unnamed: Vec<&str> = FindingKind::ALL
        .into_iter()
        .filter(|kind| {
            let suites = suites();
            !suites.contains(kind.name()) && !suites.contains(&format!("{kind:?}"))
        })
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
    let root = njutest_devkit::paths::workspace_root();
    let mut read = String::new();
    for crate_name in ["njutest-cli", "rust-mutants-cli", "rust-mutants", "njutest"] {
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
