// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a mutant execution records the union of items its whole process entered, which is what a carried answer rests on (ADR 0041).

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use njutest_devkit::fixture::Fixture;
use rust_mutants::glob::Pattern;
use rust_mutants::rule::Tier;
use rust_mutants::runner::Cancel;
use rust_mutants::session::{PrepareOptions, Recording, Request, Session};
use rust_mutants::testkit::opening::opening;
use rust_mutants::touch::{Completeness, ItemRef};
use rust_mutants::workspace::Workspace;

fn prepare(fixture: &Fixture) -> Session {
    prepare_narrowed(fixture, Vec::new())
}

fn prepare_narrowed(fixture: &Fixture, narrowing: Vec<Pattern>) -> Session {
    Workspace::open(
        fixture.root(),
        opening(&njutest_devkit::paths::cargo_binary(), fixture.temp()),
        &Cancel::new(),
    )
    .expect("open")
    .prepare(
        &PrepareOptions {
            tier: Tier::All,
            touch: true,
            narrowing,
            ..PrepareOptions::default()
        },
        &Cancel::new(),
    )
    .expect("prepare")
}

/// The portable name of the item called `name`, as the session's own item catalog numbers it.
fn named(session: &Session, name: &str) -> ItemRef {
    let items = &session.touched().items;
    let item = items
        .iter()
        .find(|one| one.name == name)
        .expect("the fixture's item");
    let first = items
        .iter()
        .filter(|one| one.path == item.path)
        .map(|one| one.index)
        .min()
        .expect("its file's first item");
    ItemRef {
        package: item.package.clone(),
        path: item.path.clone(),
        ordinal: item
            .index
            .checked_sub(first)
            .expect("an item after its file's first"),
    }
}

/// The items the process entered when `mutant` ran with recording asked for.
fn entered_under(session: &Session, rule: &str, inside: &str) -> rust_mutants::touch::Entered {
    let mutant = session
        .catalog()
        .mutants()
        .iter()
        .find(|one| one.candidate.rule.name == rule && session.item_of(one.index) == Some(inside))
        .expect("the fixture's mutation");
    let result = session
        .exec(
            &Request::new(mutant.id.to_string()).recording(Recording::Items),
            &Cancel::new(),
        )
        .expect("exec");
    result
        .entered
        .expect("an execution asked to record what it entered records it")
}

#[test]
fn every_mutant_execution_names_the_items_it_entered() {
    let fixture = Fixture::copy("fixture-entered");
    let session = prepare(&fixture);
    let rare = named(&session, "rare");

    let forced = entered_under(&session, "condition-to-true", "pick");
    assert!(
        forced.items.contains(&rare),
        "a mutation that forces the branch the tests never take enters the item behind it, and \
         the union says so: {forced:?}"
    );
    assert_eq!(
        forced.completeness,
        Completeness::Whole,
        "a process that ran to its end accounts for everything it entered"
    );
    assert!(
        forced.records >= 1
            && usize::try_from(forced.records)
                .is_ok_and(|records| records <= forced.items.len() * 4),
        "and says what recording it cost, in records written, which stays a small multiple of \
         the items named: {} records for {} items",
        forced.records,
        forced.items.len()
    );

    let elsewhere = entered_under(&session, "return-default", "common");
    assert!(
        !elsewhere.items.contains(&rare),
        "and a mutation that leaves the branch alone never reaches it: {elsewhere:?}"
    );
    assert_eq!(
        elsewhere.completeness,
        Completeness::UpToFirstFailure,
        "and a kill's union is claimed only up to its failure, however the process ended, \
         because a process may be stopped there: {elsewhere:?}"
    );
    session.close().expect("close");
}

