// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading the environment a run was given, the way the platform spells names.

use std::ffi::OsString;

use rust_mutants::vars::{search_path, var};

/// An environment of exactly these variables, in this order.
fn given(named: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    named
        .iter()
        .map(|(name, value)| (OsString::from(*name), OsString::from(*value)))
        .collect()
}

#[test]
fn a_name_spelled_the_way_it_was_set_is_found_everywhere() {
    assert_eq!(
        var(&given(&[("PATH", "/usr/bin")]), "PATH"),
        Some(std::ffi::OsStr::new("/usr/bin")),
        "the ordinary case is a name asked for as it was set, and a machine where that \
         did not answer would be one where nothing could be found at all"
    );
}

#[test]
fn a_name_spelled_another_way_is_the_same_name_only_where_the_platform_says_so() {
    let windows = given(&[("Path", "C:\\bin")]);
    assert_eq!(
        var(&windows, "PATH").is_some(),
        cfg!(windows),
        "Windows spells its search path `Path` and treats the name as one name however it \
         is written, so a run given that environment has a search path; a unix machine \
         where `Path` and `PATH` are two variables must not answer one with the other"
    );
}

#[test]
fn the_search_path_is_the_one_the_environment_names() {
    assert_eq!(
        search_path(&given(&[("PATH", "/usr/bin")])),
        Some(OsString::from("/usr/bin")),
        "resolving a bare program name is what the search path is for, and a run that \
         could not read the one it was handed refuses every bare name it is given"
    );
    assert_eq!(
        search_path(&given(&[("HOME", "/home/somebody")])),
        None,
        "an environment that names no search path names none, and answering with \
         something else would resolve a program against a path nobody gave"
    );
}

#[test]
fn a_variable_of_another_name_is_not_the_one_asked_for() {
    assert_eq!(
        var(
            &given(&[("PATHEXT", ".EXE"), ("MANPATH", "/usr/share/man")]),
            "PATH"
        ),
        None,
        "the names are the names, and matching a prefix or a suffix of one would read a \
         variable somebody set for something else"
    );
}

#[test]
fn the_first_of_a_repeated_variable_is_the_one_read() {
    assert_eq!(
        var(&given(&[("PATH", "/first"), ("PATH", "/second")]), "PATH"),
        Some(std::ffi::OsStr::new("/first")),
        "an environment is a list and a shell may hand over the same name twice; reading \
         the first is what a process does with `getenv`, and answering differently would \
         run a different program than the one the environment says"
    );
}
