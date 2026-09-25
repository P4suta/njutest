// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each crash decision raises, and what a run says about a crash the compiler refused (ADR 0035).

use njutest::report::FindingKind;
use njutest::report::crashes::{CrashDecision, CrashRecord, found, limited};

fn record(decision: CrashDecision) -> CrashRecord {
    CrashRecord {
        catalog_index: njutest::report::CatalogIndex::new(0),
        id: "d".repeat(64),
        display_id: "d".repeat(20),
        path: "src/lib.rs".to_owned(),
        item: "save".to_owned(),
        position: None,
        decision,
    }
}

#[test]
fn a_corrupt_crash_is_a_defect_and_one_the_run_could_not_decide_is_a_hole() {
    let records: Vec<CrashRecord> = CrashDecision::every().into_iter().map(record).collect();
    let kinds: Vec<FindingKind> = found(&records).iter().map(|finding| finding.kind).collect();
    assert_eq!(
        kinds,
        vec![
            FindingKind::CorruptAfterCrash,
            FindingKind::NotMeasured,
            FindingKind::NotMeasured
        ],
        "corrupt is a defect; unshared and undecided leave the run short of assured; the rest \
         raise nothing"
    );
    let stated: Vec<String> = limited(&records)
        .into_iter()
        .map(|limitation| limitation.name)
        .collect();
    assert_eq!(
        stated,
        vec!["crash-not-put".to_owned()],
        "a crash the compiler refused is stated, never counted as anything the suite did"
    );
}
