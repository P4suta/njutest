// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The measurement document `njutest measure` writes is the one its schema describes, every closed set of it included.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::collections::{BTreeMap, BTreeSet};

use njutest::reach::{Document, Schema};
use rust_mutants::select::{Inputs, Measurement, Shadows, Standing, Target};
use rust_mutants::snapshot::{Survey, Surveyed};
use rust_mutants::touch::Unmeasured;

fn validator() -> jsonschema::Validator {
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(
            njutest_devkit::paths::workspace_root().join("schema/njutest-reach-v1.json"),
        )
        .expect("the schema is there"),
    )
    .expect("the schema is JSON");
    jsonschema::validator_for(&schema).expect("the schema compiles")
}

fn document(standing: Standing) -> Document {
    Document {
        schema: Schema::V1,
        measurement: Measurement {
            toolchain: "rustc 1.98.1".to_owned(),
            survey: Survey {
                rules: "rules".to_owned(),
                files: BTreeMap::from([(
                    "src/lib.rs".to_owned(),
                    Surveyed {
                        sha256: "a".repeat(64),
                        executable: false,
                    },
                )]),
                passed_over: BTreeMap::from([("link".to_owned(), Some("target".to_owned()))]),
            },
            inputs: Inputs {
                outside: BTreeMap::from([("/registry/dep.rs".to_owned(), "b".repeat(64))]),
                env: BTreeMap::from([("UNSET".to_owned(), None)]),
            },
            environment: BTreeMap::from([("LANG".to_owned(), "C".to_owned())]),
            settings: BTreeMap::from([("build".to_owned(), "default".to_owned())]),
            items: Vec::new(),
            targets: BTreeMap::from([(
                "demo/lib/demo".to_owned(),
                Target {
                    entered: BTreeSet::from([0]),
                    standing,
                },
            )]),
            shadows: BTreeMap::from([("demo".to_owned(), Shadows::of(["use tokio::test;"]))]),
        },
    }
}

#[test]
fn every_standing_a_measurement_can_record_is_one_the_schema_accepts() {
    let validator = validator();
    let standings = [Standing::Held, Standing::Moved, Standing::Uncompared]
        .into_iter()
        .chain(
            Unmeasured::ALL
                .into_iter()
                .map(|why| Standing::NotMeasured { why }),
        );
    for standing in standings {
        let value = serde_json::to_value(document(standing)).expect("the document renders");
        let errors: Vec<String> = validator
            .iter_errors(&value)
            .map(|error| error.to_string())
            .collect();
        assert!(
            errors.is_empty(),
            "{standing:?} is written as something the schema refuses: {errors:?}"
        );
    }
}
