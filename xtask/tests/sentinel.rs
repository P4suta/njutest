// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A gate's silence is believed only after it has found what was planted for it.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use xtask::devgates::SeamKind;
use xtask::engineaudit::Layer;
use xtask::engineaudit::sentinel::{Perturbation, clean};
use xtask::gates::{
    GateError, engine_audit_sentinels, engine_audit_sighted, lint_sentinels, lint_sighted,
    proofaudit_sentinels, proofaudit_sighted, seam_sentinels, seam_sighted,
};
use xtask::lints::Kind;
use xtask::proofaudit;
use xtask::sentinel::{PlantedError, Shape, shapes};

const INERT: &str = "=== inert source crates/app/src/lib.rs\n//! A file.\npub fn f() {}\n";

#[test]
fn every_lint_kind_is_found_in_every_shape_planted_for_it() {
    let found = lint_sentinels().unwrap_or_else(|GateError(said)| panic!("{said}"));
    assert!(
        found >= Kind::ALL.len(),
        "{found} shapes cannot cover {} kinds",
        Kind::ALL.len()
    );
}

#[test]
fn a_kind_whose_planted_shape_is_not_found_makes_the_gate_refuse_rather_than_pass() {
    for kind in Kind::ALL {
        let Err(GateError(said)) = lint_sighted(*kind, INERT) else {
            panic!(
                "{} passed over a shape that carries none of it",
                kind.label()
            );
        };
        assert!(
            said.contains(&format!("the {} check is blind", kind.label()))
                && said.contains("`inert`"),
            "the refusal names the kind and the shape it could not see: {said}"
        );
    }
}

#[test]
fn a_shape_that_is_not_found_is_named_even_when_an_earlier_one_was() {
    let planted = format!("=== seen source crates/app/src/lib.rs\n#![allow(dead_code)]\n{INERT}");
    let Err(GateError(said)) = lint_sighted(Kind::AllowAttribute, &planted) else {
        panic!("one shape found does not stand for another");
    };
    assert!(said.contains("`inert`"), "{said}");
}

#[test]
fn the_planted_text_says_where_each_shape_begins_and_which_files_it_is() {
    assert_eq!(
        shapes("=== one source a/b.rs\nx\ny\n=== two tree\n--- c.rs\nz\n--- d.rs\n"),
        Ok(vec![
            Shape::Source {
                name: "one".to_owned(),
                path: "a/b.rs".to_owned(),
                text: "x\ny\n".to_owned(),
            },
            Shape::Tree {
                name: "two".to_owned(),
                files: vec![
                    ("c.rs".to_owned(), "z\n".to_owned()),
                    ("d.rs".to_owned(), String::new()),
                ],
            },
        ])
    );
}

#[test]
fn planted_text_that_does_not_say_what_it_plants_is_refused() {
    for (text, refused) in [
        (
            "x\n",
            PlantedError::Headless {
                line: "x".to_owned(),
            },
        ),
        (
            "=== one sauce a.rs\n",
            PlantedError::Header {
                header: "=== one sauce a.rs".to_owned(),
            },
        ),
        (
            "=== one source\n",
            PlantedError::Header {
                header: "=== one source".to_owned(),
            },
        ),
        (
            "=== one tree extra\n",
            PlantedError::Header {
                header: "=== one tree extra".to_owned(),
            },
        ),
        (
            "=== one tree\nx\n",
            PlantedError::Unplaced {
                name: "one".to_owned(),
            },
        ),
        (
            "=== one source a.rs\n--- b.rs\n",
            PlantedError::SecondFile {
                name: "one".to_owned(),
            },
        ),
        (
            "=== one tree\n",
            PlantedError::Empty {
                name: "one".to_owned(),
            },
        ),
        ("", PlantedError::Nothing),
    ] {
        assert_eq!(shapes(text), Err(refused), "{text:?}");
    }
}

#[test]
fn every_seam_kind_is_found_in_every_shape_planted_for_it() {
    let found = seam_sentinels().unwrap_or_else(|GateError(said)| panic!("{said}"));
    assert!(
        found >= SeamKind::ALL.len(),
        "{found} shapes cannot cover {} kinds",
        SeamKind::ALL.len()
    );
}

#[test]
fn a_seam_kind_whose_planted_shape_is_not_found_makes_the_gate_refuse() {
    for kind in SeamKind::ALL {
        let Err(GateError(said)) = seam_sighted(kind, INERT) else {
            panic!("{kind} passed over a shape that carries none of it");
        };
        assert!(
            said.contains(&format!("the {kind} check is blind")) && said.contains("`inert`"),
            "the refusal names the kind and the shape it could not see: {said}"
        );
    }
}

#[test]
fn a_seam_planted_outside_production_code_is_not_found() {
    let planted = "=== in-a-test tree\n--- crates/app/tests/it.rs\nstatic mut X: u32 = 0;\n";
    assert!(
        seam_sighted(SeamKind::StaticMut, planted).is_err(),
        "the tree shape goes through the same production filter the gate applies"
    );
}

#[test]
fn every_engine_audit_layer_fires_on_what_was_planted_for_it() {
    let found =
        engine_audit_sentinels(&checkers()).unwrap_or_else(|GateError(said)| panic!("{said}"));
    assert!(
        found >= Layer::ALL.len(),
        "{found} planted defects cannot cover {} layers",
        Layer::ALL.len()
    );
}

