// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! How xtask reads the environment it was started with: by the rule its host tells two names apart by, held to the table the engine's `vars::Spelling` is held to as well.

#![expect(
    clippy::panic,
    reason = "a test reports a malformed table by panicking"
)]

use std::ffi::{OsStr, OsString};

use xtask::environment::{Environment, Spelling};

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
fn every_name_is_read_as_the_table_the_engine_is_held_to_says() {
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
        "the table names exactly one rule this platform reads names by, and xtask's host is the \
         engine's"
    );
}

fn spelled(spelling: Spelling, named: &[(&str, &str)]) -> Environment {
    Environment::spelled(
        spelling,
        named
            .iter()
            .map(|(name, value)| (OsString::from(*name), OsString::from(*value))),
    )
}

#[test]
fn a_variable_is_read_by_the_rule_its_environment_is_spelled_under() {
    for spelling in Spelling::ALL {
        let caseless = spelling == Spelling::AsciiCaseless;
        let environment = spelled(
            spelling,
            &[
                ("njutest_slot_dir", "/slots"),
                ("Git_Dir", "/elsewhere/.git"),
                ("GIT_WORK_TREE", "/elsewhere"),
                ("cargo_home", "/cargo"),
                ("PATH", "/bin"),
            ],
        );
        assert_eq!(
            environment.value("NJUTEST_SLOT_DIR").is_some(),
            caseless,
            "{spelling:?}: Windows takes `njutest_slot_dir` for `NJUTEST_SLOT_DIR`, and a lane \
             read by bytes there was kept somewhere the person who set it never asked for"
        );
        let removed: Vec<&OsStr> = environment.beginning("GIT_").collect();
        assert_eq!(
            removed.contains(&OsStr::new("Git_Dir")),
            caseless,
            "{spelling:?}: git on Windows reads `Git_Dir` as `GIT_DIR`, so a hook's variable \
             compared by bytes reached the git that was asked about another directory: \
             {removed:?}"
        );
        assert!(
            removed.contains(&OsStr::new("GIT_WORK_TREE")),
            "{spelling:?}: {removed:?}"
        );
        let building: Vec<OsString> = environment
            .canonical(|name| xtask::prepush::shapes_the_build(spelling, name))
            .into_iter()
            .map(|(name, _value)| name)
            .collect();
        assert_eq!(
            building,
            if caseless {
                vec![OsString::from("CARGO_HOME")]
            } else {
                Vec::new()
            },
            "{spelling:?}: what a remembered pass answers for is every variable cargo reads, by \
             the one spelling its rule gives it, so a lowercase one is neither missed nor a \
             second identity"
        );
    }
}
