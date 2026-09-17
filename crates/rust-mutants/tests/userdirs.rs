// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where a run keeps what it establishes between runs, and what it refuses to keep it in.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rust_mutants::userdirs::cache_directory;

/// An environment of exactly these variables, in this order.
fn given(named: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
    named
        .iter()
        .map(|(name, value)| (OsString::from(*name), OsString::from(*value)))
        .collect()
}

/// A path that is absolute on the machine the test runs on, from a slash-separated tail.
fn absolute(tail: &str) -> String {
    if cfg!(windows) {
        format!("C:\\{}", tail.replace('/', "\\"))
    } else {
        format!("/{tail}")
    }
}

#[test]
fn the_directory_the_platform_names_is_the_one_used() {
    assert_eq!(
        cache_directory(
            &given(&[("XDG_CACHE_HOME", &absolute("var/cache/mine"))]),
            ".fallback"
        ),
        PathBuf::from(absolute("var/cache/mine")),
        "a person who moved their cache moved it, and a tool that kept its own place \
         would fill a disk they had made room on"
    );
}

#[test]
fn a_home_directory_gives_the_place_below_it_every_other_tool_uses() {
    assert_eq!(
        cache_directory(&given(&[("HOME", &absolute("home/somebody"))]), ".fallback"),
        Path::new(&absolute("home/somebody")).join(".cache"),
        "with nothing naming the cache, the convention every tool on the platform \
         follows is where a person will look for it"
    );
}

#[test]
fn the_variable_that_names_the_cache_wins_over_the_one_that_names_a_home() {
    assert_eq!(
        cache_directory(
            &given(&[
                ("HOME", &absolute("home/somebody")),
                ("XDG_CACHE_HOME", &absolute("var/cache/mine")),
                ("LOCALAPPDATA", &absolute("Users/somebody/AppData/Local")),
            ]),
            ".fallback"
        ),
        PathBuf::from(absolute("var/cache/mine")),
        "the three are asked in the order they are specific, and one that named the \
         cache itself said the most"
    );
    assert_eq!(
        cache_directory(
            &given(&[
                ("LOCALAPPDATA", &absolute("Users/somebody/AppData/Local")),
                ("HOME", &absolute("home/somebody")),
            ]),
            ".fallback"
        ),
        Path::new(&absolute("home/somebody")).join(".cache"),
        "and a home says more than the place a Windows program keeps its own, which is \
         the last one asked because it is the one a unix machine also sets under wine"
    );
}

#[test]
fn a_place_that_is_not_an_absolute_path_is_not_a_place() {
    for relative in ["relative/cache", ".", "", "~/cache"] {
        assert_eq!(
            cache_directory(
                &given(&[
                    ("XDG_CACHE_HOME", relative),
                    ("HOME", &absolute("home/somebody"))
                ]),
                ".fallback"
            ),
            Path::new(&absolute("home/somebody")).join(".cache"),
            "{relative:?} resolves against the working directory, which is the tree a run \
             measures: the run's own writes would change the tree it is measuring, and the \
             sweep that empties the cache would take somebody's source with it"
        );
    }
}

#[test]
fn an_environment_that_names_nowhere_falls_back_on_what_the_product_was_given() {
    assert_eq!(
        cache_directory(&given(&[]), ".rust-mutants-cache"),
        PathBuf::from(".rust-mutants-cache"),
        "a machine with no home and no cache still runs, and the name it falls back on is \
         the product's own so two of them never share one"
    );
    assert_ne!(
        cache_directory(&given(&[]), ".rust-mutants-cache"),
        cache_directory(&given(&[]), ".njutest-cache"),
        "which is the one thing the two products do not share"
    );
}

#[test]
fn a_variable_of_another_name_is_not_one_of_these() {
    assert_eq!(
        cache_directory(
            &given(&[
                ("XDG_CACHE_HOME_OLD", &absolute("var/cache/mine")),
                ("CACHE_HOME", &absolute("var/cache/other")),
                ("HOMEPATH", &absolute("home/somebody")),
            ]),
            ".fallback"
        ),
        PathBuf::from(".fallback"),
        "the names are the names, and matching a prefix of one would take a variable \
         somebody set for something else"
    );
}

#[test]
fn the_first_of_a_repeated_variable_is_the_one_read() {
    assert_eq!(
        cache_directory(
            &given(&[
                ("XDG_CACHE_HOME", &absolute("var/cache/first")),
                ("XDG_CACHE_HOME", &absolute("var/cache/second")),
            ]),
            ".fallback"
        ),
        PathBuf::from(absolute("var/cache/first")),
        "an environment is a list and a shell may hand over the same name twice; reading \
         the first is what a process does with `getenv`, and answering differently would \
         put the cache somewhere the run's own children do not look"
    );
}
