// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A target directory forgets every unit of a member whose files moved since it last built it, and only those.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use std::path::{Path, PathBuf};

use rust_mutants::cargo::{BuildDir, LEDGER_NAME, Member, MemberFile, fingerprint_of};

const HASH: &str = "0123456789abcdef";

fn member(tree: &Path, name: &str, files: &[&str]) -> Member {
    Member {
        name: name.to_owned(),
        files: files
            .iter()
            .map(|rel_path| MemberFile {
                rel_path: (*rel_path).to_owned(),
                path: tree.join(rel_path),
            })
            .collect(),
    }
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("a file has a directory"))
        .expect("the directory is made");
    std::fs::write(path, text).expect("the file is written");
}

/// Every fingerprint directory a test lays out: two profiles, with and without a target triple, for each member.
fn fingerprints(target: &Path) -> Vec<PathBuf> {
    ["a", "b"]
        .iter()
        .flat_map(|name| {
            [
                target.join(format!("debug/.fingerprint/{name}-{HASH}")),
                target.join(format!(
                    "x86_64-unknown-linux-gnu/release/.fingerprint/{name}-{HASH}"
                )),
            ]
        })
        .collect()
}

fn lay_out(target: &Path) {
    for fingerprint in fingerprints(target) {
        write(&fingerprint.join("lib-crate"), "fresh");
    }
}

fn present(target: &Path) -> Vec<String> {
    fingerprints(target)
        .iter()
        .filter(|path| std::fs::symlink_metadata(path).is_ok())
        .map(|path| {
            path.strip_prefix(target)
                .expect("under the target")
                .display()
                .to_string()
        })
        .collect()
}

#[test]
fn a_fingerprint_is_the_package_name_a_hyphen_and_sixteen_lowercase_hex_digits() {
    assert!(fingerprint_of(
        "witness-upstream-22c12a902edac2fb",
        "witness-upstream"
    ));
    for (name, member) in [
        ("witness-upstream-22c12a902edac2fb", "witness"),
        ("witness-upstream-22C12A902EDAC2FB", "witness-upstream"),
        ("witness-upstream-22c12a902edac2f", "witness-upstream"),
        ("witness-upstream-22c12a902edac2fbb", "witness-upstream"),
        ("witness-upstream22c12a902edac2fb", "witness-upstream"),
        ("other-22c12a902edac2fb", "witness-upstream"),
    ] {
        assert!(
            !fingerprint_of(name, member),
            "{name} is not a unit of {member}: a member whose name begins another's must not \
             take that one's units with it"
        );
    }
}

#[test]
fn a_member_whose_files_moved_loses_every_fingerprint_and_the_others_keep_theirs() {
    let temp = tempfile::tempdir().expect("a directory");
    let tree = temp.path().join("tree");
    let target = temp.path().join("target");
    write(&tree.join("a/src/lib.rs"), "pub fn a() {}\n");
    write(&tree.join("b/src/lib.rs"), "pub fn b() {}\n");
    let nested = target.join("witness");
    write(&nested.join(LEDGER_NAME), "a record of its own");
    let kept_apart = nested.join(format!("debug/.fingerprint/a-{HASH}"));
    write(&kept_apart.join("lib-a"), "fresh");
    let dir = BuildDir::new(
        target.clone(),
        vec![
            member(&tree, "a", &["a/src/lib.rs"]),
            member(&tree, "b", &["b/src/lib.rs"]),
        ],
    );

    lay_out(&target);
    dir.settle().expect("a directory with no record settles");
    assert_eq!(
        present(&target),
        Vec::<String>::new(),
        "a directory that never recorded what it built vouches for nothing it holds"
    );

    lay_out(&target);
    dir.settle().expect("an unchanged tree settles");
    assert_eq!(
        present(&target).len(),
        4,
        "nothing moved, so every unit stays as fresh as cargo says it is"
    );

    write(&tree.join("a/src/lib.rs"), "pub fn a() { }\n");
    dir.settle().expect("a moved member settles");
    assert_eq!(
        present(&target),
        vec![
            format!("debug/.fingerprint/b-{HASH}"),
            format!("x86_64-unknown-linux-gnu/release/.fingerprint/b-{HASH}"),
        ],
        "every unit of the member whose bytes moved goes, under every profile and triple, and \
         the other member's stay"
    );
    assert!(
        std::fs::symlink_metadata(&kept_apart).is_ok(),
        "a directory inside that keeps its own record is another target directory, settled by \
         its own builds"
    );
}

#[test]
fn a_file_that_appears_where_one_was_absent_moves_its_member() {
    let temp = tempfile::tempdir().expect("a directory");
    let tree = temp.path().join("tree");
    let target = temp.path().join("target");
    let dir = BuildDir::new(target.clone(), vec![member(&tree, "a", &["a/src/lib.rs"])]);
    dir.settle()
        .expect("an absent file is a digest like any other");
    lay_out(&target);
    write(&tree.join("a/src/lib.rs"), "pub fn a() {}\n");
    dir.settle().expect("the file is read now");
    assert_eq!(
        present(&target),
        vec![
            format!("debug/.fingerprint/b-{HASH}"),
            format!("x86_64-unknown-linux-gnu/release/.fingerprint/b-{HASH}"),
        ],
    );
}

#[test]
fn a_record_this_release_did_not_write_is_refused_rather_than_trusted() {
    let temp = tempfile::tempdir().expect("a directory");
    let tree = temp.path().join("tree");
    let target = temp.path().join("target");
    write(&tree.join("a/src/lib.rs"), "pub fn a() {}\n");
    for record in [
        "not json",
        r#"{"schema":"rust-mutants-built-v0","members":{}}"#,
        r#"{"schema":"rust-mutants-built-v1","members":{},"more":1}"#,
    ] {
        write(&target.join(LEDGER_NAME), record);
        let dir = BuildDir::new(target.clone(), vec![member(&tree, "a", &["a/src/lib.rs"])]);
        let Err(error) = dir.settle() else {
            panic!("{record} was trusted as a record of what the directory holds");
        };
        assert_eq!(error.code().code, "RM1022", "{record}: {error}");
    }
}
