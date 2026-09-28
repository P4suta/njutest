// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How the engine tells two environment variable names apart, held to the table xtask's reader is held to as well.

#![expect(
    clippy::panic,
    reason = "a test reports a malformed table by panicking"
)]

use std::ffi::OsStr;

use rust_mutants::vars::Spelling;

fn rule(word: &str) -> Spelling {
    match word {
        "exact" => Spelling::Exact,
        "ascii-caseless" => Spelling::AsciiCaseless,
        other => panic!("the table names no rule {other:?}"),
    }
}

fn answer(word: &str) -> bool {
    match word {
        "yes" => true,
        "no" => false,
        other => panic!("the table answers neither yes nor no: {other:?}"),
    }
}

#[test]
fn every_name_is_read_as_the_table_says() {
    let table = std::fs::read_to_string(
        njutest_devkit::paths::workspace_root()
            .join("crates/rust-mutants/tests/testdata/spelling.tsv"),
    )
    .unwrap_or_else(|error| panic!("the shared table is readable: {error}"));
    let this_platform = if cfg!(windows) { "windows" } else { "unix" };
    let mut hosts = 0_usize;
    for line in table
        .lines()
        .filter(|line| !line.starts_with('#') && !line.starts_with("question\t"))
    {
        let [question, spelled, name, other, expected] =
            line.split('\t').collect::<Vec<&str>>()[..]
        else {
            panic!("a row has five cells: {line:?}")
        };
        let spelling = rule(spelled);
        let expected = answer(expected);
        match question {
            "same" => assert_eq!(
                spelling.same(OsStr::new(name), OsStr::new(other)),
                expected,
                "{line}"
            ),
            "begins" => assert_eq!(spelling.begins(OsStr::new(name), other), expected, "{line}"),
            "host" if name == this_platform => {
                hosts = hosts.saturating_add(usize::from(expected));
                assert_eq!(Spelling::HOST == spelling, expected, "{line}");
            }
            "host" => {}
            other => panic!("the table asks no question {other:?}"),
        }
    }
    assert_eq!(
        hosts, 1,
        "the table names exactly one rule this platform reads names by"
    );
}
