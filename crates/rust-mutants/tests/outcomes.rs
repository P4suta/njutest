// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a remembered outcome is filed under, and what makes it stop answering.

use njutest_devkit::result::{ResultState::Returned, result_state};

#[test]
fn a_key_over_nothing_is_not_a_key_and_remembers_nothing() {
    let keyed = rust_mutants::outcomes::Keyed {
        closure: String::new(),
        manifests: "m".to_owned(),
        toolchain: "rustc 1.98.0".to_owned(),
        args: Vec::new(),
        timeout: "auto".to_owned(),
        steps: 50_000_000,
        build: Vec::new(),
        engine: "e".to_owned(),
        runner: None,
    };
    assert!(
        !keyed.usable(),
        "a build whose dep-info could not be read names no closure, and a key over nothing would \
         file every mutant of every tree under one name"
    );
    let usable = rust_mutants::outcomes::Keyed {
        closure: "c".to_owned(),
        ..keyed
    };
    assert!(usable.usable());
    let unread = rust_mutants::outcomes::Keyed {
        engine: String::new(),
        ..usable
    };
    assert!(
        !unread.usable(),
        "an engine that could not be read names no build of it, and a verdict one build \
         remembered is only this one's answer while the two mean the same thing by it"
    );
}

#[test]
fn what_the_key_is_computed_from_is_what_could_change_the_answer() {
    let mutant = rust_mutants::id::HexDigest::try_from(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    );
    assert_eq!(result_state(&mutant), Returned, "canonical digest");
    let Ok(mutant) = mutant else { return };
    let another = rust_mutants::id::HexDigest::try_from(
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    );
    assert_eq!(result_state(&another), Returned, "canonical digest");
    let Ok(another) = another else { return };
    let base = rust_mutants::outcomes::Keyed {
        closure: "c".to_owned(),
        manifests: "m".to_owned(),
        toolchain: "rustc 1.98.0".to_owned(),
        args: vec!["--test-threads=1".to_owned()],
        timeout: "auto".to_owned(),
        steps: 50_000_000,
        build: vec!["--all-features".to_owned()],
        engine: "e".to_owned(),
        runner: None,
    };
    let key = base.key(&mutant);
    for other in [
        rust_mutants::outcomes::Keyed {
            closure: "other".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            manifests: "other".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            toolchain: "rustc 1.99.0".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            args: Vec::new(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            timeout: "30s".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            steps: 1_000_000,
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            build: Vec::new(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            engine: "another build".to_owned(),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            runner: Some("a runner's contract".to_owned()),
            ..base.clone()
        },
        rust_mutants::outcomes::Keyed {
            runner: Some("none".to_owned()),
            ..base.clone()
        },
    ] {
        assert_ne!(
            other.key(&mutant),
            key,
            "{other:?} names a different program and files under the same name"
        );
    }
    assert_ne!(base.key(&another), key, "and so does another mutant");
    assert_eq!(base.key(&mutant), key, "the same question is the same key");
}

#[test]
fn only_decided_test_outcomes_fit_in_a_cache_record() {
    use rust_mutants::outcome::Outcome;
    use rust_mutants::outcomes::{CacheOutcome, Record, SCHEMA};

    assert_eq!(Outcome::from(CacheOutcome::Killed), Outcome::Killed);
    assert_eq!(Outcome::from(CacheOutcome::Survived), Outcome::Survived);
    let undecided = serde_json::json!({
        "schema": SCHEMA,
        "mutant": "m",
        "outcome": "step_limit_reached",
        "target": "demo/lib/demo",
        "tests_run": null,
        "run_id": "run"
    });
    assert!(
        serde_json::from_value::<Record>(undecided).is_err(),
        "the wire type rejects a finite execution bound before it can become reusable evidence"
    );
}

#[test]
fn the_policy_break_has_a_new_schema_layout_and_cache_abi() {
    assert_eq!(rust_mutants::outcomes::SCHEMA, "rust-mutants-outcome-v2");
    assert_eq!(rust_mutants::outcomes::LAYOUT, "rust-mutants/outcomes-v2");
    assert_eq!(rust_mutants::outcomes::CACHE_ABI, 8);
    assert_eq!(rust_mutants::outcomes::INSTRUMENTATION_ABI, 3);
    assert_eq!(rust_mutants::outcomes::STEP_POLICY_ABI, 1);
}

#[test]
fn the_engine_is_named_by_what_it_is_and_not_by_where_it_lies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (one, same, other) = (
        dir.path().join("one"),
        dir.path().join("same"),
        dir.path().join("other"),
    );
    std::fs::write(&one, b"an engine").expect("write");
    std::fs::write(&same, b"an engine").expect("write");
    std::fs::write(&other, b"another engine").expect("write");
    let digest = |path: &std::path::Path| {
        rust_mutants::outcomes::engine_of(path).expect("a readable engine")
    };
    assert_eq!(digest(&one), digest(&same), "one build, two places");
    assert_ne!(
        digest(&one),
        digest(&other),
        "a rebuilt engine may mean something else by a verdict, so it files under another name"
    );
    assert!(
        rust_mutants::outcomes::engine_of(&dir.path().join("missing")).is_err(),
        "and one that cannot be read names nothing"
    );
}

#[test]
fn a_record_that_does_not_say_which_runner_asked_is_refused_rather_than_read_as_none() {
    let keyed = serde_json::json!({
        "closure": "c",
        "manifests": "m",
        "toolchain": "rustc 1.98.0",
        "args": [],
        "timeout": "auto",
        "steps": 0,
        "build": [],
        "engine": "e",
    });
    let absent = serde_json::from_value::<rust_mutants::outcomes::Keyed>(keyed.clone());
    assert!(
        absent.is_err(),
        "a record with no `runner` is one written before runners were keyed, or a hand-made one, \
         and reading it as the engine's own question would answer a runner's with it: {absent:?}"
    );
    let mut said = keyed;
    if let Some(object) = said.as_object_mut() {
        object.insert("runner".to_owned(), serde_json::Value::Null);
    }
    let null = serde_json::from_value::<rust_mutants::outcomes::Keyed>(said);
    assert!(
        null.is_ok_and(|keyed| keyed.runner.is_none()),
        "`null` says the engine asked"
    );
}
