// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ledgers the runner's own pages keep, against the code that is the ledger.

#![expect(
    clippy::expect_used,
    clippy::panic,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest_cli::config::Config;
use njutest_cli::report::{Decision, FindingKind};
use njutest_devkit::docs::{
    TraceSpecimen, frozen_trace_field_ledger, table_count, trace_field_ledger,
};

fn page(relative: &str) -> String {
    let path = njutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn a_count_a_page_states_is_read_from_the_paragraph_that_states_it() {
    let written = "An earlier paragraph about the three narrowings.\n\
                   \n\
                   A **finding** is a problem. There are eleven kinds, and a report\n\
                   carries the name rather than a number:\n\
                   \n\
                   | `kind` | what it says |\n\
                   | --- | --- |\n\
                   | `a` | b |\n";
    assert_eq!(
        table_count(written, "| `kind` |", 11, "kinds").map_err(|error| error.to_string()),
        Ok(())
    );
    assert!(
        table_count(written, "| `kind` |", 10, "kinds").is_err(),
        "a page that counts wrong is the whole reason this is read"
    );
    assert!(
        table_count(written, "| `kind` |", 3, "narrowings").is_err(),
        "only the paragraph directly above the table is this table's count, so a \
         number further up the page counting something else cannot answer for it"
    );
    assert!(
        table_count(written, "| nothing |", 11, "kinds").is_err(),
        "a marker that names no table is a ledger reading a page that moved"
    );
}

#[test]
fn every_way_a_run_can_choose_and_every_reason_it_widened_is_on_the_report_page() {
    let text = page("docs/report-v2.md");
    let missing: Vec<&str> = rust_mutants::session::Granularity::ALL
        .iter()
        .map(|one| one.name())
        .chain(
            rust_mutants::session::Fallback::ALL
                .iter()
                .map(|one| one.name()),
        )
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
    let text = page("docs/report-v2.md");
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
    if let Err(why) = table_count(
        &text,
        "| column | what decided it |",
        Decision::ALL.len(),
        "columns",
    ) {
        panic!("{why}");
    }

    let mut paged: Vec<(String, String)> = listed
        .iter()
        .flat_map(|(column, outcomes)| {
            outcomes
                .iter()
                .map(move |outcome| (outcome.clone(), column.clone()))
        })
        .collect();
    paged.sort();
    let mut ours: Vec<(String, String)> = njutest_cli::report::Outcome::ALL
        .iter()
        .map(|one| (one.name().to_owned(), one.decision().name().to_owned()))
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
    let text = page("docs/report-v2.md");
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
    if let Err(why) = table_count(&text, "| `kind` |", FindingKind::ALL.len(), "kinds") {
        panic!("{why}");
    }
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
        let written = match serde_json::to_value(kind) {
            Ok(written) => Some(written),
            Err(_) => None,
        };
        assert_eq!(
            written,
            Some(serde_json::Value::String(kind.name().to_owned())),
            "a kind is written into the report by one rule and read out of the code by \
             another, and the two are the same word or the page below documents a name \
             no report carries"
        );
    }
}

#[test]
fn every_finding_kind_is_named_on_the_page_that_documents_the_report() {
    let text = page("docs/report-v2.md");
    let missing: Vec<&str> = FindingKind::ALL
        .into_iter()
        .map(FindingKind::name)
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/report-v2.md does not name these, and a report can carry every one of \
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
    let failure = match &parsed {
        Ok(_) => None,
        Err(error) => Some(error),
    };
    assert!(
        parsed.is_ok(),
        "a reader who copies what the page shows gets a file the run refuses, which is \
         the page teaching somebody to write a configuration that does not work: {:?}\n\
         {skeleton}",
        failure
    );
}

