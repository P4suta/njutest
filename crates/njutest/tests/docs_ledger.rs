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

use njutest::config::Config;
use njutest::report::{Decision, FindingKind};
use njutest_devkit::docs::{TraceSpecimen, table_count, trace_field_ledger};

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
    let text = page("docs/report-v1.md");
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
    let mut ours: Vec<(String, String)> = njutest::report::Outcome::ALL
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
    let default =
        toml::to_string(&njutest::testkit::documented_specimen()).expect("the specimen serialises");
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

/// Whether the page shows `key`, which for a section of named tables is a table with any name in it.
///
/// `[resources.api]` is not a key a reader copies: `resources` is theirs to name a thing inside, and the page shows one called something else.
/// What has to be documented is the section.
fn documented(text: &str, key: &str) -> bool {
    text.contains(key)
        || key
            .strip_suffix(']')
            .is_some_and(|section| text.contains(&format!("{section}.")))
        || named_table(key).is_some_and(|section| text.contains(&format!("[{section}.")))
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
    let printed = njutest::cli::exit_codes();
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
fn every_trace_type_and_its_serialized_fields_are_exactly_one_row_on_the_page() {
    let text = page("docs/trace-v1.md");
    let payloads = njutest::testkit::every_payload();
    let specimens: Vec<TraceSpecimen<'_, njutest::trace::Payload>> = payloads
        .iter()
        .map(|payload| {
            TraceSpecimen::new(payload, Some(njutest::testkit::payload_record_key(payload)))
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
fn every_reason_a_route_can_give_for_believing_nothing_is_on_that_page_too() {
    let text = page("docs/trace-v1.md");
    let missing: Vec<&'static str> = njutest::testkit::every_refusal()
        .iter()
        .map(njutest::evidence::store::Refusal::name)
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
    for crate_name in ["njutest", "rust-mutants-cli", "rust-mutants", "njutest"] {
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

/// The section of a `[section.name]` key, where the name is the reader's to choose.
fn named_table(key: &str) -> Option<&str> {
    let inside = key.strip_prefix('[')?.strip_suffix(']')?;
    let (section, name) = inside.split_once('.')?;
    (!section.is_empty() && !name.is_empty() && !name.contains('.')).then_some(section)
}

#[test]
fn the_configuration_the_ledger_walks_has_a_member_in_every_collection_it_holds() {
    let text =
        toml::to_string(&njutest::testkit::documented_specimen()).expect("the specimen serialises");
    let value: toml::Value = toml::from_str(&text).expect("what was just written reads back");
    let mut empty = Vec::new();
    nothing_in(&value, String::new(), &mut empty);
    empty.sort();
    assert!(
        empty.is_empty(),
        "the ledger below learns which keys a reader may write by walking this value, so a \
         collection with nothing in it hides every key of whatever goes in it: the eight of \
         a `[resources.*]` table, the three of `[generation]`, and the ten of an \
         `[[acceptance]]` entry were invisible for exactly that reason. {empty:?}"
    );
}

/// Every path of `value` that holds an empty table or array, which is a shape the walk above cannot see into.
fn nothing_in(value: &toml::Value, at: String, into: &mut Vec<String>) {
    match value {
        toml::Value::Table(table) => {
            if table.is_empty() {
                into.push(at);
                return;
            }
            for (key, held) in table {
                let under = if at.is_empty() {
                    key.clone()
                } else {
                    format!("{at}.{key}")
                };
                nothing_in(held, under, into);
            }
        }
        toml::Value::Array(array) => {
            if array.is_empty() {
                into.push(at);
                return;
            }
            for (index, held) in array.iter().enumerate() {
                nothing_in(held, format!("{at}[{index}]"), into);
            }
        }
        toml::Value::String(..)
        | toml::Value::Integer(..)
        | toml::Value::Float(..)
        | toml::Value::Boolean(..)
        | toml::Value::Datetime(..) => {}
    }
}

/// The wire name a value serialises to, which for a closed set is the only name it has.
fn wire<T: serde::Serialize>(value: &T) -> String {
    let rendered = serde_json::to_value(value).expect("a closed set serialises");
    match rendered {
        serde_json::Value::String(name) => name,
        other => panic!("a closed set renders as one string, not {other}"),
    }
}

/// Every wire name a closed set produces, in the order it declares them.
fn names<T: serde::Serialize>(all: &[T]) -> Vec<String> {
    all.iter().map(wire).collect()
}

#[test]
fn every_closed_set_the_schema_declares_is_one_this_release_produces() {
    const BRANCHES: [&str; 8] = [
        "/$defs/modelUncertainty/oneOf/1/properties/kind",
        "/$defs/modelUncertainty/oneOf/2/properties/kind",
        "/$defs/modelUncertainty/oneOf/3/properties/kind",
        "/$defs/modelUncertainty/oneOf/4/properties/kind",
        "/$defs/modelUncertainty/oneOf/5/properties/kind",
        "/$defs/modelUncertainty/oneOf/6/properties/kind",
        "/$defs/modelUncertainty/oneOf/7/properties/kind",
        "/$defs/modelUncertainty/oneOf/8/properties/kind",
    ];
    use njutest::report::{
        Blind, ModelAffirmative, ModelArtifactFailure, ModelConfiguration, ModelIneligibility,
        ModelProcessFailure, ModelPropertyStatus, ModelProtocol, ModelToolFailure, Outcome,
        TargetStatus,
    };
    use rust_mutants::session::{Fallback, Granularity, Proof};

    let schema = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../schema/njutest-assurance-report-v1.json"),
    )
    .expect("the schema this release validates every stored report against");

    let outcomes = names(&Outcome::ALL);
    let decided = njutest::report::Decided::every_against("0123456789abcdef");
    let counted: Vec<String> = vec![wire(&Outcome::StepLimitReached)];
    let settled: Vec<String> = decided
        .iter()
        .filter(|one| one.decided_by().is_some() && one.outcome() != Outcome::StepLimitReached)
        .map(|one| wire(&one.outcome()))
        .collect();
    let unrouted: Vec<String> = decided
        .iter()
        .filter(|one| one.decided_by().is_none())
        .map(|one| wire(&one.outcome()))
        .collect();
    assert_eq!(
        settled.len() + counted.len() + unrouted.len(),
        outcomes.len(),
        "the schema splits a decision three ways by what it may carry beside the outcome, \
         and those three are that split: every outcome this release produces is in exactly \
         one of them, so a new one is a branch somebody has to place"
    );

    let mut tags: Vec<String> = njutest::testkit::every_model_uncertainty()
        .iter()
        .map(|one| {
            let rendered = serde_json::to_value(one).expect("an uncertainty serialises");
            match rendered.get("kind") {
                Some(serde_json::Value::String(kind)) => kind.clone(),
                other => panic!("an uncertainty is tagged by one name, not {other:?}"),
            }
        })
        .collect();
    let shared = tags.split_off(UNCERTAIN_KINDS.len());
    assert_eq!(
        tags, UNCERTAIN_KINDS,
        "the three the schema puts in one branch are the three that carry nothing"
    );
    assert_eq!(
        shared.len(),
        8,
        "and each of the rest is a branch of its own: {shared:?}"
    );

    let mut rows: Vec<(&str, Vec<String>)> = vec![
        (
            "/properties/document_type",
            DOCUMENT_TYPES.iter().map(|one| (*one).to_owned()).collect(),
        ),
        (
            "/$defs/candidates/items/properties/kind",
            njutest::repair::Kind::ALL
                .iter()
                .map(|one| (*one).name().to_owned())
                .collect(),
        ),
        (
            "/$defs/seam/properties/rule",
            names(&njutest::wire::rule::Rule::ALL),
        ),
        ("/$defs/target/properties/status", names(&TargetStatus::ALL)),
        ("/$defs/finding/properties/kind", names(&FindingKind::ALL)),
        ("/$defs/blindIn/properties/decision", names(&Blind::ALL)),
        ("/$defs/discharged/properties/proof", names(&Proof::ALL)),
        (
            "/$defs/routing/properties/granularity",
            names(&Granularity::ALL),
        ),
        (
            "/$defs/routing/properties/fallback/oneOf/0",
            names(&Fallback::ALL),
        ),
        ("/$defs/answered/properties/outcome", outcomes),
        ("/$defs/mutantDecision/oneOf/0/properties/outcome", unrouted),
        ("/$defs/mutantDecision/oneOf/1/properties/outcome", settled),
        ("/$defs/mutantDecision/oneOf/2/properties/outcome", counted),
        (
            "/$defs/mutant/allOf/0/then/properties/decision/properties/outcome",
            Outcome::ALL
                .into_iter()
                .filter(|one| one.review_answerable())
                .map(|one| wire(&one))
                .collect(),
        ),
        (
            "/$defs/completeReport/properties/contract",
            names(&njutest::config::Contract::ALL),
        ),
        (
            "/$defs/shardReport/properties/contract",
            names(&njutest::config::Contract::ALL),
        ),
        ("/$defs/completeReport/properties/run_kind", run_kinds()),
        ("/$defs/shardReport/properties/run_kind", run_kinds()),
        (
            "/$defs/modelAnswer/oneOf/0/properties/reason",
            names(&ModelIneligibility::ALL),
        ),
        (
            "/$defs/modelProcess/oneOf/1/properties/kind",
            process_kinds(),
        ),
        (
            "/$defs/modelUncertainty/oneOf/0/properties/kind",
            UNCERTAIN_KINDS
                .iter()
                .map(|one| (*one).to_owned())
                .collect(),
        ),
        (
            "/$defs/modelUncertainty/oneOf/1/properties/detail",
            names(&ModelConfiguration::ALL),
        ),
        (
            "/$defs/modelUncertainty/oneOf/2/properties/detail",
            names(&ModelToolFailure::ALL),
        ),
        (
            "/$defs/modelUncertainty/oneOf/3/properties/detail",
            names(&ModelProcessFailure::ALL),
        ),
        (
            "/$defs/modelUncertainty/oneOf/4/properties/detail",
            names(&ModelArtifactFailure::ALL),
        ),
        (
            "/$defs/modelUncertainty/oneOf/5/properties/detail/properties/expected",
            names(&ModelAffirmative::ALL),
        ),
        (
            "/$defs/modelUncertainty/oneOf/6/properties/detail",
            names(&ModelProtocol::ALL),
        ),
        (
            "/$defs/modelUncertainty/oneOf/7/properties/detail",
            names(&ModelPropertyStatus::ALL),
        ),
    ];
    for (pointer, tag) in BRANCHES.into_iter().zip(shared) {
        rows.push((pointer, vec![tag]));
    }
    rows.push((
        "/$defs/drift/oneOf/2/properties/why",
        names(&njutest::report::drift::Unmeasured::ALL),
    ));
    rows.push((
        "/$defs/beside/properties/failed",
        names(&njutest::report::faults::Failed::ALL),
    ));
    rows.push(("/$defs/knob", names(&njutest::report::knobs::Knob::ALL)));
    rows.push((
        "/$defs/knobStanding/oneOf/4/properties/why",
        names(&njutest::report::drift::Unmeasured::ALL),
    ));
    rows.push((
        "/$defs/knobStanding/oneOf/5/properties/why",
        names(&njutest::report::knobs::Unsettled::ALL),
    ));
    rows.push((
        "/$defs/knobStanding/oneOf/6/properties/why",
        names(&njutest::report::knobs::NotPut::ALL),
    ));
    rows.push((
        "/$defs/concurrencyStarts",
        names(&njutest::concurrency::scan::Starts::ALL),
    ));
    rows.push((
        "/$defs/concurrencyExploration/oneOf/0/properties/why",
        names(&njutest::report::concurrency::Unexplored::ALL),
    ));
    let borrowed: Vec<(&str, Vec<&str>)> = rows
        .iter()
        .map(|(pointer, names)| {
            (
                *pointer,
                names.iter().map(String::as_str).collect::<Vec<&str>>(),
            )
        })
        .collect();
    let expected: Vec<(&str, &[&str])> = borrowed
        .iter()
        .map(|(pointer, names)| (*pointer, names.as_slice()))
        .collect();
    if let Err(refusal) = njutest_devkit::docs::schema_enum_ledger(&schema, &expected) {
        panic!("{refusal}");
    }
}

/// What a run was narrowed to, by the type that decides it.
fn run_kinds() -> Vec<String> {
    use njutest::evidence::digest::Mode;

    let every = [
        Mode::Full,
        Mode::Changed {
            base: "HEAD~1".to_owned(),
        },
        Mode::Scoped {
            packages: vec!["demo".to_owned()],
        },
    ];
    for one in &every {
        match one {
            Mode::Full | Mode::Changed { .. } | Mode::Scoped { .. } => {}
        }
    }
    every.iter().map(|one| one.name().to_owned()).collect()
}

/// The two documents a run may store.
const DOCUMENT_TYPES: [&str; 2] = ["complete", "shard"];

/// The ends of a proof process that carry no exit code, by the type that ends one.
fn process_kinds() -> Vec<String> {
    use njutest::report::ModelProcess;

    let every = [
        ModelProcess::NotRun,
        ModelProcess::Cutoff,
        ModelProcess::Cancelled,
        ModelProcess::Failed,
    ];
    for one in &[
        ModelProcess::NotRun,
        ModelProcess::Exited(0),
        ModelProcess::Cutoff,
        ModelProcess::Cancelled,
        ModelProcess::Failed,
    ] {
        match one {
            ModelProcess::NotRun
            | ModelProcess::Exited(..)
            | ModelProcess::Cutoff
            | ModelProcess::Cancelled
            | ModelProcess::Failed => {}
        }
    }
    every
        .iter()
        .map(|one| {
            let rendered = serde_json::to_value(one).expect("a process end serialises");
            match rendered.get("kind") {
                Some(serde_json::Value::String(kind)) => kind.clone(),
                other => panic!("a process end is tagged by one name, not {other:?}"),
            }
        })
        .collect()
}

/// Why a proof answered nothing.
const UNCERTAIN_KINDS: [&str; 3] = ["bound-exhausted", "cutoff", "cancelled"];

#[test]
fn the_sections_the_contract_says_a_specification_lists_are_the_ones_it_draws() {
    let text = page("docs/assurance-contract.md");
    let (_, after) = text
        .split_once("## What a specification says")
        .expect("the contract says what a specification says");
    let section = after.split("\n## ").next().unwrap_or_default();
    let documented: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        section
            .lines()
            .filter(|line| {
                line.starts_with("| ")
                    && !line.starts_with("| Section")
                    && !line.starts_with("| ---")
            })
            .map(|line| {
                let cells: Vec<&str> = line.trim_matches('|').split(" | ").map(str::trim).collect();
                let heading = cells.first().copied().unwrap_or_default().to_owned();
                let decisions = cells
                    .last()
                    .copied()
                    .unwrap_or_default()
                    .split(", ")
                    .map(|name| name.trim_matches('`').to_owned())
                    .collect();
                (heading, decisions)
            })
            .collect();
    let drawn: std::collections::BTreeMap<String, std::collections::BTreeSet<String>> =
        njutest::spec::Section::ALL
            .into_iter()
            .map(|section| {
                (
                    njutest::presentation::spec::heading(section).to_owned(),
                    njutest::report::Outcome::ALL
                        .into_iter()
                        .filter(|outcome| njutest::spec::Section::of(outcome.decision()) == section)
                        .map(|outcome| outcome.name().to_owned())
                        .collect(),
                )
            })
            .collect();
    assert_eq!(
        documented, drawn,
        "the contract's table of what a specification lists is the page's headings, each with \
         the decisions that land under it, in both directions: a decision the table puts in the \
         wrong row tells a reader a change stands where the page will not show it"
    );
}

#[test]
fn every_record_type_the_trace_schema_declares_has_a_specimen() {
    let schema: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&page("schema/njutest-trace-v1.json"))
            .expect("the trace schema is JSON");
    let declared: std::collections::BTreeSet<String> = schema
        .pointer("/properties/payload/oneOf")
        .and_then(serde_json::Value::as_array)
        .expect("the payload is one of a closed list of records")
        .iter()
        .filter_map(|arm| arm.pointer("/properties/type/const"))
        .filter_map(serde_json::Value::as_str)
        .map(str::to_owned)
        .collect();
    let specimens: std::collections::BTreeSet<String> = njutest::testkit::every_payload()
        .iter()
        .map(|payload| payload.type_name().to_owned())
        .collect();
    assert_eq!(
        declared, specimens,
        "the field ledger is compared against specimens, so a record type with none is a row \
         the page can leave out and a reader can never see checked"
    );
}

#[test]
fn every_fault_decision_is_one_the_schema_publishes_and_the_page_documents() {
    let schema: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&page("schema/njutest-assurance-report-v1.json"))
            .expect("the report schema is JSON");
    let published: std::collections::BTreeSet<&str> = schema
        .pointer("/$defs/faultDecision/oneOf")
        .and_then(serde_json::Value::as_array)
        .expect("a fault decision is one of a closed list")
        .iter()
        .filter_map(|arm| arm.pointer("/properties/decision/const"))
        .filter_map(serde_json::Value::as_str)
        .collect();
    let every = njutest::report::faults::FaultDecision::every();
    let produced: std::collections::BTreeSet<&str> = every
        .iter()
        .map(njutest::report::faults::FaultDecision::name)
        .collect();
    assert_eq!(
        published, produced,
        "the schema and the set it publishes are one list"
    );
    let text = page("docs/report-v1.md");
    let undocumented: Vec<&&str> = produced
        .iter()
        .filter(|name| !text.contains(&format!("| `{name}` |")))
        .collect();
    assert!(
        undocumented.is_empty(),
        "docs/report-v1.md has no row for {undocumented:?}"
    );
    for decision in &every {
        let wire = serde_json::to_value(decision).expect("a decision serialises");
        assert_eq!(
            wire.get("decision").and_then(serde_json::Value::as_str),
            Some(decision.name()),
            "the wire tag is the name"
        );
    }
}

#[test]
fn every_crash_decision_is_one_the_schema_publishes_and_the_page_documents() {
    let schema: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&page("schema/njutest-assurance-report-v1.json"))
            .expect("the report schema is JSON");
    let published: std::collections::BTreeSet<&str> = schema
        .pointer("/$defs/crashDecision/oneOf")
        .and_then(serde_json::Value::as_array)
        .expect("a crash decision is one of a closed list")
        .iter()
        .filter_map(|arm| arm.pointer("/properties/decision/const"))
        .filter_map(serde_json::Value::as_str)
        .collect();
    let every = njutest::report::crashes::CrashDecision::every();
    let produced: std::collections::BTreeSet<&str> = every
        .iter()
        .map(njutest::report::crashes::CrashDecision::name)
        .collect();
    assert_eq!(
        published, produced,
        "the schema and the set it publishes are one list"
    );
    let text = page("docs/report-v1.md");
    let undocumented: Vec<&&str> = produced
        .iter()
        .filter(|name| !text.contains(&format!("| `{name}` |")))
        .collect();
    assert!(
        undocumented.is_empty(),
        "docs/report-v1.md has no row for {undocumented:?}"
    );
    for decision in &every {
        let wire = serde_json::to_value(decision).expect("a decision serialises");
        assert_eq!(
            wire.get("decision").and_then(serde_json::Value::as_str),
            Some(decision.name()),
            "the wire tag is the name"
        );
    }
}
