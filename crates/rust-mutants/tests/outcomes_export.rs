// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a store travels as records carrying what they were keyed on, and is filed only under the names those inputs derive.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use rust_mutants::id::HexDigest;
use rust_mutants::outcomes::{Abi, CacheOutcome, Keyed, Record, SCHEMA, Store, StoreError};

fn record(byte: char) -> Record {
    Record {
        schema: SCHEMA.to_owned(),
        mutant: HexDigest::try_from(byte.to_string().repeat(64)).expect("a mutant"),
        outcome: CacheOutcome::Killed,
        target: "demo/lib/demo".to_owned(),
        tests_run: Some(1),
        failed_tests: vec!["noticed".to_owned()],
        run_id: "20260101T000000000Z".to_owned(),
        keyed: Keyed {
            closure: "c".repeat(64),
            manifests: "d".repeat(64),
            toolchain: "cargo 1.98.0 rustc 1.98.0 aarch64-apple-darwin".to_owned(),
            args: Vec::new(),
            timeout: "auto".to_owned(),
            steps: 0,
            build: Vec::new(),
            engine: "e".to_owned(),
        },
    }
}

#[test]
fn a_store_exported_and_imported_holds_the_same_records_under_the_same_names() {
    let from = tempfile::tempdir().expect("tempdir");
    let into = tempfile::tempdir().expect("tempdir");
    let source = Store::new(from.path());
    for byte in ['a', 'b', 'e'] {
        source.put(&record(byte)).expect("stored");
    }
    let exported = source.export().expect("exported");
    assert_eq!(exported.records.len(), 3);
    let target = Store::new(into.path());
    assert_eq!(target.import(&exported).expect("imported"), 3);
    for byte in ['a', 'b', 'e'] {
        let one = record(byte);
        let found = target.get(&one.key(), &one.mutant).expect("readable");
        assert_eq!(
            found.map(|(_, record)| record),
            Some(one),
            "each record answers under the key its own inputs name"
        );
    }
}

#[test]
fn records_keyed_under_another_release_are_refused_before_anything_is_filed() {
    let from = tempfile::tempdir().expect("tempdir");
    let into = tempfile::tempdir().expect("tempdir");
    let source = Store::new(from.path());
    source.put(&record('a')).expect("stored");
    let mut older = source.export().expect("exported");
    older.abi = Abi {
        cache: Abi::CURRENT.cache.saturating_sub(1),
        ..Abi::CURRENT
    };
    let target = Store::new(into.path());
    assert!(
        matches!(target.import(&older), Err(StoreError::Refused { .. })),
        "a record keyed under another release's versions would be filed under a name this release \
         computes differently"
    );
    assert_eq!(target.size().expect("sized").0, 0, "and nothing was filed");
}

#[test]
fn a_record_filed_under_a_name_its_inputs_do_not_derive_is_corrupt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = Store::new(dir.path());
    let honest = record('a');
    let filed = store.put(&honest).expect("stored");
    let forged = HexDigest::try_from("f".repeat(64)).expect("another key");
    std::fs::rename(&filed, filed.with_file_name(format!("{forged}.json"))).expect("moved");
    assert!(
        matches!(
            store.get(&forged, &honest.mutant),
            Err(StoreError::Corrupt { .. })
        ),
        "an answer found under a name it was not keyed to is not an answer to that name"
    );
}
