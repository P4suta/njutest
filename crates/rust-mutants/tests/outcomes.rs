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
    assert_eq!(rust_mutants::outcomes::SCHEMA, "rust-mutants-outcome-v1");
    assert_eq!(rust_mutants::outcomes::LAYOUT, "rust-mutants/outcomes-v1");
    assert_eq!(rust_mutants::outcomes::CACHE_ABI, 6);
    assert_eq!(rust_mutants::outcomes::INSTRUMENTATION_ABI, 2);
    assert_eq!(rust_mutants::outcomes::STEP_POLICY_ABI, 1);
}
