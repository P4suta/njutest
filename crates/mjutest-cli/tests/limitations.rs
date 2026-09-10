// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every limitation that can reach a report says what a reader can do about it.

use mjutest_cli::assure::run::limitation_detail;

const GENERIC: &str = "stated by a phase of the run";

#[test]
fn every_limitation_the_engine_can_state_has_a_sentence_of_its_own() {
    let mut said: std::collections::BTreeMap<String, &str> = std::collections::BTreeMap::new();
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
        mjutest_devkit::paths::workspace_root().join("docs/limitations.md"),
    )
    .expect("the limitations page");
    let missing: Vec<&str> = mjutest_cli::limitation::ALL
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
fn the_names_a_run_states_are_the_names_the_register_holds() {
    let root = mjutest_devkit::paths::workspace_root().join("crates/mjutest-cli/src");
    let held: std::collections::BTreeSet<&str> = mjutest_cli::limitation::ALL.into_iter().collect();
    let mut loose = Vec::new();
    let mut stack = vec![root];
    while let Some(directory) = stack.pop() {
        for entry in std::fs::read_dir(&directory).expect("the source").flatten() {
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
        mjutest_cli::limitation::ALL.len(),
        "and no name is in the register twice"
    );
}
