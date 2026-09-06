// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate that forbids what the guards allow is one no mutant of it can compile in.

use rust_mutants::cargo::manifest::read_forbidden;
use rust_mutants::discover::forbids_guard_noise;
use rust_mutants::instrument::{ALLOW_ATTRIBUTE, GUARD_NOISE_LINTS};

#[test]
fn every_lint_the_guards_allow_is_one_the_attribute_names() {
    for lint in GUARD_NOISE_LINTS {
        assert!(
            ALLOW_ATTRIBUTE.contains(lint),
            "{lint} is not in the attribute the guards carry, so forbidding it would be read \
             as a reason the run does not have"
        );
    }
    let named = ALLOW_ATTRIBUTE.matches(',').count().saturating_add(1);
    assert_eq!(
        GUARD_NOISE_LINTS.len(),
        named,
        "the attribute names a lint this list does not, so a crate could forbid it and every \
         mutant would be refused with nobody able to say why"
    );
}

#[test]
fn forbid_is_read_from_the_crate_root_only_and_only_for_the_lints_guards_fire() {
    assert!(forbids_guard_noise(
        "#![forbid(unused_qualifications)]\npub fn f() {}\n",
        &[]
    ));
    assert!(
        forbids_guard_noise("#![forbid(warnings)]\n", &[]),
        "forbidding warnings forbids every lint the guards turn off"
    );
    assert!(
        !forbids_guard_noise("#![deny(unused_qualifications)]\n", &[]),
        "an allow attribute overrides a deny, which is what the guards carry one for"
    );
    assert!(
        !forbids_guard_noise("#![forbid(unsafe_code)]\n", &[]),
        "a lint the guards never fire is a lint a crate is free to forbid"
    );
    assert!(
        !forbids_guard_noise("pub mod inner {\n    #![forbid(warnings)]\n}\n", &[]),
        "an attribute inside an item is about that item, and the crate root is what a run reads"
    );
    assert!(
        !forbids_guard_noise("this is not rust [[[", &[]),
        "a root nobody can parse is reported by the walk, not guessed at here"
    );
}

#[test]
fn a_manifest_that_forbids_is_read_too_because_cargo_passes_it_on_the_command_line() {
    let forbidden = read_forbidden(
        "[lints.rust]\nunused_qualifications = \"forbid\"\nunused = { level = \"deny\" }\n\n\
         [lints.clippy]\npedantic = { level = \"forbid\", priority = -1 }\n",
        None,
    );
    assert_eq!(
        forbidden,
        vec![
            "clippy::pedantic".to_owned(),
            "unused_qualifications".to_owned()
        ]
    );
    assert!(
        forbids_guard_noise("pub fn f() {}\n", &forbidden),
        "cargo passes a forbid from the manifest on the command line, where an attribute in \
         the source overrides nothing"
    );
}

#[test]
fn a_member_that_inherits_the_workspace_lints_inherits_what_they_forbid() {
    let workspace = "[workspace.lints.rust]\nunused_qualifications = \"forbid\"\n";
    assert_eq!(
        read_forbidden("[lints]\nworkspace = true\n", Some(workspace)),
        vec!["unused_qualifications".to_owned()]
    );
    assert!(
        read_forbidden("[package]\nname = \"a\"\n", Some(workspace)).is_empty(),
        "a member that does not ask to inherit them does not have them"
    );
}
