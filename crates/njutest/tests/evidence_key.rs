// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The behaviour key of one target: everything that could change what it does, and nothing else.

#![expect(
    clippy::expect_used,
    reason = "the helper that keys one package of a tree this test wrote is not itself a test, and a tree that cannot be read is a setup failure to report by panicking"
)]

use std::collections::BTreeMap;

use njutest::assure::identity::{Evidence, Keying};
use njutest::evidence::key::{
    Common, Linked, Reading, behaviour, continuation_identity, linked_by, reads_directories_under,
};
use njutest::evidence::tree::{Scan, scan};
use njutest_devkit::repo::Repo;
use rust_mutants::cargo::Metadata;

fn common() -> Common {
    Common {
        toolchain: "rustc 1.98.0".to_owned(),
        platform: "x86_64-unknown-linux-gnu".to_owned(),
        environment: vec![("RUSTFLAGS".to_owned(), "-Copt-level=1".to_owned())],
        contract: "standard-v1".to_owned(),
        test_args: vec!["--test-threads=1".to_owned()],
        build: rust_mutants::cargo::BuildConfig {
            features: vec!["a".to_owned()],
            ..rust_mutants::cargo::BuildConfig::default()
        }
        .selection(),
        timeout_ms: 600_000,
        steps: 50_000_000,
        versions: vec!["njutest 0.1.0".to_owned(), "rust-mutants 0.1.0".to_owned()],
        corpus: "c".repeat(64),
    }
}

#[test]
fn the_key_is_over_the_typed_build_selection_and_separate_machine_inputs() {
    let config = njutest::config::Configuration {
        name: "release".to_owned(),
        all_features: true,
        no_default_features: true,
        profile: Some("release".to_owned()),
        target: Some("wasm32-unknown-unknown".to_owned()),
        features: vec!["a".to_owned()],
    };
    let mut shared = common();
    shared.build = config.build().selection();
    assert_ne!(
        behaviour(&linked(), &shared),
        behaviour(&linked(), &common()),
        "the cache key binds the build selection as typed fields rather than trusting an \
         argument spelling assembled elsewhere"
    );
}

#[test]
fn every_build_selection_field_separates_cache_and_continuation_state() {
    let base = rust_mutants::cargo::BuildConfig::default();
    let base_input = base.selection();
    let base_behaviour = {
        let mut common = common();
        common.build = base_input.clone();
        behaviour(&linked(), &common)
    };
    let base_continuation = continuation_identity(&"a".repeat(64), &base_input);

    let changed: [(&str, rust_mutants::cargo::BuildConfig); 7] = [
        (
            "features",
            rust_mutants::cargo::BuildConfig {
                features: vec!["one".to_owned()],
                ..base.clone()
            },
        ),
        (
            "all_features",
            rust_mutants::cargo::BuildConfig {
                all_features: true,
                ..base.clone()
            },
        ),
        (
            "no_default_features",
            rust_mutants::cargo::BuildConfig {
                no_default_features: true,
                ..base.clone()
            },
        ),
        (
            "profile",
            rust_mutants::cargo::BuildConfig {
                profile: Some("release".to_owned()),
                ..base.clone()
            },
        ),
        (
            "target",
            rust_mutants::cargo::BuildConfig {
                target: Some("wasm32-unknown-unknown".to_owned()),
                ..base.clone()
            },
        ),
        (
            "jobs",
            rust_mutants::cargo::BuildConfig {
                jobs: Some(2),
                ..base.clone()
            },
        ),
        (
            "debug",
            rust_mutants::cargo::BuildConfig {
                debug: true,
                ..base
            },
        ),
    ];

    for (field, build) in changed {
        let input = build.selection();
        let mut common = common();
        common.build = input.clone();
        assert_ne!(
            behaviour(&linked(), &common),
            base_behaviour,
            "changing only BuildConfig::{field} must make an earlier build's mutation answer unusable"
        );
        assert_ne!(
            continuation_identity(&"a".repeat(64), &input),
            base_continuation,
            "changing only BuildConfig::{field} must make an earlier build's checkpoint unusable"
        );
    }
}

