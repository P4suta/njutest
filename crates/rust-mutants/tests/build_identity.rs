// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Canonical build selections bind every Cargo option controlled by the engine.

use njutest_devkit::result::{
    OptionState::{Absent, Present},
    ResultState::{Refused, Returned},
    option_state, result_state,
};
use rust_mutants::cargo::{BuildConfig, BuildSelection};

fn default_selection() -> BuildSelection {
    BuildConfig::default().selection()
}

fn decode(value: &serde_json::Value) -> Result<BuildSelection, serde_json::Error> {
    serde_json::to_string(value)
        .and_then(|encoded| njutest_devkit::strictjson::decode_str(&encoded))
}

#[test]
fn every_build_selection_field_changes_the_canonical_digest() {
    let base = BuildConfig::default();
    let baseline = base.selection();
    let changed = [
        BuildConfig {
            features: vec!["one".to_owned()],
            ..base.clone()
        },
        BuildConfig {
            all_features: true,
            ..base.clone()
        },
        BuildConfig {
            no_default_features: true,
            ..base.clone()
        },
        BuildConfig {
            target: Some("wasm32-unknown-unknown".to_owned()),
            ..base.clone()
        },
        BuildConfig {
            profile: Some("release".to_owned()),
            ..base.clone()
        },
        BuildConfig {
            jobs: Some(2),
            ..base.clone()
        },
        BuildConfig {
            debug: true,
            ..base
        },
    ];

    for selection in changed.map(|configuration| configuration.selection()) {
        assert_ne!(selection, baseline);
        assert_ne!(selection.digest(), baseline.digest());
    }
}

#[test]
fn features_are_a_canonical_set_and_round_trip_strictly() {
    let first = BuildConfig {
        features: vec!["b".to_owned(), "a".to_owned(), "b".to_owned()],
        ..BuildConfig::default()
    }
    .selection();
    let second = BuildConfig {
        features: vec!["a".to_owned(), "b".to_owned()],
        ..BuildConfig::default()
    }
    .selection();
    assert_eq!(first, second);
    assert_eq!(first.features(), ["a", "b"]);

    let encoded = serde_json::to_string(&first);
    assert_eq!(result_state(&encoded), Returned, "selection: {encoded:?}");
    let Ok(encoded) = encoded else { return };
    let decoded: Result<BuildSelection, _> = njutest_devkit::strictjson::decode_str(&encoded);
    assert_eq!(result_state(&decoded), Returned, "selection: {decoded:?}");
    let Ok(decoded) = decoded else { return };
    assert_eq!(decoded, first);
}

#[test]
fn the_wire_rejects_noncanonical_or_incomplete_build_claims() {
    let encoded = serde_json::to_string(&default_selection());
    assert_eq!(result_state(&encoded), Returned, "selection: {encoded:?}");
    let Ok(encoded) = encoded else { return };

    let duplicate = encoded.replace("\"debug\":false", "\"debug\":false,\"debug\":false");
    assert_ne!(
        duplicate, encoded,
        "the duplicate-key fixture was constructed"
    );
    let decoded = njutest_devkit::strictjson::decode_str::<BuildSelection>(&duplicate);
    assert_eq!(result_state(&decoded), Refused, "duplicate: {decoded:?}");

    let missing = njutest_devkit::strictjson::decode_str::<serde_json::Value>(&encoded);
    assert_eq!(result_state(&missing), Returned, "fixture: {missing:?}");
    let Ok(mut missing) = missing else { return };
    let object = missing.as_object_mut();
    assert_eq!(option_state(object.as_deref()), Present, "selection object");
    let Some(object) = object else { return };
    let removed = object.remove("target");
    assert_eq!(option_state(removed.as_ref()), Present, "removed target");
    let decoded = decode(&missing);
    assert_eq!(
        result_state(&decoded),
        Refused,
        "missing target: {decoded:?}"
    );

    let extra = njutest_devkit::strictjson::decode_str::<serde_json::Value>(&encoded);
    assert_eq!(result_state(&extra), Returned, "fixture: {extra:?}");
    let Ok(mut extra) = extra else { return };
    let object = extra.as_object_mut();
    assert_eq!(option_state(object.as_deref()), Present, "selection object");
    let Some(object) = object else { return };
    let previous = object.insert("future".to_owned(), serde_json::Value::Bool(true));
    assert_eq!(option_state(previous.as_ref()), Absent, "new future field");
    let decoded = decode(&extra);
    assert_eq!(result_state(&decoded), Refused, "extra field: {decoded:?}");

    let forged = njutest_devkit::strictjson::decode_str::<serde_json::Value>(&encoded);
    assert_eq!(result_state(&forged), Returned, "fixture: {forged:?}");
    let Ok(mut forged) = forged else { return };
    let digest = forged.get_mut("digest");
    assert_eq!(option_state(digest.as_deref()), Present, "digest field");
    let Some(digest) = digest else { return };
    *digest = serde_json::Value::String("0".repeat(64));
    let decoded = decode(&forged);
    assert_eq!(
        result_state(&decoded),
        Refused,
        "forged digest: {decoded:?}"
    );
}

#[test]
fn the_wire_rejects_noncanonical_feature_order_before_it_can_be_claimed() {
    let selection = BuildConfig {
        features: vec!["a".to_owned(), "b".to_owned()],
        ..BuildConfig::default()
    }
    .selection();
    let unsorted = serde_json::to_value(&selection);
    assert_eq!(result_state(&unsorted), Returned, "selection: {unsorted:?}");
    let Ok(mut unsorted) = unsorted else { return };
    let features = unsorted.get_mut("features");
    assert_eq!(option_state(features.as_deref()), Present, "features field");
    let Some(features) = features else { return };
    *features = serde_json::json!(["b", "a"]);
    let decoded = decode(&unsorted);
    assert_eq!(result_state(&decoded), Refused, "unsorted: {decoded:?}");

    let duplicate = serde_json::to_value(&selection);
    assert_eq!(
        result_state(&duplicate),
        Returned,
        "selection: {duplicate:?}"
    );
    let Ok(mut duplicate) = duplicate else { return };
    let features = duplicate.get_mut("features");
    assert_eq!(option_state(features.as_deref()), Present, "features field");
    let Some(features) = features else { return };
    *features = serde_json::json!(["a", "a", "b"]);
    let decoded = decode(&duplicate);
    assert_eq!(
        result_state(&decoded),
        Refused,
        "duplicate features: {decoded:?}"
    );
}
