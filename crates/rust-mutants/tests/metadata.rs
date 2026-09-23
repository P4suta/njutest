// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The resolved dependency graph: what goes into one package's test binary, and what does not.

use njutest_devkit::result::{ResultState::Returned, result_state};
use rust_mutants::cargo::Metadata;

#[test]
fn the_closure_of_a_package_is_what_goes_into_its_test_binary() {
    let document = r#"{
      "version": 1,
      "workspace_root": "/w",
      "target_directory": "/w/target",
      "workspace_members": ["path+file:///w#app@0.1.0"],
      "packages": [],
      "resolve": {
        "root": null,
        "nodes": [
          {
            "id": "path+file:///w#app@0.1.0",
            "deps": [
              { "pkg": "registry+x#lib@1.0.0", "dep_kinds": [{ "kind": null }] },
              { "pkg": "registry+x#gen@1.0.0", "dep_kinds": [{ "kind": "build" }] },
              { "pkg": "registry+x#helper@1.0.0", "dep_kinds": [{ "kind": "dev" }] }
            ]
          },
          {
            "id": "registry+x#lib@1.0.0",
            "deps": [
              { "pkg": "registry+x#deep@1.0.0", "dep_kinds": [{ "kind": null }] },
              { "pkg": "registry+x#theirs@1.0.0", "dep_kinds": [{ "kind": "dev" }] }
            ]
          },
          { "id": "registry+x#gen@1.0.0", "deps": [] },
          { "id": "registry+x#helper@1.0.0", "deps": [] },
          { "id": "registry+x#deep@1.0.0", "deps": [] },
          { "id": "registry+x#theirs@1.0.0", "deps": [] }
        ]
      }
    }"#;
    let metadata = Metadata::parse(document.as_bytes());
    assert_eq!(result_state(&metadata), Returned, "metadata: {metadata:?}");
    let Ok(metadata) = metadata else { return };
    let closure = metadata.closure("path+file:///w#app@0.1.0");
    assert!(closure.contains(&"path+file:///w#app@0.1.0".to_owned()));
    assert!(
        closure.contains(&"registry+x#lib@1.0.0".to_owned()),
        "a normal dependency"
    );
    assert!(
        closure.contains(&"registry+x#deep@1.0.0".to_owned()),
        "transitively"
    );
    assert!(
        closure.contains(&"registry+x#gen@1.0.0".to_owned()),
        "a build dependency"
    );
    assert!(
        closure.contains(&"registry+x#helper@1.0.0".to_owned()),
        "the package's own tests link its development dependencies"
    );
    assert!(
        !closure.contains(&"registry+x#theirs@1.0.0".to_owned()),
        "somebody else's development dependency is not linked into this binary"
    );

    let mut sorted = closure.clone();
    sorted.sort();
    assert_eq!(
        closure, sorted,
        "two runs of the same graph read the same list"
    );
}

#[test]
fn a_document_with_no_resolved_graph_keys_on_every_package_it_names() {
    let document = r#"{
      "version": 1,
      "workspace_root": "/w",
      "target_directory": "/w/target",
      "workspace_members": [],
      "packages": [
        { "id": "path+file:///w#a@0.1.0", "name": "a", "version": "0.1.0",
          "manifest_path": "/w/Cargo.toml" }
      ]
    }"#;
    let metadata = Metadata::parse(document.as_bytes());
    assert_eq!(result_state(&metadata), Returned, "metadata: {metadata:?}");
    let Ok(metadata) = metadata else { return };
    assert!(metadata.resolve.is_none());
    assert_eq!(
        metadata.closure("anything"),
        ["path+file:///w#a@0.1.0"],
        "with no graph to narrow with, everything counts"
    );
}
