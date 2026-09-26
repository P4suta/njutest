// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation that can reach a report says what a reader can do about it.

#![expect(
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]
use std::collections::BTreeMap;
use std::path::Path;

use njutest::assure::run::limitation_detail;

const GENERIC: &str = "stated by a phase of the run";

#[test]
fn every_limitation_the_engine_can_state_has_a_sentence_of_its_own() {
    let mut said: BTreeMap<String, &str> = BTreeMap::new();
    for name in rust_mutants::limitation::ALL {
        let detail = limitation_detail(name);
        assert_ne!(
            detail, GENERIC,
            "{name} reaches a report with nothing a reader can act on. A layer that \
             names itself and says nothing more is not one a person can audit, which \
             is what ADR 0004 decision 4 asks of every layer"
        );
        assert!(
            !detail.trim().is_empty(),
            "{name} reaches a report with an empty sentence, which is not the generic \
             one and is less than it: a reader is told the name of something that went \
             unmeasured and nothing whatever about it"
        );
        if let Some(other) = said.insert(detail.clone(), name) {
            panic!(
                "{name} and {other} say the same sentence, so a reader told either one \
                 learns which name it is and not which thing happened: {detail}"
            );
        }
    }
}

#[test]
fn a_limitation_that_names_its_target_is_still_looked_up_by_what_it_is() {
    let about = limitation_detail(&format!(
        "{}:core/lib/core",
        rust_mutants::limitation::BASELINE_NOT_PASSING
    ));
    assert_eq!(
        about,
        limitation_detail(rust_mutants::limitation::BASELINE_NOT_PASSING),
        "which target could not be measured is what a reader acts on, and the sentence \
         is about the limitation rather than about the target"
    );
    assert_ne!(about, GENERIC);
}

#[test]
fn a_name_from_a_later_engine_still_says_it_came_from_a_phase() {
    assert_eq!(
        limitation_detail("a limitation no release of this runner has ever seen"),
        GENERIC,
        "an unknown name is still reported rather than dropped: a limitation a reader \
         cannot look up is better than one they never hear about"
    );
}

#[test]
fn the_limitations_page_names_every_limitation_the_runner_can_state() {
    let text = std::fs::read_to_string(
        njutest_devkit::paths::workspace_root().join("docs/limitations.md"),
    )
    .expect("the limitations page");
    let missing: Vec<&str> = njutest::limitation::ALL
        .into_iter()
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "a limitation is what a run says when it could not establish something, and a \
         name a reader cannot look up is one they cannot act on. Six of these had \
         reached a report without ever reaching the page. {missing:?}"
    );
}

#[test]
fn the_limitations_page_names_every_skip_a_run_can_report() {
    let text = std::fs::read_to_string(
        njutest_devkit::paths::workspace_root().join("docs/limitations.md"),
    )
    .expect("the limitations page");
    let missing: Vec<String> = rust_mutants::syntax::SkipReason::ALL
        .into_iter()
        .map(|reason| format!("skipped-{}", reason.name()))
        .filter(|name| !text.contains(&format!("`{name}`")))
        .collect();
    assert!(
        missing.is_empty(),
        "a place the walk passed over reaches the report as a limitation of its own, one \
         row per reason, and the reason is in the name. The register the page test reads \
         holds none of them, so eighteen names a reader can meet are eighteen a reader \
         cannot look up. {missing:?}"
    );
}

#[test]
fn the_names_a_run_states_are_the_names_the_register_holds() {
    let root = njutest_devkit::paths::workspace_root().join("crates/njutest/src");
    let held: std::collections::BTreeSet<&str> = njutest::limitation::ALL.into_iter().collect();
    let mut loose = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory)
            .expect("the source")
            .map(|entry| entry.expect("every source entry is readable"))
        {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.file_name().is_some_and(|name| name == "limitation.rs")
                || path.extension().is_none_or(|kind| kind != "rs")
            {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            for line in source.lines() {
                if line.contains("LIMITATION: &str") || line.contains("_LIMITATION:") {
                    loose.push(format!("{}: {}", path.display(), line.trim()));
                }
            }
        }
    }
    assert!(
        loose.is_empty(),
        "a limitation named beside the code that states it is one the page test cannot \
         see, which is how six of them reached a report and no reader: {loose:#?}"
    );
    assert_eq!(
        held.len(),
        njutest::limitation::ALL.len(),
        "and no name is in the register twice"
    );
}

