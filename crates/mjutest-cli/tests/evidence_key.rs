// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The behaviour key of one target: everything that could change what it does, and nothing else.

#![expect(
    clippy::expect_used,
    reason = "the helper that keys one package of a tree this test wrote is not itself a test, and a tree that cannot be read is a setup failure to report by panicking"
)]

use std::collections::BTreeMap;

use mjutest_cli::evidence::key::{
    Common, Linked, Reading, behaviour, linked_by, reads_directories_under,
};
use mjutest_cli::evidence::tree::scan;
use mjutest_devkit::repo::Repo;
use rust_mutants::cargo::Metadata;

fn common() -> Common {
    Common {
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: vec![("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned())],
        contract: "standard-v1".to_owned(),
        test_args: vec!["--test-threads=1".to_owned()],
        features: vec!["a".to_owned()],
        timeout_ms: 600_000,
        versions: vec!["mjutest 0.1.0".to_owned(), "rust-mutants 0.1.0".to_owned()],
        corpus: "c".repeat(64),
    }
}

fn linked() -> Linked {
    Linked {
        packages: vec!["demo@0.1.0".to_owned(), "serde@1.0.0".to_owned()],
        sources: BTreeMap::from([("demo@0.1.0".to_owned(), "a".repeat(64))]),
        dependencies: "b".repeat(64),
        reads_directories: false,
        tree: "d".repeat(64),
    }
}

#[test]
fn every_key_is_sixty_four_hex_and_a_function_of_its_inputs() {
    let value = behaviour(&linked(), &common());
    assert_eq!(value.len(), 64);
    assert_eq!(value, behaviour(&linked(), &common()));
}

/// One thing that changes, what it links, and what the run shares.
type Case = (&'static str, Linked, Common);

/// One change to what every key of a run shares.
type Shared = (&'static str, fn(&mut Common));

#[test]
fn a_key_covers_everything_that_could_change_what_the_target_does() {
    let base = behaviour(&linked(), &common());
    let mut cases: Vec<Case> = Vec::new();

    let mut one = linked();
    one.packages.push("extra@1.0.0".to_owned());
    cases.push(("a package it links", one, common()));
    let mut one = linked();
    one.sources.insert("demo@0.1.0".to_owned(), "e".repeat(64));
    cases.push(("the sources of one of them", one, common()));
    let mut one = linked();
    one.dependencies = "e".repeat(64);
    cases.push(("what the lock file resolved", one, common()));

    let changes: [Shared; 9] = [
        ("the toolchain", |c| {
            c.toolchain = "rustc 1.99.0".to_owned();
        }),
        ("the platform", |c| {
            c.platform = "aarch64-apple-darwin".to_owned();
        }),
        ("the environment", |c| {
            c.environment.push(("CC".to_owned(), "clang".to_owned()));
        }),
        ("the contract", |c| c.contract = "deep-v1".to_owned()),
        ("the harness arguments", |c| c.test_args.clear()),
        ("the features", |c| c.features.push("b".to_owned())),
        ("the timeout", |c| c.timeout_ms = 1),
        ("the versions", |c| {
            c.versions.push("something 9".to_owned());
        }),
        ("the corpus", |c| c.corpus = "e".repeat(64)),
    ];
    for (what, apply) in changes {
        let mut shared = common();
        apply(&mut shared);
        cases.push((what, linked(), shared));
    }

    for (what, one, other) in cases {
        assert_ne!(
            behaviour(&one, &other),
            base,
            "{what} did not change the key"
        );
    }
}

#[test]
fn the_order_a_process_listed_its_environment_in_is_not_a_fact_about_the_target() {
    let mut reversed = common();
    reversed
        .environment
        .push(("CC".to_owned(), "clang".to_owned()));
    let mut forwards = reversed.clone();
    forwards.environment.reverse();
    assert_eq!(
        behaviour(&linked(), &reversed),
        behaviour(&linked(), &forwards)
    );
}

#[test]
fn a_package_that_reads_a_directory_keys_on_the_whole_tree() {
    let mut reading = linked();
    reading.reads_directories = true;
    let one = behaviour(&reading, &common());
    assert_ne!(one, behaviour(&linked(), &common()));

    let mut elsewhere = reading;
    elsewhere.tree = "e".repeat(64);
    assert_ne!(
        behaviour(&elsewhere, &common()),
        one,
        "a file anywhere in the tree can change what it reads"
    );

    let mut not_reading = linked();
    not_reading.tree = "e".repeat(64);
    assert_eq!(
        behaviour(&not_reading, &common()),
        behaviour(&linked(), &common()),
        "a package that reads only the files it names is not keyed on the rest of the tree"
    );
}

#[test]
fn what_reads_a_directory_is_found_in_the_source_rather_than_guessed_at() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    let scanned = scan(repo.root(), &[], &[]).expect("the tree reads");
    assert!(!reads_directories_under(repo.root(), &scanned, ""));

    repo.write(
        "src/listing.rs",
        "pub fn all() -> usize { std::fs::read_dir(\".\").into_iter().count() }\n",
    );
    let scanned = scan(repo.root(), &[], &[]).expect("the tree reads");
    assert!(
        reads_directories_under(repo.root(), &scanned, ""),
        "a package whose result depends on what is in a directory keys the whole tree"
    );
    assert!(
        !reads_directories_under(repo.root(), &scanned, "tests"),
        "and one that does not is not keyed on it"
    );
}

#[test]
fn what_a_target_links_is_read_from_the_resolved_graph() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    let scanned = scan(repo.root(), &[], &[]).expect("the tree reads");
    let document = format!(
        r#"{{
          "version": 1,
          "workspace_root": "{root}",
          "target_directory": "{root}/target",
          "workspace_members": ["demo 0.1.0 (path+file://{root})"],
          "packages": [
            {{ "id": "demo 0.1.0 (path+file://{root})", "name": "demo", "version": "0.1.0",
               "manifest_path": "{root}/Cargo.toml" }},
            {{ "id": "far 1.0.0 (registry+x)", "name": "far", "version": "1.0.0",
               "manifest_path": "/elsewhere/Cargo.toml" }}
          ],
          "resolve": {{
            "root": null,
            "nodes": [
              {{ "id": "demo 0.1.0 (path+file://{root})",
                 "deps": [{{ "pkg": "far 1.0.0 (registry+x)", "dep_kinds": [{{ "kind": null }}] }}] }},
              {{ "id": "far 1.0.0 (registry+x)", "deps": [] }}
            ]
          }}
        }}"#,
        root = repo.root().display()
    );
    let metadata = Metadata::parse(document.as_bytes()).expect("the document parses");
    let dependencies = "b".repeat(64);
    let linked = linked_by(
        &Reading {
            metadata: &metadata,
            scan: &scanned,
            root: repo.root(),
            dependencies: &dependencies,
        },
        &format!("demo 0.1.0 (path+file://{})", repo.root().display()),
    );
    assert_eq!(linked.packages, ["demo@0.1.0", "far@1.0.0"]);
    assert!(
        linked.sources.contains_key("demo@0.1.0"),
        "a package in the tree is keyed on its own sources: {linked:?}"
    );
    assert!(
        !linked.sources.contains_key("far@1.0.0"),
        "a package outside the tree is keyed on what the lock file says its bytes are"
    );
    assert_eq!(linked.dependencies, "b".repeat(64));
    assert!(!linked.reads_directories);
}