#[test]
fn a_change_set_narrows_what_is_mutated_and_never_what_is_marked() {
    let fixture = Fixture::copy("fixture-outside");
    let whole = prepare(&fixture);
    let narrowed = prepare_narrowed(
        &fixture,
        vec![Pattern::compile("src/lib.rs").expect("a pattern")],
    );
    let paths = |session: &Session, keep: &dyn Fn(&str) -> bool| -> Vec<String> {
        session
            .catalog()
            .mutants()
            .iter()
            .map(|mutant| mutant.candidate.path.clone())
            .filter(|path| keep(path))
            .collect()
    };
    let changed = paths(&whole, &|path| path == "src/lib.rs");
    assert!(
        !changed.is_empty() && paths(&narrowed, &|_| true) == changed,
        "a change set still decides what is mutated: exactly the whole run's mutants of the \
         changed file"
    );
    assert_eq!(
        narrowed.touched().items,
        whole.touched().items,
        "and every file the configuration selects carries its entry markers whatever the change \
         set, so what an execution entered is named the same way on a pull request as on the \
         whole run it is carried from"
    );
    let marked = std::fs::read_to_string(narrowed.snapshot_root().join("src/spin.rs"))
        .expect("the instrumented file");
    let pristine =
        std::fs::read_to_string(fixture.root().join("src/spin.rs")).expect("the pristine file");
    assert_ne!(
        marked, pristine,
        "and a file the change set left out is instrumented for entry all the same"
    );
    narrowed.close().expect("close");
    whole.close().expect("close");
}

/// What the program compiled from `source` says `line!()` and `column!()` are where its probe calls them, and where the evidence places that call.
fn reported_and_placed(source: &[u8]) -> ((u32, u32), Option<rust_mutants::skeleton::Position>) {
    let dir = tempfile::tempdir().expect("a directory");
    let path = dir.path().join("probe.rs");
    std::fs::write(&path, source).expect("the probe");
    let built = std::process::Command::new("rustc")
        .args(["--edition", "2024", "--out-dir"])
        .arg(dir.path())
        .arg(&path)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("rustc runs");
    assert!(
        built.status.success(),
        "{}",
        njutest_devkit::process::strict_utf8(&built.stderr)
    );
    let ran = std::process::Command::new(dir.path().join("probe"))
        .output()
        .expect("the probe runs");
    let said = njutest_devkit::process::strict_utf8(&ran.stdout);
    let mut numbers = said
        .split_whitespace()
        .map(|number| number.parse::<u32>().expect("the probe prints two numbers"));
    let reported = (
        numbers.next().expect("a line"),
        numbers.next().expect("a column"),
    );
    let at = source
        .windows(b"column!()".len())
        .position(|window| window == b"column!()")
        .expect("the probe calls column!");
    let at = u32::try_from(at).expect("a small probe");
    (reported, rust_mutants::skeleton::position(source, at))
}

#[test]
fn the_evidence_places_a_token_where_the_compiler_reports_it() {
    const PROBE: &str = "fn probe() -> (u32, u32) {(line!(), column!())}";
    const MAIN: &str =
        "fn main() { let (line, column) = probe(); println!(\"{line} {column}\"); }\n";
    for (case, source) in [
        (
            "a multi-byte character, a four-byte character and a tab before it on its line",
            format!("/* \u{e9}\u{1f980}\t */ {PROBE}\n{MAIN}").into_bytes(),
        ),
        (
            "a carriage return and a line feed ending the line before it",
            format!("// before\r\n{PROBE}\n{MAIN}").into_bytes(),
        ),
        (
            "a carriage return alone earlier on its line",
            format!("/*\r*/ {PROBE}\n{MAIN}").into_bytes(),
        ),
        (
            "a byte-order mark at the start of the file, on its line",
            [
                b"\xEF\xBB\xBF".as_slice(),
                format!("{PROBE}\n{MAIN}").as_bytes(),
            ]
            .concat(),
        ),
    ] {
        let ((line, column), placed) = reported_and_placed(&source);
        assert_eq!(
            placed,
            Some(rust_mutants::skeleton::Position { line, column }),
            "with {case}, the evidence places a token where the compiled program reports it, since \
             that is the position an execution can read and the carry rule compares"
        );
    }
}
