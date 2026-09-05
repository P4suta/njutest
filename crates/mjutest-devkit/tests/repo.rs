// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The workspace builder, and the report normalizer.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reads a JSON document by the names its own fixture put there"
)]

use mjutest_devkit::repo::Repo;
use mjutest_devkit::report::{PLACEHOLDER, normalize};

#[test]
fn a_repo_writes_a_project_that_builds_offline_against_no_registry() {
    let repo = Repo::new();
    repo.package("example")
        .lib("pub fn double(n: i32) -> i32 {\n    n * 2\n}\n");

    let manifest = std::fs::read_to_string(repo.root().join("Cargo.toml")).expect("the manifest");
    assert!(manifest.contains("name = \"example\""), "{manifest}");
    assert!(
        manifest.contains("[workspace]"),
        "its own workspace table, so cargo does not look upwards: {manifest}"
    );
    assert!(repo.root().join("Cargo.lock").is_file(), "a committed lock");

    let lib = std::fs::read_to_string(repo.root().join("src/lib.rs")).expect("the library");
    assert!(lib.starts_with("// SPDX-FileCopyrightText:"), "{lib}");
    assert!(lib.contains("pub fn double"), "{lib}");
}

#[test]
fn a_repo_writes_an_integration_target_where_cargo_looks_for_one() {
    let repo = Repo::new();
    let package = repo.package("example");
    package.lib("pub const N: i32 = 1;\n");
    package.integration_test("wide", "#[test]\nfn it_works() {}\n");
    assert!(repo.root().join("tests/wide.rs").is_file());
}

#[test]
fn a_repo_that_was_committed_is_a_repository_with_one_commit() {
    let repo = Repo::new();
    repo.package("example").lib("pub const N: i32 = 1;\n");
    repo.commit();
    assert!(repo.root().join(".git").is_dir());
    assert!(
        repo.root().join(".git/refs/heads/main").is_file(),
        "on a branch a test can name"
    );
}

#[test]
fn the_tree_goes_away_with_the_value_so_a_test_bounds_its_own_mess() {
    let path = {
        let repo = Repo::new();
        repo.package("example").lib("pub const N: i32 = 1;\n");
        repo.root().to_path_buf()
    };
    assert!(!path.exists(), "{}", path.display());
}

// --- normalizing a report ------------------------------------------------------------

#[test]
fn what_changes_between_two_runs_of_the_same_work_is_replaced_in_place() {
    let document = serde_json::json!({
        "run_id": "20260905T081500Z-000001",
        "verdict": "INSUFFICIENT",
        "timing": { "started": "2026-09-05T08:15:00Z", "duration_ms": 1234 },
        "targets": [
            { "name": "a", "duration_ms": 40, "status": "passed" }
        ],
        "repository": { "git": { "commit": "abc", "merge_base": null, "dirty": true } },
    });
    let normalized = normalize(&document);

    assert_eq!(normalized["run_id"], PLACEHOLDER);
    assert_eq!(normalized["timing"]["started"], PLACEHOLDER);
    assert_eq!(normalized["repository"]["git"]["commit"], PLACEHOLDER);
    assert_eq!(normalized["timing"]["duration_ms"], 0);
    assert_eq!(normalized["targets"][0]["duration_ms"], 0);

    assert_eq!(normalized["verdict"], "INSUFFICIENT", "the claim survives");
    assert_eq!(normalized["targets"][0]["status"], "passed");
    assert_eq!(normalized["repository"]["git"]["dirty"], true);
    assert!(
        normalized["repository"]["git"]["merge_base"].is_null(),
        "\"no merge base\" is a claim, not a moment"
    );
}

#[test]
fn a_normalized_field_is_still_there_so_a_field_that_stopped_being_written_shows_up() {
    let normalized = normalize(&serde_json::json!({ "run_id": "x" }));
    assert!(
        normalized.get("run_id").is_some(),
        "removing it would hide its absence"
    );
}

#[test]
fn two_runs_of_the_same_work_normalize_to_the_same_document() {
    let one = serde_json::json!({
        "run_id": "20260905T081500Z-000001",
        "timing": { "duration_ms": 1234 },
        "verdict": "ASSURED",
    });
    let two = serde_json::json!({
        "run_id": "20260906T091600Z-000002",
        "timing": { "duration_ms": 99 },
        "verdict": "ASSURED",
    });
    assert_eq!(normalize(&one), normalize(&two));
}

#[test]
fn records_that_carry_an_identity_are_put_in_that_order() {
    let document = serde_json::json!({
        "targets": [
            { "id": "b", "duration_ms": 40 },
            { "id": "a", "duration_ms": 10 },
        ],
        "limitations": [{ "name": "second" }, { "name": "first" }],
    });
    let normalized = normalize(&document);
    assert_eq!(normalized["targets"][0]["id"], "a");
    assert_eq!(normalized["targets"][1]["id"], "b");
    assert_eq!(
        normalized["limitations"][0]["name"], "second",
        "a list with no identity keeps the order the run put it in"
    );
}