#[test]
fn a_behaviour_key_is_about_the_variables_and_not_about_how_the_list_was_built() {
    let once = behaviour(&linked(), &common());

    let twice = behaviour(
        &linked(),
        &Common {
            environment: vec![
                ("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned()),
                ("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned()),
            ],
            ..common()
        },
    );

    assert_eq!(
        once, twice,
        "what a key is about is the environment a target is run in, and the same \
         variable named twice is one variable: a key that changed with the shape of the \
         list would make a run re-establish what an identical run already answered, and \
         the report would say it was reused when it was not"
    );
}

#[test]
fn the_behaviour_key_of_a_known_target_is_the_one_it_has_always_been() {
    let golden = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/testdata/behaviour-key.golden");

    mjutest_devkit::golden::golden(&golden, behaviour(&linked(), &common()).as_bytes())
        .expect("the recorded key");
}

/// The digest one package is keyed on, in a tree this test writes.
fn keyed(repo: &Repo) -> String {
    let scanned = scan(repo.root(), &[], &[]).expect("the tree reads");
    let document = format!(
        r#"{{
          "version": 1,
          "workspace_root": "{root}",
          "target_directory": "{root}/target",
          "workspace_members": ["demo 0.1.0 (path+file://{root})"],
          "packages": [
            {{ "id": "demo 0.1.0 (path+file://{root})", "name": "demo", "version": "0.1.0",
               "manifest_path": "{root}/Cargo.toml" }}
          ],
          "resolve": {{
            "root": null,
            "nodes": [{{ "id": "demo 0.1.0 (path+file://{root})", "deps": [] }}]
          }}
        }}"#,
        root = repo.root().display()
    );
    let metadata = Metadata::parse(document.as_bytes()).expect("the document parses");
    let dependencies = "b".repeat(64);
    let linked = linked_by(
        &Reading {
            metadata: &metadata,
            scan: &scanned,
            root: repo.root(),
            dependencies: &dependencies,
        },
        &format!("demo 0.1.0 (path+file://{})", repo.root().display()),
    );
    linked
        .sources
        .get("demo@0.1.0")
        .expect("the package's own digest")
        .clone()
}

