// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which guards a run delays, and when a delayed control is believed.

use std::collections::BTreeSet;

use njutest::concurrency::explore::{Delayed, Ended, chosen, delayed};

#[test]
fn every_site_is_chosen_when_there_are_no_more_than_asked_for() {
    let reached = BTreeSet::from([3, 7, 11]);
    let mut all = chosen(&reached, 5);
    all.sort_unstable();
    assert_eq!(all, [3, 7, 11]);
}

#[test]
fn a_cap_spreads_the_sites_across_the_reach_the_same_way_every_time() {
    let reached: BTreeSet<u32> = (0..100).collect();
    let picked = chosen(&reached, 10);
    assert_eq!(picked.len(), 10);
    assert_eq!(
        picked,
        chosen(&reached, 10),
        "the same run chooses the same schedules"
    );
    assert!(
        picked.iter().any(|site| *site >= 50),
        "a cap does not mean the first files only: {picked:?}"
    );
    let unique: BTreeSet<&u32> = picked.iter().collect();
    assert_eq!(unique.len(), 10, "no site is delayed twice: {picked:?}");
}

#[test]
fn a_failure_is_believed_only_when_it_repeats_and_the_undelayed_control_passes() {
    let failed = || Ended::Failed(vec!["a_message_arrives_in_time".to_owned()]);
    assert_eq!(
        delayed(&failed(), &[failed(), failed()], Some(&Ended::Passed)),
        Delayed::Broke {
            failed: vec!["a_message_arrives_in_time".to_owned()]
        }
    );
    assert_eq!(
        delayed(&failed(), &[failed(), Ended::Passed], Some(&Ended::Passed)),
        Delayed::Undecided,
        "a failure the same schedule does not repeat is a flake, not a schedule"
    );
    assert_eq!(
        delayed(&failed(), &[failed(), failed()], Some(&failed())),
        Delayed::Undecided,
        "a test that fails without the delay too is broken on every schedule"
    );
    assert_eq!(
        delayed(&Ended::Unsettled, &[], None),
        Delayed::Undecided,
        "a control that ran past its bound under a pause established nothing"
    );
    assert_eq!(delayed(&Ended::Passed, &[], None), Delayed::Passed);
}

fn record(
    target: &str,
    explored: njutest::report::concurrency::Exploration,
) -> njutest::report::concurrency::ConcurrencyRecord {
    njutest::report::concurrency::ConcurrencyRecord {
        target: target.to_owned(),
        standing: njutest::concurrency::proof::Standing::Concurrent {
            because: vec![njutest::concurrency::proof::Because::LooseReach],
        },
        explored,
    }
}

#[test]
fn a_sample_is_stated_as_one_and_a_broken_schedule_is_a_defect_naming_its_guard() {
    use njutest::report::concurrency::{Exploration, found, limited};
    let records = [
        record(
            "pkg/lib/broke",
            Exploration::Broke {
                site: 4,
                path: "src/lib.rs".to_owned(),
                line: 9,
                failed: vec!["tests::a_message_arrives_in_time".to_owned()],
            },
        ),
        record(
            "pkg/lib/sampled",
            Exploration::Sampled {
                delayed: vec![1, 2],
                undecided: Vec::new(),
            },
        ),
    ];
    let findings = found(&records);
    let [finding] = findings.as_slice() else {
        panic!("one broken schedule is one finding: {findings:?}");
    };
    assert_eq!(
        finding.kind,
        njutest::report::FindingKind::ScheduleDependent
    );
    assert!(
        finding.kind.is_defect(),
        "the suite's answer depends on the schedule"
    );
    assert!(
        finding.detail.contains("src/lib.rs:9"),
        "{}",
        finding.detail
    );
    let limitations = limited(&records);
    let named: Vec<(&str, &str)> = limitations
        .iter()
        .map(|one| (one.name.as_str(), one.detail.as_str()))
        .collect();
    assert_eq!(
        named
            .iter()
            .filter(|(name, _)| *name == njutest::limitation::SCHEDULE_SAMPLED)
            .count(),
        1,
        "{named:?}"
    );
    assert!(
        named.iter().any(
            |(name, detail)| *name == njutest::limitation::SCHEDULE_SAMPLED
                && detail.ends_with("(pkg/lib/sampled)")
        ),
        "a sample is never a proof, and says which binary it is: {named:?}"
    );
    assert!(
        !named
            .iter()
            .any(|(name, _)| *name == njutest::limitation::SCHEDULE_NOT_EXPLORED),
        "a binary that was explored is not unexplored: {named:?}"
    );
}