#[test]
fn equivalent_feature_sets_and_identical_configured_builds_share_one_key() {
    let first = rust_mutants::cargo::BuildConfig {
        features: vec!["b".to_owned(), "a".to_owned(), "b".to_owned()],
        ..rust_mutants::cargo::BuildConfig::default()
    };
    let second = rust_mutants::cargo::BuildConfig {
        features: vec!["a".to_owned(), "b".to_owned()],
        ..rust_mutants::cargo::BuildConfig::default()
    };
    let first = first.selection();
    let second = second.selection();
    assert_eq!(first, second, "Cargo feature selection is a set");

    let mut first_common = common();
    first_common.build = first.clone();
    let mut second_common = common();
    second_common.build = second.clone();
    assert_eq!(
        behaviour(&linked(), &first_common),
        behaviour(&linked(), &second_common)
    );
    assert_eq!(
        continuation_identity(&"a".repeat(64), &first),
        continuation_identity(&"a".repeat(64), &second)
    );

    let default = rust_mutants::cargo::BuildConfig::default().selection();
    let configured_default = njutest::config::Configuration::default()
        .build()
        .selection();
    assert_eq!(
        default, configured_default,
        "the default build and an additional build with the same selected fields must share cache input"
    );
}

#[test]
fn rebinding_one_configured_request_changes_only_its_build_local_keys() {
    let mut shared = common();
    shared.build = rust_mutants::cargo::BuildConfig::default().selection();
    let evidence = Evidence {
        identity: "a".repeat(64),
        tree: "b".repeat(64),
        keying: Some(Keying {
            scan: Scan {
                tree: "b".repeat(64),
                corpus: "c".repeat(64),
                files: 0,
                bytes: 0,
                entries: BTreeMap::new(),
            },
            dependencies: "d".repeat(64),
            common: shared,
        }),
    };
    let release_build = rust_mutants::cargo::BuildConfig {
        profile: Some("release".to_owned()),
        ..rust_mutants::cargo::BuildConfig::default()
    };
    let default = evidence.for_build(&rust_mutants::cargo::BuildConfig::default());
    let release = evidence.for_build(&release_build);

    assert_eq!(
        default.identity, release.identity,
        "configured builds belong to one run-wide report identity"
    );
    assert_eq!(default.tree, release.tree);
    assert_ne!(
        default.continuation_identity(),
        release.continuation_identity(),
        "but a release build must not resume the default build's killed checkpoint"
    );
    let default_key = behaviour(
        &linked(),
        &default.keying.as_ref().expect("known evidence").common,
    );
    let release_key = behaviour(
        &linked(),
        &release.keying.as_ref().expect("known evidence").common,
    );
    assert_ne!(
        default_key, release_key,
        "and it must not read back the default build's killed or survived evidence"
    );

    let same = evidence.for_build(&rust_mutants::cargo::BuildConfig::default());
    assert_eq!(
        default.continuation_identity(),
        same.continuation_identity(),
        "two named builds with one Cargo selection and the same machine inputs may share prior work"
    );
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

    let changes: [Shared; 14] = [
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
        ("the features", |c| {
            c.build = rust_mutants::cargo::BuildConfig {
                features: vec!["b".to_owned()],
                ..rust_mutants::cargo::BuildConfig::default()
            }
            .selection();
        }),
        ("all features", |c| {
            c.build = rust_mutants::cargo::BuildConfig {
                all_features: true,
                ..rust_mutants::cargo::BuildConfig::default()
            }
            .selection();
        }),
        ("no default features", |c| {
            c.build = rust_mutants::cargo::BuildConfig {
                no_default_features: true,
                ..rust_mutants::cargo::BuildConfig::default()
            }
            .selection();
        }),
        ("the profile", |c| {
            c.build = rust_mutants::cargo::BuildConfig {
                profile: Some("release".to_owned()),
                ..rust_mutants::cargo::BuildConfig::default()
            }
            .selection();
        }),
        ("the target", |c| {
            c.build = rust_mutants::cargo::BuildConfig {
                target: Some("wasm32-unknown-unknown".to_owned()),
                ..rust_mutants::cargo::BuildConfig::default()
            }
            .selection();
        }),
        ("the timeout", |c| c.timeout_ms = 1),
        ("the step bound", |c| c.steps = 1),
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
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
    assert!(!reads_directories_under(repo.root(), &scanned, ""));

    repo.write(
        "src/listing.rs",
        "pub fn all() -> usize { std::fs::read_dir(\".\").into_iter().count() }\n",
    );
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
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
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
    let document = format!(
        r#"{{
          "version": 1,
          "workspace_root": "{root}",
          "target_directory": "{root}/target",
          "workspace_members": ["{package_id}"],
          "packages": [
            {{ "id": "{package_id}", "name": "demo", "version": "0.1.0",
               "manifest_path": "{root}/Cargo.toml" }},
            {{ "id": "registry+x#far@1.0.0", "name": "far", "version": "1.0.0",
               "manifest_path": "/elsewhere/Cargo.toml" }}
          ],
          "resolve": {{
            "root": null,
            "nodes": [
              {{ "id": "{package_id}",
                 "deps": [{{ "pkg": "registry+x#far@1.0.0", "dep_kinds": [{{ "kind": null }}] }}] }},
              {{ "id": "registry+x#far@1.0.0", "deps": [] }}
            ]
          }}
        }}"#,
        root = njutest_devkit::paths::in_json(repo.root()),
        package_id = njutest_devkit::paths::text_in_json(
            &njutest_devkit::cargo_double::package_id(repo.root(), "demo", "0.1.0")
        )
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
        &njutest_devkit::cargo_double::package_id(repo.root(), "demo", "0.1.0"),
    )
    .expect("the package links were read");
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

    let reading = Linked {
        reads_directories: true,
        ..linked()
    };
    let recorded = format!(
        "{}\n{}\n",
        behaviour(&linked(), &common()),
        behaviour(&reading, &common())
    );
    njutest_devkit::golden::golden(&golden, recorded.as_bytes()).expect("the recorded key");
}

