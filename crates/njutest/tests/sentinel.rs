// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A run stands on its sentinels: a routing layer that did not route what was planted for it ends the run in an error that says which layer and what is no longer believed.

use njutest::error::RunnerError;
use rust_mutants::sentinel::{Expected, Planted, Sighted, Sighting};
use rust_mutants::session::{Asked, Discharge, Proof, Reaches, Route};

/// The route a layer that works gives the mutant planted for it.
fn routed_as(expected: Expected) -> Route {
    let target = "sentinel/test/planted".to_owned();
    match expected {
        Expected::Unreached => Route::Unreached {
            considered: vec![target],
        },
        Expected::Discharged(proof) => Route::Discharged {
            discharged: vec![Discharge { target, proof }],
        },
        Expected::Kept => Route::Block {
            reaching: vec![Reaches {
                target,
                tests: Asked::Every,
            }],
            discharged: Vec::new(),
            fallback: None,
        },
    }
}

/// Every planted mutant, routed the way its layer must route it.
fn all_sighted() -> Sighted {
    Sighted {
        sightings: Planted::every()
            .into_iter()
            .flat_map(Planted::expectations)
            .map(|expectation| Sighting {
                expectation,
                route: Ok(routed_as(expectation.expected)),
            })
            .collect(),
        kept: Vec::new(),
    }
}

#[test]
fn a_run_whose_every_layer_routed_its_planted_mutant_goes_on() {
    let sighted = all_sighted();
    assert!(sighted.sightings.iter().all(Sighting::sighted));
    assert!(
        njutest::assure::sentinel::believed(&sighted).is_ok(),
        "a layer that routed what was planted for it is one the run may believe"
    );
}

#[test]
fn a_layer_that_did_not_route_its_planted_mutant_ends_the_run_and_says_what_is_not_believed() {
    let mut sighted = all_sighted();
    let uninfected = sighted
        .sightings
        .iter_mut()
        .find(|one| one.expectation.expected == Expected::Discharged(Proof::NeverInfected))
        .expect("never-infected has a planted mutant");
    uninfected.route = Ok(routed_as(Expected::Kept));

    let error = njutest::assure::sentinel::believed(&sighted)
        .expect_err("a blind layer is not one a run may believe");
    assert!(
        matches!(
            &error,
            RunnerError::Blind {
                layer: Planted::Proof(Proof::NeverInfected),
                ..
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.code().code, "NJ5009");
    assert_eq!(
        error.to_string(),
        "NJ5009: the never-infected layer did not route the mutant planted for it: \
         src/lib.rs:at_most:return-true was to be discharged by never-infected, and the engine \
         routed it `block`. Nothing the never-infected layer would remove from this run is \
         believed, so the run stops before its baseline",
        "the sentence names the layer, the mutant, what was due and what happened, and \
         that the run believes nothing the layer removes, because a reader told only that \
         something failed would look for the fault in their own tests"
    );
    assert!(
        error.code().remedy.contains("defect in the engine"),
        "the remedy sends the reader to the engine, not to their tests: {}",
        error.code().remedy
    );
}

#[test]
fn a_layer_that_removes_the_mutant_it_must_leave_is_as_blind_as_one_that_removes_nothing() {
    let mut sighted = all_sighted();
    let reached = sighted
        .sightings
        .iter_mut()
        .find(|one| {
            one.expectation.planted == Planted::Reach && one.expectation.expected == Expected::Kept
        })
        .expect("reach has a mutant it must leave");
    reached.route = Ok(routed_as(Expected::Unreached));

    let error = njutest::assure::sentinel::believed(&sighted)
        .expect_err("a layer that removes what the tests reach removes findings");
    assert!(
        error.to_string().contains(
            "src/lib.rs:two:return-default was to be kept for the tests that reach it, and \
             the engine routed it `unreached`"
        ),
        "{error}"
    );
}

#[test]
fn the_published_trace_schema_names_every_layer_and_every_expectation() {
    let path = njutest_devkit::paths::workspace_root().join("schema/njutest-trace-v1.json");
    let text = std::fs::read_to_string(path).expect("the trace schema");
    let schema: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("the schema is JSON");
    let branch = schema
        .pointer("/properties/payload/oneOf")
        .and_then(serde_json::Value::as_array)
        .expect("the payload branches")
        .iter()
        .find(|one| one.pointer("/properties/type/const") == Some(&"sentinel".into()))
        .expect("a sentinel branch");
    let listed = |field: &str| -> Vec<String> {
        branch
            .pointer(&format!("/properties/sentinel/properties/{field}/enum"))
            .and_then(serde_json::Value::as_array)
            .expect("an enumerated field")
            .iter()
            .map(|one| one.as_str().expect("a name").to_owned())
            .collect()
    };
    let layers: Vec<String> = Planted::every()
        .into_iter()
        .map(|one| one.name().to_owned())
        .collect();
    let expectations: Vec<String> = Expected::every()
        .into_iter()
        .map(|one| one.name().to_owned())
        .collect();
    assert_eq!(listed("layer"), layers);
    assert_eq!(listed("expected"), expectations);
}