#[test]
fn an_engine_audit_layer_whose_planted_defect_changes_nothing_is_refused_as_blind() {
    let inert = Perturbation {
        name: "inert",
        ..clean()
    };
    for layer in Layer::ALL {
        let Err(GateError(said)) =
            engine_audit_sighted(&checkers(), layer, std::slice::from_ref(&inert))
        else {
            panic!(
                "{} passed over a run with nothing planted in it",
                layer.label()
            );
        };
        assert!(
            said.contains(&format!("the {} layer is blind", layer.label()))
                && said.contains("`inert`"),
            "the refusal names the layer and the perturbation it could not see: {said}"
        );
    }
}

#[test]
fn an_engine_audit_layer_with_nothing_planted_for_it_is_refused_as_blind() {
    for layer in Layer::ALL {
        let Err(GateError(said)) = engine_audit_sighted(&checkers(), layer, &[]) else {
            panic!("{} passed with nothing planted for it", layer.label());
        };
        assert!(
            said.contains(&format!("the {} layer is blind", layer.label())),
            "{said}"
        );
    }
}

#[test]
fn a_perturbation_found_does_not_stand_for_one_that_is_not() {
    let mut planted = Layer::Identity.planted();
    planted.push(Perturbation {
        name: "inert",
        ..clean()
    });
    let Err(GateError(said)) = engine_audit_sighted(&checkers(), Layer::Identity, &planted) else {
        panic!("one defect found does not stand for another");
    };
    assert!(said.contains("`inert`"), "{said}");
}

#[test]
fn every_proofaudit_layer_fires_on_what_was_planted_for_it() {
    let found =
        proofaudit_sentinels(&checkers()).unwrap_or_else(|GateError(said)| panic!("{said}"));
    assert!(
        found >= proofaudit::Layer::ALL.len(),
        "{found} planted defects cannot cover {} layers",
        proofaudit::Layer::ALL.len()
    );
}

#[test]
fn a_proofaudit_layer_whose_planted_defect_changes_nothing_is_refused_as_blind() {
    let inert = proofaudit::sentinel::Perturbation {
        name: "inert",
        ..proofaudit::sentinel::clean()
    };
    for layer in proofaudit::Layer::ALL {
        let Err(GateError(said)) =
            proofaudit_sighted(&checkers(), layer, std::slice::from_ref(&inert))
        else {
            panic!(
                "{} passed over a run with nothing planted in it",
                layer.label()
            );
        };
        assert!(
            said.contains(&format!("the {} layer is blind", layer.label()))
                && said.contains("`inert`"),
            "the refusal names the layer and the perturbation it could not see: {said}"
        );
    }
}

#[test]
fn a_proofaudit_layer_with_nothing_planted_for_it_is_refused_as_blind() {
    for layer in proofaudit::Layer::ALL {
        let Err(GateError(said)) = proofaudit_sighted(&checkers(), layer, &[]) else {
            panic!("{} passed with nothing planted for it", layer.label());
        };
        assert!(
            said.contains(&format!("the {} layer is blind", layer.label())),
            "{said}"
        );
    }
}

#[test]
fn a_proofaudit_perturbation_found_does_not_stand_for_one_that_is_not() {
    let mut planted = proofaudit::Layer::Killers.planted();
    planted.push(proofaudit::sentinel::Perturbation {
        name: "inert",
        ..proofaudit::sentinel::clean()
    });
    let Err(GateError(said)) =
        proofaudit_sighted(&checkers(), proofaudit::Layer::Killers, &planted)
    else {
        panic!("one defect found does not stand for another");
    };
    assert!(said.contains("`inert`"), "{said}");
}

#[test]
fn every_outcome_a_report_can_claim_is_refused_when_its_executions_say_otherwise() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("schema/njutest-assurance-report-v1.json");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let schema = xtask::strictjson::from_str(&text)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut outcomes: Vec<String> = schema
        .pointer("/$defs/answered/properties/outcome/enum")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("the schema no longer lists the outcomes a row answers with"))
        .iter()
        .filter_map(|value| value.as_str().map(str::to_owned))
        .collect();
    outcomes.sort();
    assert!(
        outcomes.len() > 8,
        "the outcomes are read from the schema so that the one somebody adds next is covered \
         the day it arrives: {outcomes:?}"
    );
    let mut believed = Vec::new();
    for outcome in &outcomes {
        let Some(lie) = proofaudit::sentinel::lie(outcome) else {
            believed.push(format!("{outcome}: nothing is planted for it"));
            continue;
        };
        let laid = lie
            .lay()
            .unwrap_or_else(|error| panic!("{}: {error}", lie.name));
        let audit = xtask::gates::proofaudit(&checkers(), laid.run(), laid.trace())
            .unwrap_or_else(|error| panic!("{}: {error}", lie.name));
        if audit.violations() == 0 {
            believed.push(format!("{outcome}: `{}` drew no violation", lie.name));
        }
    }
    assert!(
        believed.is_empty(),
        "a report that claims an outcome its own executions contradict, and agrees with itself \
         everywhere else, is the lie an audit that only counts cannot see. Every outcome the \
         schema allows has one planted (`proofaudit::sentinel::lie`), and some layer has to \
         refuse it: {believed:#?}"
    );
}

/// Every published schema, compiled.
fn checkers() -> xtask::schemas::Checkers {
    xtask::schemas::Checkers::compiled().expect("the published schemas compile")
}