/// What one walk of a tree this test wrote leaves out, for a project that has said nothing about where it writes.
fn within() -> njutest::evidence::tree::Bounds<'static> {
    static EXCLUDED: std::sync::LazyLock<njutest::evidence::tree::Excluded> =
        std::sync::LazyLock::new(|| {
            njutest::evidence::tree::Excluded::beside(
                njutest::config::Config::default()
                    .reports
                    .directory
                    .as_path(),
            )
            .expect("the default reports directory can be excluded")
        });
    njutest::evidence::tree::Bounds {
        exclude: &[],
        elsewhere: &[],
        excluded: &EXCLUDED,
    }
}

/// The digest one package is keyed on, in a tree this test writes.
fn keyed(repo: &Repo) -> String {
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
    let document = format!(
        r#"{{
          "version": 1,
          "workspace_root": "{root}",
          "target_directory": "{root}/target",
          "workspace_members": ["{package_id}"],
          "packages": [
            {{ "id": "{package_id}", "name": "demo", "version": "0.1.0",
               "manifest_path": "{root}/Cargo.toml" }}
          ],
          "resolve": {{
            "root": null,
            "nodes": [{{ "id": "{package_id}", "deps": [] }}]
          }}
        }}"#,
        root = njutest_devkit::paths::in_json(repo.root()),
        package_id = njutest_devkit::paths::text_in_json(
            &njutest_devkit::cargo_double::package_id(repo.root(), "demo", "0.1.0")
        )
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
        &njutest_devkit::cargo_double::package_id(repo.root(), "demo", "0.1.0"),
    )
    .expect("the package links were read");
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

/// A workspace whose closure holds a package of every kind `linked_by` has to tell apart.
fn four_kinds(repo: &Repo) -> Metadata {
    repo.package("demo").lib("pub fn f() {}\n");
    repo.write(
        "crates/deep/Cargo.toml",
        "[package]\nname = \"deep\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    repo.write(
        "crates/deep/src/lib.rs",
        "pub fn all() -> usize { std::fs::read_dir(\".\").into_iter().count() }\n",
    );
    repo.write(
        "crates/quiet/Cargo.toml",
        "[package]\nname = \"quiet\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    );
    repo.write("crates/quiet/src/lib.rs", "pub fn g() {}\n");

    let root = njutest_devkit::paths::in_json(repo.root());
    let package_id = njutest_devkit::paths::text_in_json(
        &njutest_devkit::cargo_double::package_id(repo.root(), "demo", "0.1.0"),
    );
    let deep = njutest_devkit::paths::text_in_json(&njutest_devkit::cargo_double::package_id(
        &repo.root().join("crates/deep"),
        "deep",
        "0.1.0",
    ));
    let quiet = njutest_devkit::paths::text_in_json(&njutest_devkit::cargo_double::package_id(
        &repo.root().join("crates/quiet"),
        "quiet",
        "0.1.0",
    ));
    let document = format!(
        r#"{{
          "version": 1,
          "workspace_root": "{root}",
          "target_directory": "{root}/target",
          "workspace_members": ["{package_id}"],
          "packages": [
            {{ "id": "{package_id}", "name": "demo", "version": "0.1.0",
               "manifest_path": "{root}/Cargo.toml" }},
            {{ "id": "registry+x#far@1.0.0", "name": "far", "version": "1.0.0",
               "manifest_path": "/elsewhere/Cargo.toml" }},
            {{ "id": "{deep}", "name": "deep",
               "version": "0.1.0", "manifest_path": "{root}/crates/deep/Cargo.toml" }},
            {{ "id": "{quiet}", "name": "quiet",
               "version": "0.1.0", "manifest_path": "{root}/crates/quiet/Cargo.toml" }}
          ],
          "resolve": {{
            "root": null,
            "nodes": [
              {{ "id": "{package_id}", "deps": [
                 {{ "pkg": "registry+x#ghost@9.9.9", "dep_kinds": [{{ "kind": null }}] }},
                 {{ "pkg": "registry+x#far@1.0.0", "dep_kinds": [{{ "kind": null }}] }},
                 {{ "pkg": "{deep}", "dep_kinds": [{{ "kind": null }}] }},
                 {{ "pkg": "{quiet}", "dep_kinds": [{{ "kind": null }}] }}
              ] }},
              {{ "id": "registry+x#ghost@9.9.9", "deps": [] }},
              {{ "id": "registry+x#far@1.0.0", "deps": [] }},
              {{ "id": "{deep}", "deps": [] }},
              {{ "id": "{quiet}", "deps": [] }}
            ]
          }}
        }}"#
    );
    Metadata::parse(document.as_bytes()).expect("the document parses")
}

