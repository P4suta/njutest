// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a planted crate holds, which switches a session over it keeps, and when a route bears an expectation out.

use rust_mutants::probe::Question;
use rust_mutants::sentinel::{Expected, Infection, KeptFor, Planted};
use rust_mutants::session::{Discharge, Fallback, PrepareOptions, Proof, Reaches, Route};

#[test]
fn every_layer_plants_its_items_and_names_two_different_mutants_in_them() {
    let library = rust_mutants::sentinel::library();
    for planted in Planted::routing() {
        let [removed, kept] = planted.expectations();
        assert_ne!(
            removed.mutant, kept.mutant,
            "{planted} is asked about two mutants"
        );
        assert!(
            matches!(kept.expected, Expected::Kept(_)),
            "{planted} leaves one of its pair to the tests"
        );
        assert_eq!(
            removed.mutant.rule, kept.mutant.rule,
            "{planted} asks both halves of its pair about one rule"
        );
        for one in [removed, kept] {
            assert!(
                library.contains(&format!("pub fn {}(", one.mutant.item)),
                "{planted} names {} and the planted library does not hold it",
                one.mutant
            );
        }
    }
    for proof in Proof::ALL {
        assert!(
            Planted::every()
                .iter()
                .any(|planted| planted.proof() == Some(proof)),
            "{proof} has a pair planted for it"
        );
    }
    for question in Question::ALL {
        let planted = Planted::Infection(Infection::Probe(question));
        assert!(
            Planted::every().contains(&planted),
            "every question a probe asks has a pair, because each has its own recorder"
        );
        let [removed, _] = planted.expectations();
        assert_eq!(
            Question::of(removed.mutant.rule),
            Some(question),
            "and the pair is asked about the rule that question is asked of"
        );
    }
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
    assert!(Expected::Kept(KeptFor::Tests).holds(&kept(None)));
    assert!(
        !Expected::Kept(KeptFor::Library).holds(&kept(None)),
        "a mutant put only to the integration tests is not one the library's own tests were asked about"
    );
    assert!(
        !Expected::Kept(KeptFor::Tests).holds(&kept(Some(Fallback::TouchIncomplete))),
        "a target kept because its measurement was lost is not one the measurement placed"
    );
    assert!(
        !Expected::Kept(KeptFor::Tests).holds(&everything),
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

#[test]
fn every_rule_a_pair_or_a_question_names_is_one_the_engine_catalogs() {
    let registry = rust_mutants::rule::Registry::canonical();
    let known = |name: &str| registry.rules().iter().any(|rule| rule.name == name);
    for planted in Planted::every() {
        let rule = planted.pair().rule;
        assert!(
            known(rule),
            "{planted} plants `{rule}`, which no rule of the canonical registry is, so every run \
             would fail to find its planted mutant and stop"
        );
    }
    for question in Question::ALL {
        assert!(
            known(question.rule()),
            "a probe is asked about `{}`, which the engine never catalogs",
            question.rule()
        );
    }
}

#[test]
fn the_coverage_route_and_the_equivalence_layer_each_have_a_pair_planted() {
    for planted in [Planted::Coverage, Planted::Equivalence] {
        assert!(
            Planted::every().contains(&planted),
            "{planted} removes work, so a pair is planted for it"
        );
        assert!(
            !Planted::routing().contains(&planted),
            "{planted} is asked only of a run that could use it, never of every run's session"
        );
    }
    let [unreached, kept] = Planted::Coverage.expectations();
    assert_eq!(unreached.expected, Expected::Unreached);
    assert_eq!(kept.expected, Expected::Kept(KeptFor::Library));
    let [identical, rendered] = Planted::Equivalence.expectations();
    assert_eq!(identical.expected, Expected::Identical);
    assert_eq!(rendered.expected, Expected::Rendered);
    assert_eq!(
        identical.mutant.rule, rendered.mutant.rule,
        "one rule makes both, so only what the compiler does with each can tell them apart"
    );
    let library = rust_mutants::sentinel::equivalent_library();
    for one in [identical, rendered] {
        assert!(
            library.contains(&format!("pub fn {}(", one.mutant.item)),
            "the equivalence crate holds {}",
            one.mutant
        );
    }
}

#[test]
fn an_identity_bears_out_only_the_expectation_it_is() {
    use rust_mutants::equivalence::Identity;
    use rust_mutants::sentinel::Compared;
    let answer = |identity, withdrawn| Compared {
        identity,
        withdrawn,
    };
    assert!(Expected::Identical.compares(&answer(Identity::Identical, false)));
    assert!(!Expected::Rendered.compares(&answer(Identity::Identical, false)));
    assert!(Expected::Rendered.compares(&answer(Identity::Differs, false)));
    assert!(
        !Expected::Identical.compares(&answer(Identity::Differs, false)),
        "a layer that renders an equivalent mutation differently removes nothing it could"
    );
    assert!(
        !Expected::Identical.compares(&answer(Identity::NotEstablished("x"), false)),
        "and one still in service that establishes nothing about it has stopped telling"
    );
    let drifted = Identity::NotEstablished(rust_mutants::equivalence::CONTROL_DRIFTED);
    assert!(
        Expected::Identical.compares(&answer(drifted, true))
            && Expected::Rendered.compares(&answer(drifted, true)),
        "a layer a control withdrew calls nothing identical, so it removes nothing either way"
    );
}