/// The limitations nothing puts to a run, and why.
const UNREACHED: [&str; 2] = [
    rust_mutants::limitation::COVERAGE_BUILD_FAILED,
    rust_mutants::limitation::COVERAGE_TOOLS_MISSING,
];

/// The name of the constant a limitation is held in, which is how a test usually names it.
fn shouted(name: &str) -> String {
    name.to_uppercase().replace('-', "_")
}

/// Every other name a limitation is exported under, which is how the module that states it names it.
fn aliases(root: &Path) -> BTreeMap<String, Vec<String>> {
    let mut found: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut pending = vec![root.join("crates")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.map(|entry| entry.expect("every source entry is readable")) {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|one| one != "rs") {
                continue;
            }
            for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
                let Some(rest) = line.trim().strip_prefix("pub use ") else {
                    continue;
                };
                let Some((path_part, alias)) = rest.trim_end_matches(';').split_once(" as ") else {
                    continue;
                };
                let Some(held) = path_part.rsplit("::").next() else {
                    continue;
                };
                if path_part.contains("limitation::") {
                    found
                        .entry(held.to_owned())
                        .or_default()
                        .push(alias.to_owned());
                }
            }
        }
    }
    found
}

#[test]
fn every_limitation_either_product_can_state_is_one_a_test_puts_to_something() {
    let root = njutest_devkit::paths::workspace_root();
    let mut suites = String::new();
    for crate_name in ["njutest", "rust-mutants-cli", "rust-mutants", "njutest"] {
        let directory = root.join("crates").join(crate_name).join("tests");
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.map(|entry| entry.expect("every suite entry is readable")) {
            if entry.path().extension().is_some_and(|one| one == "rs") {
                suites.push_str(&std::fs::read_to_string(entry.path()).unwrap_or_default());
                suites.push('\n');
            }
        }
    }
    assert!(
        suites.len() > 100_000,
        "the suites are read: {}",
        suites.len()
    );

    let every: Vec<&str> = rust_mutants::limitation::ALL
        .into_iter()
        .chain(njutest::limitation::ALL)
        .collect();
    let named = aliases(&root);
    let unheld: Vec<&&str> = every
        .iter()
        .filter(|name| !UNREACHED.contains(name))
        .filter(|name| {
            let held = shouted(name);
            let others = named.get(&held).map(Vec::as_slice).unwrap_or_default();
            !suites.contains(**name)
                && !suites.contains(&held)
                && !others.iter().any(|alias| suites.contains(alias))
        })
        .collect();
    assert!(
        unheld.is_empty(),
        "a limitation nothing puts to anything is a sentence a reader may never see and \
         nobody would know: the condition that states it can stop holding and every gate \
         stays green. {unheld:?} is named by no test. Put it to a run, or add it to \
         UNREACHED with the reason nothing can"
    );

    for named in UNREACHED {
        assert!(
            every.contains(&named),
            "{named} is on the list of what nothing reaches and is not a limitation \
             either product states any more: a waiver nobody needs is one that hides the \
             next real gap"
        );
    }
}

/// Every name a run may state: both registers, and the family a skip reason derives.
fn declared() -> std::collections::BTreeSet<String> {
    njutest::limitation::ALL
        .into_iter()
        .chain(rust_mutants::limitation::ALL)
        .map(ToOwned::to_owned)
        .chain(
            rust_mutants::syntax::SkipReason::ALL
                .into_iter()
                .map(|reason| format!("skipped-{}", reason.name())),
        )
        .collect()
}