#[test]
fn a_package_is_keyed_on_what_it_compiles_and_not_on_what_sits_beside_it() {
    let repo = Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    let alone = keyed(&repo);

    repo.write("NOTES.md", "something a reader wrote\n");
    assert_eq!(
        keyed(&repo),
        alone,
        "a file the compiler never opens cannot change what the tests say, and keying \
         on it would throw away every stored answer whenever somebody edited a README"
    );

    repo.write("src/other.rs", "pub fn g() {}\n");
    let with_source = keyed(&repo);
    assert_ne!(
        with_source, alone,
        "and a Rust file is one the compiler does open"
    );

    repo.write(
        "Cargo.toml",
        "[package]\nname = \"demo\"\nversion = \"0.1.1\"\nedition = \"2024\"\n",
    );
    assert_ne!(
        keyed(&repo),
        with_source,
        "so is the manifest, which says what the compiler is given"
    );
}

#[test]
fn a_package_that_says_it_reads_what_sits_beside_it_is_keyed_on_all_of_it() {
    let reading = Repo::new();
    reading.package("demo").lib("pub fn f() {}\n");
    reading.write("data.txt", "one\n");
    let before = keyed(&reading);
    reading.write(
        "src/embedded.rs",
        "pub const DATA: &str = include_str!(\"../data.txt\");\n",
    );
    let naming = keyed(&reading);
    reading.write("data.txt", "two\n");
    assert_ne!(
        keyed(&reading),
        naming,
        "a package whose source names include_str! is one whose behaviour depends on a \
         file the compiler was never told about, so what it is keyed on is everything \
         beside it"
    );
    assert_ne!(naming, before, "and naming it is itself a change");

    let building = Repo::new();
    building.package("demo").lib("pub fn f() {}\n");
    building.write("build.rs", "fn main() {}\n");
    building.write("data.txt", "one\n");
    let with_script = keyed(&building);
    building.write("data.txt", "two\n");
    assert_ne!(
        keyed(&building),
        with_script,
        "and a build script can read anything and tell cargo to watch it, which this \
         release does not read, so the same answer holds: everything beside it"
    );
}
