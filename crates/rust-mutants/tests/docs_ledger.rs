// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every ledger the documentation keeps, against the code that is the ledger.
//!
//! A page that names a set the code also names is a page that goes stale
//! silently. These tests are the two-way set equalities that stop it: a name
//! the code adds and the page does not is a failure here rather than a reader
//! looking up something that is not there, and the reverse.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own pages are not themselves tests: a page \
              that cannot be read leaves nothing to assert"
)]

use std::collections::BTreeSet;

fn page(relative: &str) -> String {
    let path = mjutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{relative}: {error}"))
}

/// Every name in backticks on the lines of `text` a table row occupies.
fn named_in_table(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in text.lines().filter(|line| line.starts_with("| `")) {
        found.extend(backticked(line));
    }
    found
}

/// Every name in backticks in the first cell of each row: what the row is about, rather than what it says.
fn subjects(text: &str) -> BTreeSet<String> {
    text.lines()
        .filter(|line| line.starts_with("| `"))
        .filter_map(|line| line.get(1..)?.split('|').next())
        .flat_map(backticked)
        .collect()
}

/// Every backticked name of one line, splitting a cell that lists several with a slash.
fn backticked(line: &str) -> Vec<String> {
    let mut found = Vec::new();
    let mut rest = line;
    while let Some(open) = rest.find('`') {
        let after = rest.get(open.saturating_add(1)..).unwrap_or_default();
        let Some(close) = after.find('`') else {
            break;
        };
        let name = after.get(..close).unwrap_or_default();
        found.extend(
            name.split('/')
                .map(str::trim)
                .filter(|part| !part.is_empty())
                .map(str::to_owned),
        );
        rest = after.get(close.saturating_add(1)..).unwrap_or_default();
    }
    found
}

#[test]
fn the_trace_page_names_every_event_type_and_no_other() {
    let documented = subjects(&page("docs/engine/trace.md"));
    let in_code: BTreeSet<String> = rust_mutants::trace::EVERY_TYPE
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    let missing: Vec<&String> = in_code.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "docs/engine/trace.md does not name {missing:?}"
    );
    let extra: Vec<&String> = documented.difference(&in_code).collect();
    assert!(
        extra.is_empty(),
        "docs/engine/trace.md names {extra:?}, which the engine does not record"
    );
}

#[test]
fn the_architecture_page_names_every_skip_reason_and_no_other() {
    let text = page("docs/engine/architecture.md");
    let section = text
        .split("## Skips, stated")
        .nth(1)
        .expect("the skips section")
        .split("\n## ")
        .next()
        .expect("the end of it");
    let documented: BTreeSet<String> = section.lines().flat_map(backticked).collect();
    let in_code: BTreeSet<String> = rust_mutants::syntax::SkipReason::ALL
        .iter()
        .map(|reason| reason.name().to_owned())
        .collect();
    let missing: Vec<&String> = in_code.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "the Skips section does not name {missing:?}"
    );
}

#[test]
fn the_operators_page_names_every_rule_and_counts_them_as_the_table_does() {
    let text = page("docs/engine/operators.md");
    let documented = named_in_table(&text);
    let in_code: BTreeSet<String> = rust_mutants::rule::CANONICAL_TABLE
        .iter()
        .map(|rule| rule.name.to_owned())
        .collect();
    let missing: Vec<&String> = in_code.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "docs/engine/operators.md does not name {missing:?}"
    );
    let families: BTreeSet<String> = rust_mutants::rule::Family::ALL
        .iter()
        .map(|family| family.name().to_owned())
        .collect();
    let missing: Vec<&String> = families.difference(&documented).collect();
    assert!(
        missing.is_empty(),
        "docs/engine/operators.md does not name the families {missing:?}"
    );
    let counted = format!(
        "{} families, {} rules",
        spelled(rust_mutants::rule::CANONICAL_FAMILY_COUNT),
        spelled(rust_mutants::rule::CANONICAL_RULE_COUNT)
    );
    assert!(
        text.contains(&counted),
        "the page says something other than {counted:?}"
    );
}

/// The English for the two counts the operators page spells out.
fn spelled(count: usize) -> &'static str {
    match count {
        12 => "twelve",
        15 => "fifteen",
        51 => "fifty-one",
        69 => "sixty-nine",
        other => panic!("nobody has spelled {other} on the operators page yet"),
    }
}

#[test]
fn the_limitations_page_names_every_limitation_the_engine_can_state() {
    let text = page("docs/limitations.md");
    for limitation in rust_mutants::limitation::ALL {
        assert!(
            text.contains(&format!("`{limitation}`")),
            "docs/limitations.md does not name {limitation}, which a report can carry"
        );
    }
}

#[test]
fn every_engine_page_says_what_it_is_the_status_of() {
    let root = mjutest_devkit::paths::workspace_root();
    let mut without = Vec::new();
    for directory in ["docs", "docs/engine", "docs/adr"] {
        let Ok(entries) = std::fs::read_dir(root.join(directory)) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "md") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            if !text.contains("**Status:") && !text.contains("## Status") {
                without.push(format!(
                    "{directory}/{}",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
    }
    assert!(
        without.is_empty(),
        "these pages do not say whether what they describe is implemented: {without:?}"
    );
}