/// Every string literal handed straight to `Limitation::new`, with the file it is in.
fn spelled_at_a_call_site() -> Vec<(String, String)> {
    let root = njutest_devkit::paths::workspace_root().join("crates/njutest");
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.map(|entry| entry.expect("every source entry is readable")) {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().is_none_or(|name| name != "target") {
                    pending.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|kind| kind != "rs") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap_or_default();
            let call = concat!("Limitation", "::new(");
            for part in source.split(call).skip(1) {
                let trimmed = part.trim_start();
                let Some(rest) = trimmed.strip_prefix('"') else {
                    continue;
                };
                let Some((name, _)) = rest.split_once('"') else {
                    continue;
                };
                found.push((path.display().to_string(), name.to_owned()));
            }
        }
    }
    found
}

#[test]
fn a_limitation_spelled_at_a_call_site_is_one_the_registers_hold() {
    let held = declared();
    let fabricated: Vec<(String, String)> = spelled_at_a_call_site()
        .into_iter()
        .filter(|(_, name)| !held.contains(name))
        .collect();
    assert!(
        fabricated.is_empty(),
        "a limitation name written as a literal beside the call rather than taken from \
         a register is one nothing holds to the page or to the sentence a reader looks \
         up, and two of these reached the committed report goldens under a name no run \
         can emit: {fabricated:#?}"
    );
}

/// The text of a register file as the tree holds it.
fn register_text(module: &str) -> String {
    std::fs::read_to_string(njutest_devkit::paths::workspace_root().join(module))
        .expect("the register")
}

/// Every limitation a register file declares, and every one its `ALL` names.
fn declared_and_registered(module: &str) -> (Vec<String>, Vec<String>) {
    registered(&register_text(module))
}

/// Every limitation the register `text` declares, and every one its `ALL` names.
fn registered(text: &str) -> (Vec<String>, Vec<String>) {
    (
        njutest_devkit::rust_source::public_text_constants(text)
            .expect("the register is Rust")
            .into_iter()
            .filter(|name| name != "ALL")
            .collect(),
        njutest_devkit::rust_source::names_listed(text, "ALL")
            .expect("the register names each limitation it holds in ALL"),
    )
}

#[test]
fn a_register_reads_the_same_in_the_tree_the_engine_instruments() {
    for module in [
        "crates/njutest/src/limitation.rs",
        "crates/rust-mutants/src/limitation.rs",
    ] {
        let text = register_text(module);
        let runtime = rust_mutants::instrument::render(&rust_mutants::instrument::Rendering {
            module: "__rust_mutants_limitation",
            catalog_digest: &"0".repeat(64),
            placements: &[],
            markers: &[],
            first_item: 0,
            item_count: 1,
            newline: "\n",
            watched: "/nowhere",
        })
        .expect("the engine renders the runtime it appends to an instrumented file");
        assert_eq!(
            registered(&format!("{text}{runtime}")),
            registered(&text),
            "{module}: when njutest measures itself this suite runs in the tree the engine \
             instrumented, where the file ends with the engine's runtime, and the register must \
             read the same there as here"
        );
    }
}

#[test]
fn every_limitation_a_register_declares_is_one_its_register_holds() {
    for module in [
        "crates/njutest/src/limitation.rs",
        "crates/rust-mutants/src/limitation.rs",
    ] {
        let (declared, held) = declared_and_registered(module);
        assert!(declared.len() > 10, "{module} declares them: {declared:?}");
        let missing: Vec<&String> = declared.iter().filter(|one| !held.contains(one)).collect();
        assert!(
            missing.is_empty(),
            "{module} declares a limitation its ALL does not name, and ALL is what the \
             page test, the put-to-something test and the register test all read: a name \
             left out of it reaches a report and no ledger whatever. {missing:?}"
        );
        let stale: Vec<&String> = held.iter().filter(|one| !declared.contains(one)).collect();
        assert!(stale.is_empty(), "{module}: {stale:?}");
    }
}