#[test]
fn every_package_of_a_closure_is_reached_whatever_the_ones_before_it_were() {
    let repo = Repo::new();
    let metadata = four_kinds(&repo);
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
    let dependencies = "b".repeat(64);
    let linked = linked_by(
        &Reading {
            metadata: &metadata,
            scan: &scanned,
            root: repo.root(),
            dependencies: &dependencies,
        },
        &njutest_devkit::cargo_double::package_id(repo.root(), "demo", "0.1.0"),
    )
    .expect("the package links were read");

    assert!(
        linked
            .packages
            .contains(&"registry+x#ghost@9.9.9".to_owned()),
        "a package the resolved graph names and the package list does not is named by \
         the id it was asked about, because a key that leaves it out is a key that says \
         two closures are one: {:?}",
        linked.packages
    );
    assert!(
        linked.packages.contains(&"quiet@0.1.0".to_owned())
            && linked.packages.contains(&"deep@0.1.0".to_owned())
            && linked.packages.contains(&"far@1.0.0".to_owned()),
        "and every package after it is still read: a closure is walked to its end, and \
         one that stops at the first package it cannot name, or at the first outside the \
         tree, silently keys a target on part of what it links: {:?}",
        linked.packages
    );
    assert!(
        linked.sources.contains_key("deep@0.1.0") && linked.sources.contains_key("quiet@0.1.0"),
        "a package inside the tree is keyed on its own sources wherever in the tree it \
         is, so the directories between the root and it have to be spelled back out: \
         {:?}",
        linked.sources
    );
    assert_ne!(
        linked.sources.get("deep@0.1.0"),
        linked.sources.get("quiet@0.1.0"),
        "and two packages in two directories are keyed on two different things: a \
         prefix that came out empty would name the whole tree for both of them"
    );
    assert!(
        linked.reads_directories,
        "one package of a closure that reads a directory makes the whole key the tree's, \
         however many packages beside it do not: a target links what it links, and the \
         one dependency whose answer depends on what is on the disk decides for all of \
         them"
    );
}

#[test]
fn a_word_that_is_ordinary_english_does_not_key_a_package_on_the_whole_tree() {
    let repo = Repo::new();
    repo.package("demo").lib(
        "pub fn f() -> usize { 1 }\n\
         #[cfg(test)]\n\
         mod tests {\n\
             #[test]\n\
             #[ignore = \"slow\"]\n\
             fn slow() { assert_eq!(super::f(), 1); }\n\
             #[test]\n\
             fn global_counter_starts_at_one() { assert_eq!(super::f(), 1); }\n\
         }\n",
    );
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
    assert!(
        !reads_directories_under(repo.root(), &scanned, ""),
        "a suite that marks a test ignored, or names something global, has not said it \
         reads a directory. Keying it on the whole tree throws away every answer the \
         moment anybody edits a file beside it, which is every stored answer in a \
         repository where somebody writes documentation"
    );

    repo.write(
        "src/listing.rs",
        "use ignore::WalkBuilder;\npub fn all() -> usize { WalkBuilder::new(\".\").build().count() }\n",
    );
    let scanned = scan(repo.root(), &within()).expect("the tree reads");
    assert!(
        reads_directories_under(repo.root(), &scanned, ""),
        "while a package that reaches the crate does say so, and the path it is reached \
         through is what says it"
    );
}
