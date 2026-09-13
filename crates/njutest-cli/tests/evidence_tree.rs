// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Reading the tree's own contribution to a run's identity: which files count, which do not, and what a corpus and a lock file add.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;

use njutest_cli::evidence::tree::{EXCLUDED_DIRECTORIES, Scan, dependencies, scan};
use njutest_devkit::repo::Repo;

fn read(root: &Path) -> Scan {
    scan(root, &[], &[]).expect("the tree reads")
}

fn repo() -> Repo {
    let repo = Repo::new();
    repo.package("demo")
        .lib("pub fn f(a: i32) -> i32 { a + 1 }\n");
    repo
}

#[test]
fn the_same_tree_reads_the_same_twice_and_a_changed_file_reads_differently() {
    let repo = repo();
    let first = read(repo.root());
    assert_eq!(first.tree, read(repo.root()).tree, "a walk is a function");
    assert!(first.files > 0);
    assert!(first.bytes > 0);

    repo.write("src/lib.rs", "pub fn f(a: i32) -> i32 { a - 1 }\n");
    assert_ne!(read(repo.root()).tree, first.tree, "a byte is a byte");
}

#[test]
fn a_file_that_moves_reads_differently_even_with_the_same_bytes() {
    let repo = repo();
    let before = read(repo.root()).tree;
    let text = std::fs::read_to_string(repo.root().join("src/lib.rs")).expect("read");
    std::fs::remove_file(repo.root().join("src/lib.rs")).expect("remove");
    repo.write("src/other.rs", &text);
    assert_ne!(
        read(repo.root()).tree,
        before,
        "where a file is decides what compiles it"
    );
}

#[test]
fn what_a_run_writes_is_not_what_a_run_reads() {
    let repo = repo();
    let before = read(repo.root()).tree;
    for directory in EXCLUDED_DIRECTORIES {
        repo.write(&format!("{directory}/noise.txt"), "whatever\n");
    }
    repo.write("target/debug/thing", "binary\n");
    repo.write("reports/runs/a/report.json", "{}\n");
    assert_eq!(
        read(repo.root()).tree,
        before,
        "the tree a run verifies is not the tree a run writes"
    );
}

#[test]
fn the_target_directory_cargo_names_is_left_out_wherever_it_is() {
    let repo = repo();
    let before = read(repo.root()).tree;
    repo.write("elsewhere/debug/thing", "binary\n");
    assert_ne!(
        read(repo.root()).tree,
        before,
        "an unremarkable directory counts"
    );
    let with = scan(repo.root(), &[], &[Path::new("elsewhere")]).expect("the tree reads");
    let absolute = repo.root().join("elsewhere");
    let without = scan(repo.root(), &[], &[absolute.as_path()]).expect("the tree reads");
    assert_eq!(
        with.tree, without.tree,
        "cargo names its target directory absolutely and a configuration may name it relatively"
    );
    assert_eq!(with.tree, before, "and either way it is left out");
}

#[test]
fn a_corpus_is_counted_apart_from_the_tree_because_it_grows_without_the_code_changing() {
    let repo = repo();
    let before = read(repo.root());
    repo.write("fuzz/corpus/parse/seed-1", "input\n");
    let after = read(repo.root());
    assert_eq!(
        after.tree, before.tree,
        "a corpus entry is not a change to the code"
    );
    assert_ne!(after.corpus, before.corpus, "but it is a change to the run");
    assert_eq!(after.corpus.len(), 64);
}

#[test]
fn a_pattern_the_configuration_excludes_is_left_out_too() {
    let repo = repo();
    repo.write("src/generated.rs", "pub fn g() {}\n");
    let counted = read(repo.root()).tree;
    let excluded = scan(
        repo.root(),
        &[rust_mutants::glob::Pattern::compile("**/generated.rs").expect("a pattern")],
        &[],
    )
    .expect("the tree reads")
    .tree;
    assert_ne!(counted, excluded);
    std::fs::remove_file(repo.root().join("src/generated.rs")).expect("remove");
    assert_eq!(
        read(repo.root()).tree,
        excluded,
        "a file left out reads as a file that is not there"
    );
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_is_read_as_the_link_it_is_and_never_followed() {
    let repo = repo();
    let before = read(repo.root()).tree;
    let link = repo.root().join("src/linked.rs");
    std::os::unix::fs::symlink("lib.rs", &link).expect("a link");
    let after = read(repo.root()).tree;
    assert_ne!(after, before, "a link is a fact about the tree");

    std::fs::remove_file(&link).expect("remove");
    std::os::unix::fs::symlink("other.rs", &link).expect("a link");
    assert_ne!(
        read(repo.root()).tree,
        after,
        "what a link points at is part of what it is"
    );
}

#[test]
fn a_missing_root_says_so_rather_than_reading_as_an_empty_tree() {
    let error = scan(Path::new("/no/such/tree/anywhere"), &[], &[])
        .expect_err("a tree that is not there is not an empty tree");
    assert!(error.to_string().contains("NJ2"), "{error}");
}

#[test]
fn the_dependencies_are_the_lock_files_checksums_and_nothing_else() {
    let lock = "\
version = 4

[[package]]
name = \"demo\"
version = \"0.1.0\"

[[package]]
name = \"serde\"
version = \"1.0.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
checksum = \"aaaa\"
";
    let one = dependencies(lock).expect("a lock file reads");
    assert_eq!(one.len(), 64);
    assert_eq!(
        one,
        dependencies(lock).expect("again"),
        "a walk is a function"
    );

    let reordered = "\
version = 4

[[package]]
name = \"serde\"
version = \"1.0.0\"
source = \"registry+https://github.com/rust-lang/crates.io-index\"
checksum = \"aaaa\"

[[package]]
name = \"demo\"
version = \"0.1.0\"
";
    assert_eq!(
        dependencies(reordered).expect("a lock file reads"),
        one,
        "the order cargo wrote them in is not a fact about the run"
    );

    let bumped = lock.replace("1.0.0", "1.0.1");
    assert_ne!(dependencies(&bumped).expect("reads"), one);
    let repointed = lock.replace("aaaa", "bbbb");
    assert_ne!(
        dependencies(&repointed).expect("reads"),
        one,
        "a checksum is what says the bytes are the same bytes"
    );

    assert!(
        dependencies("this is not toml [[[").is_err(),
        "a lock file that cannot be read is not an empty one"
    );
    assert_eq!(
        dependencies("version = 4\n").expect("a lock file with no packages"),
        dependencies("version = 4\n").expect("again")
    );
}
