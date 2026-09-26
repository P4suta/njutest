// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A crate that forbids what the guards allow is one no mutant of it can compile in.

#![expect(
    clippy::panic,
    reason = "a test reports a reading that could not happen by panicking"
)]

use rust_mutants::cargo::manifest::read_forbidden;

/// Whether the crate root `source` forbids what the guards allow, once it has been read.
fn forbids_guard_noise(source: &str, forbidden: &[String]) -> bool {
    match rust_mutants::discover::forbids_guard_noise(source, forbidden) {
        Ok(forbids) => forbids,
        Err(unread) => panic!("the crate root is read: {unread}"),
    }
}

#[test]
fn only_the_two_generated_module_lints_and_their_groups_conflict() {
    for lint in ["warnings", "unused", "dead_code", "unused_qualifications"] {
        assert!(forbids_guard_noise("pub fn f() {}\n", &[lint.to_owned()]));
    }
    for lint in ["clippy::all", "missing_docs", "unsafe_code"] {
        assert!(!forbids_guard_noise("pub fn f() {}\n", &[lint.to_owned()]));
    }
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
        forbids_guard_noise(
            "#![cfg_attr(any(target_os = \"linux\", target_os = \"macos\"), forbid(dead_code))]\n",
            &[]
        ),
        "a conditional forbid may be active in the build and must fail closed"
    );
    assert!(
        !forbids_guard_noise("#![forbid(unsafe_code)]\n", &[]),
        "a lint the generated module never allows is a lint a crate is free to forbid"
    );
    assert!(
        !forbids_guard_noise("pub mod inner {\n    #![forbid(warnings)]\n}\n", &[]),
        "the generated module is at the file root, outside this inline module"
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
