// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading the environment a run was given, the way the platform spells names.

use std::ffi::OsString;

use rust_mutants::vars::{Spelling, Variables};

/// The variables `named`, in this order, under `spelling`.
fn spelled(spelling: Spelling, named: &[(&str, &str)]) -> Variables {
    Variables::spelled(
        spelling,
        named
            .iter()
            .map(|(name, value)| (OsString::from(*name), OsString::from(*value))),
    )
}

/// An environment of exactly these variables, in this order, as this platform reads it.
fn given(named: &[(&str, &str)]) -> Variables {
    spelled(Spelling::HOST, named)
}

#[test]
fn a_name_spelled_the_way_it_was_set_is_found_everywhere() {
    assert_eq!(
        given(&[("PATH", "/usr/bin")]).var("PATH"),
        Some(std::ffi::OsStr::new("/usr/bin")),
        "the ordinary case is a name asked for as it was set, and a machine where that \
         did not answer would be one where nothing could be found at all"
    );
}

#[test]
fn a_name_spelled_another_way_is_the_same_name_only_where_the_platform_says_so() {
    let windows = given(&[("Path", "C:\\bin")]);
    assert_eq!(
        windows.var("PATH").is_some(),
        cfg!(windows),
        "Windows spells its search path `Path` and treats the name as one name however it \
         is written, so a run given that environment has a search path; a unix machine \
         where `Path` and `PATH` are two variables must not answer one with the other"
    );
}

#[test]
fn the_search_path_is_the_one_the_environment_names() {
    assert_eq!(
        given(&[("PATH", "/usr/bin")])
            .search_path()
            .map(ToOwned::to_owned),
        Some(OsString::from("/usr/bin")),
        "resolving a bare program name is what the search path is for, and a run that \
         could not read the one it was handed refuses every bare name it is given"
    );
    assert_eq!(
        given(&[("HOME", "/home/somebody")])
            .search_path()
            .map(ToOwned::to_owned),
        None,
        "an environment that names no search path names none, and answering with \
         something else would resolve a program against a path nobody gave"
    );
}

#[test]
fn a_variable_of_another_name_is_not_the_one_asked_for() {
    assert_eq!(
        given(&[("PATHEXT", ".EXE"), ("MANPATH", "/usr/share/man")]).var("PATH"),
        None,
        "the names are the names, and matching a prefix or a suffix of one would read a \
         variable somebody set for something else"
    );
}

#[test]
fn the_first_of_a_repeated_variable_is_the_one_read() {
    assert_eq!(
        given(&[("PATH", "/first"), ("PATH", "/second")]).var("PATH"),
        Some(std::ffi::OsStr::new("/first")),
        "an environment is a list and a shell may hand over the same name twice; reading \
         the first is what a process does with `getenv`, and answering differently would \
         run a different program than the one the environment says"
    );
}

#[test]
fn every_rule_holds_a_variable_once_however_its_name_was_spelled() {
    for spelling in Spelling::ALL {
        let variables = spelled(spelling, &[("Path", "C:\\first"), ("PATH", "C:\\second")]);
        let (held, second) = match spelling {
            Spelling::Exact => (2, "C:\\second"),
            Spelling::AsciiCaseless => (1, "C:\\first"),
        };
        assert_eq!(
            (
                variables.len(),
                variables.var("Path"),
                variables.var("PATH")
            ),
            (
                held,
                Some(std::ffi::OsStr::new("C:\\first")),
                Some(std::ffi::OsStr::new(second))
            ),
            "{spelling:?}: where two spellings are one name, the first is the one `getenv` \
             reads, and it is the only one held"
        );
    }
}

