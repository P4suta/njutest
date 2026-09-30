// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A receipt of a sealed mutation run holds a registry decision only while it is fresh, sealed and complete.

#![expect(
    clippy::expect_used,
    reason = "a scratch tree that cannot be laid leaves nothing to test"
)]

use std::path::Path;

use sha2::{Digest as _, Sha256};
use xtask::receipt::{Execution, Mutant, Receipt, SCHEMA, file_name, held};

const MODULE: &str = "crates/app/src/decide.rs";
const SOURCE: &str = "pub fn decide(one: u8) -> bool { one > 1 }\n";
const NAME: &str = "xtask/receipts/decide.json";
const NAMED: &str = "xtask/receipts/decide-adapt.json";

fn killed(display_id: &str) -> Mutant {
    Mutant {
        display_id: display_id.to_owned(),
        position: "1:34".to_owned(),
        rule: "gt-to-ge".to_owned(),
        outcome: "killed".to_owned(),
        accepted: false,
        executions: vec![Execution {
            target: "app/lib/app".to_owned(),
            test: "tests::decides".to_owned(),
            came_to: "panicked".to_owned(),
        }],
    }
}

fn receipt(mutants: Vec<Mutant>) -> Receipt {
    Receipt {
        schema: SCHEMA.to_owned(),
        decision: "decide".to_owned(),
        package: "app".to_owned(),
        module: MODULE.to_owned(),
        source_sha256: hex::encode(Sha256::digest(SOURCE.as_bytes())),
        mutants,
    }
}

fn laid(receipt: &Receipt, source: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("a root");
    let module = root.path().join(MODULE);
    std::fs::create_dir_all(module.parent().expect("a directory")).expect("mkdir");
    std::fs::write(&module, source).expect("the module");
    let path = root.path().join(NAME);
    std::fs::create_dir_all(path.parent().expect("a directory")).expect("mkdir");
    std::fs::write(&path, serde_json::to_string_pretty(receipt).expect("JSON")).expect("receipt");
    root
}

fn refusal(root: &Path) -> String {
    match held(root, NAME, "decide") {
        Ok(_) => String::new(),
        Err(refused) => refused.0,
    }
}

#[test]
fn a_receipt_whose_every_mutant_a_sealed_execution_killed_holds_its_decision() {
    let root = laid(&receipt(vec![killed("aaaa")]), SOURCE);
    assert_eq!(
        refusal(root.path()),
        "",
        "a fresh, sealed, complete receipt holds"
    );
}

#[test]
fn a_receipt_that_does_not_hold_says_why_for_each_way_it_can_fail() {
    let mut survivor = killed("bbbb");
    survivor.outcome = "survived".to_owned();
    for execution in &mut survivor.executions {
        execution.came_to = "passed".to_owned();
    }
    let mut lead = killed("cccc");
    lead.executions.clear();
    let mut accepted = survivor.clone();
    accepted.accepted = true;
    for (why, root, words) in [
        (
            "a module changed since its run measured it",
            laid(
                &receipt(vec![killed("aaaa")]),
                "pub fn decide(one: u8) -> bool { one >= 1 }\n",
            ),
            "changed since its run measured it",
        ),
        (
            "a survivor no claim accepts",
            laid(&receipt(vec![killed("aaaa"), survivor]), SOURCE),
            "mutant bbbb",
        ),
        (
            "a kill no sealed execution detected",
            laid(&receipt(vec![lead]), SOURCE),
            "mutant cccc",
        ),
        (
            "a run that cataloged nothing",
            laid(&receipt(Vec::new()), SOURCE),
            "cataloged no mutant",
        ),
    ] {
        let said = refusal(root.path());
        assert!(said.contains(words), "{why}: {said:?}");
    }
    let root = laid(&receipt(vec![killed("aaaa"), accepted]), SOURCE);
    assert_eq!(
        refusal(root.path()),
        "",
        "a survivor a claim accepts is accounted for"
    );
    let root = laid(&receipt(vec![killed("aaaa")]), SOURCE);
    assert!(
        match held(root.path(), NAME, "another") {
            Ok(_) => false,
            Err(refused) => refused.0.contains("the registry names it for"),
        },
        "a receipt holds only the decision it was written for"
    );
}

#[test]
fn a_receipt_is_kept_under_a_name_that_says_which_decision_it_holds() {
    assert!(matches!(
        file_name("decide", None).as_deref(),
        Ok("decide.json")
    ));
    assert!(matches!(
        file_name("decide", Some("decide")).as_deref(),
        Ok("decide.json")
    ));
    assert!(
        matches!(
            file_name("decide", Some("decide-adapt")).as_deref(),
            Ok("decide-adapt.json")
        ),
        "a decision resting on several modules keeps a receipt for each"
    );
    for name in [
        "other",
        "decide-",
        "decide-Adapt",
        "decided",
        "decide-a.b",
        "adapt-decide",
    ] {
        assert!(
            file_name("decide", Some(name)).is_err(),
            "{name:?} does not say it holds `decide`"
        );
    }
}

#[test]
fn a_stale_receipt_under_its_own_name_says_how_to_write_it_again() {
    let root = laid(&receipt(vec![killed("aaaa")]), SOURCE);
    std::fs::rename(root.path().join(NAME), root.path().join(NAMED)).expect("renamed");
    std::fs::write(root.path().join(MODULE), "pub fn decide() {}\n").expect("the module changes");
    let said = match held(root.path(), NAMED, "decide") {
        Ok(_) => String::new(),
        Err(refused) => refused.0,
    };
    assert!(
        said.contains("--package app --name decide-adapt"),
        "the command to run again names the receipt: {said:?}"
    );
    let root = laid(&receipt(vec![killed("aaaa")]), SOURCE);
    std::fs::write(root.path().join(MODULE), "pub fn decide() {}\n").expect("the module changes");
    assert!(
        refusal(root.path()).contains("--package app` again"),
        "a receipt under the decision's own name needs no name to run again"
    );
}
