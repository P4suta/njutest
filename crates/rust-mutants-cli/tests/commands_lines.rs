// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The literal text the commands that measure nothing answer with.

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

/// What one command said, driven in this process.
struct Said {
    code: u8,
    out: String,
    err: String,
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        ci: rust_mutants_cli::CiHost::None,
    }
}

fn asked(fixture: &Fixture, args: &[&str]) -> Said {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    Said {
        code,
        out: njutest_devkit::process::strict_utf8(&out).into_owned(),
        err: njutest_devkit::process::strict_utf8(&err).into_owned(),
    }
}

#[test]
fn the_tiers_are_a_chain_and_the_widest_of_them_is_the_whole_table() {
    let fixture = Fixture::copy("fixture-simple");
    let whole = rows(&asked(&fixture, &["rules"]).out);

    let mut previous: Vec<String> = Vec::new();
    for tier in rust_mutants::rule::Tier::ALL {
        let named = tier.name();
        let listed = asked(&fixture, &["rules", "--tier", named]);
        assert_eq!(listed.code, 0, "{}{}", listed.out, listed.err);
        let selected = rows(&listed.out);
        assert!(
            !selected.is_empty(),
            "{named} selects operators, or the tier is a name with nothing behind it"
        );
        assert!(
            previous.iter().all(|row| selected.contains(row)),
            "and a wider tier adds to the narrower one rather than trading rules with \
             it: {named} dropped one of the {} the tier before it selected:\n{}",
            previous.len(),
            listed.out
        );
        assert!(
            selected.len() >= previous.len(),
            "which is what puts them in an order at all"
        );
        previous = selected;
    }
    assert_eq!(
        previous, whole,
        "and the widest of them is the table itself, or `rules` and `rules --tier all` \
         describe two different releases"
    );
}

/// The rows of an operator table, which is every line that is one.
fn rows(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| line.split_whitespace().count() == 4 && !line.starts_with("FAMILY"))
        .map(ToOwned::to_owned)
        .collect()
}

#[test]
fn a_tier_that_is_not_one_is_refused_by_naming_the_ones_there_are() {
    let fixture = Fixture::copy("fixture-simple");
    let refused = asked(&fixture, &["rules", "--tier", "thorough"]);
    assert_ne!(
        refused.code, 0,
        "a tier nobody defined selects nothing, and a run that answered with an empty \
         table would read as a release that dropped its operators: {}{}",
        refused.out, refused.err
    );
    for named in rust_mutants::rule::Tier::ALL.map(rust_mutants::rule::Tier::name) {
        assert!(
            refused.err.contains(named),
            "and the refusal lists the tiers there are, which is the only place a person \
             finds out: {named} is missing from {}",
            refused.err
        );
    }
    assert!(
        refused.err.contains("thorough"),
        "along with what was asked for: {}",
        refused.err
    );
}

#[test]
fn a_file_a_run_is_narrowed_to_takes_a_line_a_range_or_neither() {
    let fixture = Fixture::copy("fixture-simple");
    for accepted in ["src/lib.rs", "src/lib.rs:3", "src/lib.rs:2-9"] {
        let said = asked(
            &fixture,
            &[
                "run",
                "--offline",
                "--locked",
                "--file",
                accepted,
                "--dry-run",
            ],
        );
        assert!(
            !said.err.contains("PATH, PATH:LINE"),
            "{accepted} is one of the three shapes the flag documents: {}{}",
            said.out,
            said.err
        );
    }
}

#[test]
fn a_range_that_addresses_nothing_is_refused_by_naming_the_shapes_that_do() {
    let fixture = Fixture::copy("fixture-simple");
    for wrong in [
        "src/lib.rs:0",
        "src/lib.rs:0-4",
        "src/lib.rs:9-3",
        "src/lib.rs:two",
        "src/lib.rs:3-",
        "src/lib.rs:-3",
    ] {
        let said = asked(
            &fixture,
            &["run", "--offline", "--locked", "--file", wrong, "--dry-run"],
        );
        assert_ne!(
            said.code, 0,
            "{wrong} addresses no line of any file, and a run that took it would measure \
             a selection nobody asked for and report it as the whole: {}{}",
            said.out, said.err
        );
        assert!(
            said.err.contains("PATH, PATH:LINE, or PATH:FROM-TO") && said.err.contains(wrong),
            "and the refusal says what was passed and what the flag takes, because a \
             range is easy to get one way round: {}",
            said.err
        );
    }
}

#[test]
fn a_line_on_its_own_is_the_range_of_that_one_line() {
    let fixture = Fixture::copy("fixture-simple");
    let one = tallied(&asked(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--file",
            "src/lib.rs:4",
            "--dry-run",
        ],
    ));
    let same = tallied(&asked(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--file",
            "src/lib.rs:4-4",
            "--dry-run",
        ],
    ));
    assert!(
        !one.is_empty(),
        "a dry run says what it would start, or there is nothing here to compare"
    );
    assert_eq!(
        one, same,
        "PATH:N and PATH:N-N select the same line, or a person who wrote the short form \
         measured something else"
    );
}

/// What a dry run said it would do, without the times it took to say it.
fn tallied(said: &Said) -> String {
    said.out
        .lines()
        .skip_while(|line| !line.starts_with("WOULD START"))
        .collect::<Vec<&str>>()
        .join("\n")
}

#[test]
fn a_path_with_a_colon_in_it_and_no_line_after_it_is_a_path() {
    let fixture = Fixture::copy("fixture-simple");
    let said = asked(
        &fixture,
        &[
            "run",
            "--offline",
            "--locked",
            "--file",
            "src/lib.rs:",
            "--dry-run",
        ],
    );
    assert_ne!(
        said.code, 0,
        "a colon with nothing after it names no line, and reading it as the whole file \
         would measure everything while the person believes one line was selected: {}{}",
        said.out, said.err
    );
}