#[test]
fn the_exit_codes_the_page_lists_are_the_ones_a_run_can_carry() {
    let printed = njutest_cli::cli::exit_codes();
    let text = page("docs/report-v2.md");
    let table: String = text
        .lines()
        .skip_while(|line| !line.starts_with("| Code |"))
        .take_while(|line| line.starts_with('|'))
        .collect::<Vec<&str>>()
        .join("\n");
    assert!(
        !table.is_empty(),
        "docs/report-v2.md has no exit code table"
    );

    for line in printed.lines().skip(1) {
        let (code, names) = line.trim().split_once(' ').expect("a code and its names");
        let listed = table
            .lines()
            .find(|row| row.starts_with(&format!("| {code} |")))
            .unwrap_or_else(|| panic!("docs/report-v2.md has no row for exit code {code}"));
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
fn every_trace_type_and_its_serialized_fields_are_exactly_one_row_on_the_page() {
    let text = page("docs/trace-v2.md");
    let payloads = njutest_cli::testkit::every_payload();
    let specimens: Vec<TraceSpecimen<'_, njutest_cli::trace::Payload>> = payloads
        .iter()
        .map(|payload| {
            TraceSpecimen::new(
                payload,
                Some(njutest_cli::testkit::payload_record_key(payload)),
            )
        })
        .collect();
    if let Err(why) = trace_field_ledger(&text, "| Type | Fields | Records |", &specimens) {
        panic!(
            "a trace row is the wire vocabulary a reader implements; missing, extra or \
             repeated names let the page and a recording describe different objects: {why}"
        );
    }
}

#[test]
fn the_historical_v1_trace_table_cannot_be_rewritten_from_the_v2_types() {
    const FIELDS: &[(&str, &[&str])] = &[
        (
            "run-start",
            &[
                "schema",
                "njutest",
                "rust_mutants",
                "run_id",
                "run_kind",
                "contract",
            ],
        ),
        ("phase-start", &["name", "duration_ms"]),
        ("phase-end", &["name", "duration_ms"]),
        (
            "exec",
            &[
                "argv",
                "dir",
                "env_names",
                "timeout_ms",
                "exit_code",
                "timed_out",
                "duration_ms",
                "output_bytes",
                "output_sha256",
                "output_truncated",
                "output_path",
                "error",
            ],
        ),
        ("progress", &["message", "subject", "done", "total"]),
        ("artifact", &["kind", "path", "bytes"]),
        (
            "route",
            &[
                "mutant",
                "granularity",
                "fallback",
                "reaching",
                "tests",
                "discharged",
                "considered",
                "reused",
                "refused",
            ],
        ),
        (
            "mutant-exec",
            &[
                "mutant",
                "target",
                "args",
                "outcome",
                "duration_ms",
                "alone",
            ],
        ),
        ("probe-exec", &["target", "outcome", "infected"]),
        (
            "wire-exchange",
            &[
                "capability",
                "seq",
                "during",
                "duration_ms",
                "wire",
                "method",
                "path",
                "status",
                "request_bytes",
                "response_bytes",
            ],
        ),
        (
            "wire-exec",
            &[
                "fault",
                "capability",
                "seq",
                "rule",
                "decision",
                "noticed_by",
                "proof",
            ],
        ),
        ("note", &["kind", "detail"]),
        (
            "run-end",
            &[
                "verdict",
                "accounting",
                "error",
                "events_emitted",
                "events_dropped",
            ],
        ),
    ];
    let text = page("docs/trace-v1.md");
    if let Err(why) = frozen_trace_field_ledger(&text, "| Type | Fields | Records |", FIELDS) {
        panic!(
            "trace v1 describes already-written bytes, so a current type must not rewrite its \
             vocabulary: {why}"
        );
    }
}

#[test]
fn every_reason_a_route_can_give_for_believing_nothing_is_on_that_page_too() {
    let text = page("docs/trace-v2.md");
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
        for entry in entries.map(|entry| entry.expect("every source entry is readable")) {
            if entry.path().extension().is_some_and(|one| one == "rs") {
                read.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
                read.push('\n');
            }
        }
    }
    assert!(read.len() > 100_000, "the suites are read: {}", read.len());
    read
}