#[test]
fn setting_a_name_replaces_every_spelling_the_rule_takes_as_it() {
    for spelling in Spelling::ALL {
        let mut variables = spelled(spelling, &[("UserProfile", "C:\\Users\\given")]);
        variables.set("USERPROFILE", "C:\\scratch\\home");
        let seen: Vec<_> = variables.for_process().collect();
        let expected: Vec<(&std::ffi::OsStr, &std::ffi::OsStr)> = match spelling {
            Spelling::Exact => vec![
                ("UserProfile".as_ref(), "C:\\Users\\given".as_ref()),
                ("USERPROFILE".as_ref(), "C:\\scratch\\home".as_ref()),
            ],
            Spelling::AsciiCaseless => {
                vec![("USERPROFILE".as_ref(), "C:\\scratch\\home".as_ref())]
            }
        };
        assert_eq!(
            seen, expected,
            "{spelling:?}: a home composed for a test replaces the given one however the \
             parent spelled it, or the process would be handed both and keep the given one"
        );
        variables.remove("userprofile");
        assert_eq!(
            variables.is_empty(),
            spelling == Spelling::AsciiCaseless,
            "{spelling:?}: removing a name removes what the rule takes as it, and nothing else"
        );
    }
}

#[test]
fn a_digest_reads_one_spelling_of_each_variable_in_name_order() {
    for spelling in Spelling::ALL {
        let one = spelled(
            spelling,
            &[("Rustflags", "-Cdebuginfo=0"), ("CARGO_HOME", "/c")],
        );
        let other = spelled(
            spelling,
            &[("CARGO_HOME", "/c"), ("RUSTFLAGS", "-Cdebuginfo=0")],
        );
        assert_eq!(
            one.canonical() == other.canonical(),
            spelling == Spelling::AsciiCaseless,
            "{spelling:?}: `Rustflags` and `RUSTFLAGS` are one variable only where the rule \
             says so, and there a digest of either environment is one digest"
        );
    }
}

#[test]
fn a_prefix_selects_under_the_rule_the_names_are_read_by() {
    for spelling in Spelling::ALL {
        let variables = spelled(
            spelling,
            &[
                ("cargo_home", "/c"),
                ("CARGO_TARGET_DIR", "/t"),
                ("RUSTUP_HOME", "/r"),
            ],
        );
        let names: Vec<OsString> = variables
            .prefixed("CARGO_")
            .into_iter()
            .map(|(name, _value)| name)
            .collect();
        let expected: Vec<OsString> = match spelling {
            Spelling::Exact => vec!["CARGO_TARGET_DIR".into()],
            Spelling::AsciiCaseless => vec!["CARGO_HOME".into(), "CARGO_TARGET_DIR".into()],
        };
        assert_eq!(
            names, expected,
            "{spelling:?}: a selection by prefix is a selection by name, so it reads a name \
             as the rule does and gives each the spelling a digest reads"
        );
    }
}

#[test]
fn keeping_named_variables_keeps_them_however_they_were_spelled() {
    for spelling in Spelling::ALL {
        let variables = spelled(spelling, &[("Tz", "UTC"), ("HOME", "/h"), ("LANG", "C")]);
        let kept = variables.only(&["TZ", "LANG"]);
        assert_eq!(
            kept.len(),
            match spelling {
                Spelling::Exact => 1,
                Spelling::AsciiCaseless => 2,
            },
            "{spelling:?}: {kept:?}"
        );
    }
}

#[test]
fn the_host_rule_is_the_one_every_reading_of_a_name_uses() {
    assert_eq!(
        Spelling::HOST,
        if cfg!(windows) {
            Spelling::AsciiCaseless
        } else {
            Spelling::Exact
        }
    );
    assert_eq!(
        given(&[("Path", "C:\\bin")]).search_path().is_some(),
        cfg!(windows),
        "the variables a run is given are read by the platform's own rule"
    );
}

#[test]
fn the_test_support_reads_a_name_by_the_rule_the_engine_does() {
    for (one, other) in [
        ("PATH", "PATH"),
        ("Path", "PATH"),
        ("path", "PATH"),
        ("PATHEXT", "PATH"),
        ("Rust_Mutants_Active", "RUST_MUTANTS_ACTIVE"),
    ] {
        let (one, other) = (std::ffi::OsStr::new(one), std::ffi::OsStr::new(other));
        assert_eq!(
            njutest_devkit::paths::same_name(one, other),
            Spelling::HOST.same(one, other),
            "the devkit cannot depend on the engine and keeps its own reading of a name, which \
             the suites use to compose what a run is given; on every platform the suite runs \
             on, it is the engine's: {one:?} and {other:?}"
        );
    }
}
