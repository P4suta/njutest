// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What holds for every run rather than for the ones somebody thought to write down.
//!
//! A table of cases is as thorough as whoever wrote it was, and the rows that
//! matter most are the ones nobody imagined. Each law here is one a reader of
//! a report relies on: the counts add up, the identity is a function of the
//! inputs and of nothing else, and what a run wrote is what the next one
//! reads.

#![expect(
    clippy::arithmetic_side_effects,
    reason = "a law about counts is written the way a reader adds them up; the values are a report's own and cannot approach the width they are held in"
)]

use std::collections::{BTreeMap, BTreeSet};

use mjutest_cli::assure::mutation::{Disposition, Judged, Mutation, Unconfirmed};
use mjutest_cli::config::Contract;
use mjutest_cli::evidence::digest::{Inputs, Mode, identity};
use proptest::prelude::*;
use rust_mutants::session::{Fallback, Route};

/// One disposition of each shape, as a run can reach it.
fn disposition() -> impl Strategy<Value = Disposition> {
    let route = || Route::All {
        reaching: vec!["pkg/lib/pkg".to_owned()],
        fallback: Fallback::NotMeasured,
    };
    prop_oneof![
        Just(Disposition::Rejected {
            diagnostic: "no".to_owned()
        }),
        Just(Disposition::Killed {
            by: "pkg/lib/pkg".to_owned()
        }),
        Just(Disposition::TimedOut {
            on: "pkg/lib/pkg".to_owned()
        }),
        Just(Disposition::Survived { route: route() }),
        Just(Disposition::Unreached),
        Just(Disposition::Equivalent { route: route() }),
        Just(Disposition::Unconfirmed {
            on: "pkg/lib/pkg".to_owned(),
            why: Unconfirmed::DidNotReproduce,
        }),
        Just(Disposition::Errored {
            on: "pkg/lib/pkg".to_owned(),
            detail: "no binary".to_owned(),
        }),
    ]
}

/// A run that judged these mutations, each under an identity of its own.
fn judged_from(dispositions: Vec<Disposition>) -> Mutation {
    Mutation {
        judged: dispositions
            .into_iter()
            .enumerate()
            .map(|(at, disposition)| Judged {
                id: format!("{at:064x}"),
                display_id: format!("{at:020x}"),
                path: "src/lib.rs".to_owned(),
                rule: "add-to-sub@1".to_owned(),
                position: None,
                disposition,
                source_run_id: None,
            })
            .collect(),
        skips: BTreeMap::new(),
    }
}

proptest! {
    /// Every mutation is in exactly one of the four columns a reader adds up.
    #[test]
    fn what_a_run_catalogued_is_what_it_refused_ran_could_not_reach_or_proved_identical(
        dispositions in proptest::collection::vec(disposition(), 0..24)
    ) {
        let counts = judged_from(dispositions).accounting(&BTreeSet::new());
        prop_assert_eq!(
            counts.cataloged,
            counts.rejected + counts.executed + counts.unreached + counts.equivalent,
            "this is the identity a reader adds up to check every other number in the \
             report, and the audit refuses a run that breaks it: {:?}",
            counts
        );
        prop_assert!(
            counts.killed + counts.survived + counts.timed_out <= counts.executed,
            "and what ran is at least what the outcomes of running account for: {:?}",
            counts
        );
    }

    /// A reviewer answers for what nothing noticed, and for nothing else.
    #[test]
    fn an_acceptance_covers_a_survivor_an_unreached_mutation_or_an_equivalent_one(
        dispositions in proptest::collection::vec(disposition(), 0..24)
    ) {
        let mutation = judged_from(dispositions);
        let every: BTreeSet<String> = mutation.judged.iter().map(|one| one.id.clone()).collect();
        let counts = mutation.accounting(&every);
        prop_assert_eq!(
            counts.accepted,
            counts.survived + counts.unreached + counts.equivalent,
            "a ledger that accepts every mutation the run judged answers for exactly the \
             ones nothing noticed. Counting a timeout or a harness that failed among them \
             would let a reviewer sign off on an outcome nobody established: {:?}",
            counts
        );
        let none = judged_from(Vec::new()).accounting(&every);
        prop_assert_eq!(none.accepted, 0, "and a run that judged nothing accepts nothing");
    }
}

/// What a run is, as the digest reads it.
fn inputs() -> Inputs {
    Inputs {
        tree: "a".repeat(64),
        corpus: "b".repeat(64),
        dependencies: "c".repeat(64),
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: vec![("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned())],
        contract: Contract::StandardV1,
        configuration: "d".repeat(64),
        test_args: vec!["--test-threads=1".to_owned()],
        mode: Mode::Full,
        shard: None,
    }
}

/// One field of a run's identity, and how to make it say something else.
type Field = (&'static str, fn(&mut Inputs, &str));

/// Every field a run's identity is computed from.
const FIELDS: [Field; 9] = [
    ("tree", |it, value| value.clone_into(&mut it.tree)),
    ("corpus", |it, value| value.clone_into(&mut it.corpus)),
    ("dependencies", |it, value| {
        value.clone_into(&mut it.dependencies);
    }),
    ("toolchain", |it, value| value.clone_into(&mut it.toolchain)),
    ("platform", |it, value| value.clone_into(&mut it.platform)),
    ("configuration", |it, value| {
        value.clone_into(&mut it.configuration);
    }),
    ("environment", |it, value| {
        it.environment.push(("EXTRA".to_owned(), value.to_owned()));
    }),
    ("test_args", |it, value| {
        it.test_args.push(value.to_owned());
    }),
    ("shard", |it, value| {
        it.shard = Some(format!("1/{}", value.len().max(1)));
    }),
];

proptest! {
    /// Nothing a run is a function of may be changed without changing what it is.
    #[test]
    fn a_run_that_differs_in_any_one_of_its_inputs_is_a_different_run(
        value in "[a-z0-9]{1,24}"
    ) {
        let base = identity(&inputs());
        for (named, change) in FIELDS {
            let mut altered = inputs();
            change(&mut altered, &value);
            prop_assert_ne!(
                identity(&altered),
                base.clone(),
                "two runs that differ in {} share an identity, so one of them will be \
                 handed the other's answer. A table of cases is as thorough as whoever \
                 wrote it; this is every field there is",
                named
            );
        }
    }

    /// The identity is a function of the inputs and of nothing else about how they were collected.
    #[test]
    fn the_order_and_the_repetition_of_an_environment_are_not_facts_about_a_run(
        mut environment in proptest::collection::vec(
            ("[A-Z_]{1,8}", "[a-z0-9 =/-]{0,16}"),
            0..8,
        )
    ) {
        environment.sort();
        environment.dedup_by(|one, other| one.0 == other.0);
        let ordered = Inputs { environment: environment.clone(), ..inputs() };
        let mut shuffled = environment.clone();
        shuffled.reverse();
        prop_assert_eq!(
            identity(&Inputs { environment: shuffled, ..inputs() }),
            identity(&ordered),
            "a process lists its environment in whatever order it likes, and a run whose \
             identity depended on that order would never read back its own answer"
        );
        let mut twice = environment.clone();
        twice.extend(environment);
        prop_assert_eq!(
            identity(&Inputs { environment: twice, ..inputs() }),
            identity(&ordered),
            "and a variable named twice with one value is one variable: a process cannot \
             be started with two of them and read anything but one"
        );
    }
}
