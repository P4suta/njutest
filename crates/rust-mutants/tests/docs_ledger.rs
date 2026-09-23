// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every ledger the documentation keeps, against the code that is the ledger.

use std::collections::BTreeSet;

use njutest_devkit::docs::{TraceSpecimen, table_count, trace_field_ledger};

fn page(relative: &str) -> std::io::Result<String> {
    let path = njutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(path)
}

/// Every name in backticks on the lines of `text` a table row occupies.
fn named_in_table(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in text.lines().filter(|line| line.starts_with("| `")) {
        found.extend(backticked(line));
    }
    found
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
fn every_trace_type_and_its_serialized_fields_are_exactly_one_row_on_the_page() {
    let text = page("docs/engine/trace.md");
    assert!(text.is_ok(), "the trace page is readable: {text:?}");
    let Ok(text) = text else { return };
    let payloads = rust_mutants::testkit::trace::every_payload();
    let specimens: Vec<TraceSpecimen<'_, rust_mutants::trace::Payload>> = payloads
        .iter()
        .map(|payload| {
            TraceSpecimen::new(payload, rust_mutants::testkit::trace::record_key(payload))
        })
        .collect();
    let checked = trace_field_ledger(&text, "| Type | Fields | Records |", &specimens);
    assert!(
        checked.is_ok(),
        "a trace row is the wire vocabulary a reader implements; missing, extra or repeated \
         names let the page and a recording describe different objects: {checked:?}"
    );
}

#[test]
fn the_architecture_page_names_every_skip_reason_and_no_other() {
    let text = page("docs/engine/architecture.md");
    assert!(text.is_ok(), "the architecture page is readable: {text:?}");
    let Ok(text) = text else { return };
    let section = text.split("## Skips, stated").nth(1);
    assert!(section.is_some(), "the skips section exists");
    let Some(section) = section else { return };
    let section = section.split("\n## ").next();
    assert!(section.is_some(), "the end of the skips section exists");
    let Some(section) = section else { return };
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
fn the_catalog_schema_accepts_exactly_every_compiler_named_skip_reason() {
    let source = page("schema/rust-mutants-catalog-v1.json");
    assert!(source.is_ok(), "the catalog schema is readable: {source:?}");
    let Ok(source) = source else { return };
    let schema = njutest_devkit::strictjson::decode_str::<serde_json::Value>(&source);
    assert!(
        schema.is_ok(),
        "the catalog schema is strict JSON: {schema:?}"
    );
    let Ok(schema) = schema else { return };
    let declared = schema
        .pointer("/$defs/skip/properties/reason/enum")
        .and_then(serde_json::Value::as_array)
        .and_then(|values| {
            values
                .iter()
                .map(serde_json::Value::as_str)
                .collect::<Option<BTreeSet<_>>>()
        });
    assert!(
        declared.is_some(),
        "the catalog schema has no closed text enum for skip reasons"
    );
    let Some(declared) = declared else { return };
    let in_code = rust_mutants::syntax::SkipReason::ALL
        .iter()
        .map(|reason| reason.name())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        declared, in_code,
        "the wire schema and the compiler's exhaustive skip vocabulary drifted"
    );
}

#[test]
fn the_operators_page_names_every_rule_and_counts_them_as_the_table_does() {
    let text = page("docs/engine/operators.md");
    assert!(text.is_ok(), "the operators page is readable: {text:?}");
    let Ok(text) = text else { return };
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
    for (many, noun) in [
        (rust_mutants::rule::CANONICAL_FAMILY_COUNT, "families"),
        (rust_mutants::rule::CANONICAL_RULE_COUNT, "rules"),
    ] {
        let counted = table_count(&text, "| Family | Rules | Tier |", many, noun);
        assert!(counted.is_ok(), "{counted:?}");
    }
}

#[test]
fn the_limitations_page_names_every_limitation_the_engine_can_state() {
    let text = page("docs/limitations.md");
    assert!(text.is_ok(), "the limitations page is readable: {text:?}");
    let Ok(text) = text else { return };
    for limitation in rust_mutants::limitation::ALL {
        assert!(
            text.contains(&format!("`{limitation}`")),
            "docs/limitations.md does not name {limitation}, which a report can carry"
        );
    }
}

#[test]
fn every_engine_page_says_what_it_is_the_status_of() {
    let root = njutest_devkit::paths::workspace_root();
    let mut without = Vec::new();
    let contents_of_the_book = std::ffi::OsStr::new("SUMMARY.md");
    for directory in ["docs", "docs/engine", "docs/adr"] {
        let entries = std::fs::read_dir(root.join(directory));
        assert!(entries.is_ok(), "{directory} is readable: {entries:?}");
        let Ok(entries) = entries else { return };
        for entry in entries {
            assert!(entry.is_ok(), "documentation directory entry: {entry:?}");
            let Ok(entry) = entry else { return };
            let path = entry.path();
            if path.extension().is_none_or(|extension| extension != "md") {
                continue;
            }
            if entry.file_name() == contents_of_the_book {
                continue;
            }
            let text = std::fs::read_to_string(&path);
            assert!(text.is_ok(), "{} is readable: {text:?}", path.display());
            let Ok(text) = text else { return };
            if !text.contains("**Status:") && !text.contains("## Status") {
                let name = entry.file_name().into_string();
                assert!(name.is_ok(), "documentation names are exact UTF-8");
                let Ok(name) = name else { return };
                without.push(format!("{directory}/{name}"));
            }
        }
    }
    assert!(
        without.is_empty(),
        "these pages do not say whether what they describe is implemented: {without:?}"
    );
}

#[test]
fn the_configuration_page_names_every_variable_a_run_composes_and_no_other() {
    let text = page("docs/engine/configuration.md");
    assert!(text.is_ok(), "the configuration page is readable: {text:?}");
    let Ok(text) = text else { return };
    let section = text.split("## Reserved environment").nth(1);
    assert!(section.is_some(), "the reserved environment section exists");
    let Some(section) = section else { return };
    let section = section.split("\n## ").next();
    assert!(
        section.is_some(),
        "the end of the reserved environment section exists"
    );
    let Some(section) = section else { return };
    let documented: BTreeSet<String> = section
        .lines()
        .flat_map(backticked)
        .filter(|name| name.starts_with("RUST_MUTANTS_"))
        .collect();
    let in_code: BTreeSet<String> = rust_mutants::execute::RESERVED_ENV
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    assert_eq!(
        documented, in_code,
        "a variable a run composes that the page does not name is one a reader sets and loses \
         a run to, and one the page names that the code does not is one they leave set for \
         nothing"
    );
}
