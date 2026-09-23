// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a planted crate holds, which switches a session over it keeps, and when a route bears an expectation out.

use rust_mutants::sentinel::{Expected, Planted};
use rust_mutants::session::{Discharge, Fallback, PrepareOptions, Proof, Reaches, Route};

#[test]
fn every_layer_plants_its_items_and_names_two_different_mutants_in_them() {
    let library = rust_mutants::sentinel::library();
    for planted in Planted::every() {
        let [removed, kept] = planted.expectations();
        assert_ne!(
            removed.mutant, kept.mutant,
            "{planted} is asked about two mutants"
        );
        assert_eq!(kept.expected, Expected::Kept, "{planted}");
        for one in [removed, kept] {
            assert!(
                library.contains(&format!("pub fn {}(", one.mutant.item)),
                "{planted} names {} and the planted library does not hold it",
                one.mutant
            );
        }
    }
    assert_eq!(
        Planted::every().len(),
        Proof::ALL.len() + 1,
        "one layer per proof, and the reach measurement"
    );
}

#[test]
fn a_session_over_the_planted_crate_keeps_what_routes_and_drops_what_describes_the_callers_tree() {
    let caller = PrepareOptions {
        touch: true,
        coverage: true,
        branch_proofs: true,
        verify: false,
        packages: vec!["theirs".to_owned()],
        harness_args: vec!["--skip".to_owned(), "equal".to_owned()],
        skip_targets: vec!["sentinel/test/planted".to_owned()],
        measurements: Some(std::path::PathBuf::from("remembered")),
        operators: vec!["return-true".to_owned()],
        build: rust_mutants::cargo::BuildConfig {
            features: vec!["theirs".to_owned()],
            profile: Some("release".to_owned()),
            ..rust_mutants::cargo::BuildConfig::default()
        },
        failing: rust_mutants::session::Failing::Exclude,
        ..PrepareOptions::default()
    };
    let routing = rust_mutants::sentinel::routing(&caller);
    assert!(routing.touch && routing.coverage && routing.branch_proofs);
    assert!(
        routing.verify,
        "the guards measure on the verifying run, so a session that skipped it would route by nothing"
    );
    assert!(routing.packages.is_empty() && routing.operators.is_empty());
    assert!(
        routing.harness_args.is_empty() && routing.skip_targets.is_empty(),
        "an argument that narrows the caller's tests would narrow the planted ones out of the measurement"
    );
    assert!(
        routing.measurements.is_none(),
        "a remembered measurement would answer for the layer instead of it"
    );
    assert!(routing.build.features.is_empty());
    assert_eq!(routing.build.profile.as_deref(), Some("release"));
    assert_eq!(routing.failing, rust_mutants::session::Failing::Refuse);
}

#[test]
fn a_route_bears_out_only_the_expectation_it_is() {
    let target = "sentinel/test/planted".to_owned();
    let discharged = |proof| Route::Discharged {
        discharged: vec![Discharge {
            target: target.clone(),
            proof,
        }],
    };
    let kept = |fallback| Route::Block {
        reaching: vec![Reaches {
            target: target.clone(),
            tests: rust_mutants::session::Asked::Every,
        }],
        discharged: Vec::new(),
        fallback,
    };
    let unreached = Route::Unreached {
        considered: vec![target.clone()],
    };
    let everything = Route::All {
        reaching: vec![target.clone()],
        fallback: Fallback::NotMeasured,
    };
    let never = Expected::Discharged(Proof::NeverInfected);

    assert!(never.holds(&discharged(Proof::NeverInfected)));
    assert!(!never.holds(&discharged(Proof::BranchNeverTaken)));
    assert!(
        !never.holds(&Route::Discharged {
            discharged: Vec::new()
        }),
        "a discharge that names no target proves nothing"
    );
    assert!(Expected::Unreached.holds(&unreached));
    assert!(!Expected::Unreached.holds(&everything));
    assert!(Expected::Kept.holds(&kept(None)));
    assert!(
        !Expected::Kept.holds(&kept(Some(Fallback::TouchIncomplete))),
        "a target kept because its measurement was lost is not one the measurement placed"
    );
    assert!(
        !Expected::Kept.holds(&everything),
        "every target, because nothing was measured, is not the measurement keeping one"
    );
}

#[test]
fn a_planted_crate_that_cannot_be_written_says_where_and_carries_its_code() {
    let temp = tempfile::tempdir().expect("a temporary directory");
    let occupied = temp.path().join("occupied");
    std::fs::write(&occupied, "a file where the crate's directory would go").expect("a file");
    let error = rust_mutants::sentinel::materialise(&occupied).expect_err("a file is no directory");
    assert_eq!(error.code().code, "RM5007");
    assert!(
        error.to_string().contains("occupied"),
        "the message names the path: {error}"
    );
}
