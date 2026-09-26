// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That what a run's identity takes from the environment is what the compiler would see, not where a checkout or a toolchain happens to sit.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::ffi::OsString;
use std::path::Path;

use njutest::assure::identity::selected;

fn tool(directory: &Path, name: &str, bytes: &[u8]) -> OsString {
    std::fs::create_dir_all(directory).expect("a directory for the tool");
    let path = directory.join(name);
    std::fs::write(&path, bytes).expect("the tool");
    path.into_os_string()
}

fn vars(pairs: &[(&str, OsString)]) -> rust_mutants::vars::Variables {
    pairs
        .iter()
        .map(|(name, value)| (OsString::from(name), value.clone()))
        .collect()
}

#[test]
fn the_same_compiler_at_two_paths_is_one_identity_and_another_compiler_is_not() {
    let temp = tempfile::tempdir().expect("tempdir");
    let here = tool(&temp.path().join("one/bin"), "rustc", b"the same compiler");
    let there = tool(&temp.path().join("two/bin"), "rustc", b"the same compiler");
    let other = tool(&temp.path().join("three/bin"), "rustc", b"another compiler");
    let at =
        |value: &OsString| selected(&vars(&[("RUSTC", value.clone())]), &[]).expect("selected");
    assert_eq!(
        at(&here),
        at(&there),
        "a compiler is the program it is, not the directory it was unpacked into, so two \
         checkouts on two runners with one toolchain are one identity"
    );
    assert_ne!(
        at(&here),
        at(&other),
        "and a different program under the same name is a different compiler"
    );
}

#[test]
fn where_cargo_writes_is_no_part_of_what_a_run_is() {
    let one = selected(
        &vars(&[("CARGO_TARGET_DIR", OsString::from("/runner/one/target"))]),
        &[],
    )
    .expect("selected");
    let two = selected(
        &vars(&[("CARGO_TARGET_DIR", OsString::from("/runner/two/target"))]),
        &[],
    )
    .expect("selected");
    assert_eq!(
        one, two,
        "njutest sets the target directory of every build it starts, so a person's own \
         setting changes nothing the tests see"
    );
}

#[test]
fn a_program_named_bare_is_the_one_the_runs_own_path_finds() {
    let temp = tempfile::tempdir().expect("tempdir");
    let one = temp.path().join("one/bin");
    let two = temp.path().join("two/bin");
    tool(&one, "sccache", b"one wrapper");
    tool(&two, "sccache", b"one wrapper");
    let on = |directory: &Path| {
        selected(
            &vars(&[
                ("RUSTC_WRAPPER", OsString::from("sccache")),
                ("PATH", directory.as_os_str().to_owned()),
            ]),
            &[],
        )
        .expect("selected")
    };
    assert_eq!(
        on(&one),
        on(&two),
        "a wrapper named the way people write it is the program the run's PATH finds, wherever \
         that PATH put it"
    );
    assert!(
        on(&one)
            .iter()
            .any(|(name, value)| name == "RUSTC_WRAPPER" && value.starts_with("sccache@sha256:")),
        "and it is folded as that program: {:?}",
        on(&one)
    );
}
