// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The resolved dependency graph: what goes into one package's test binary, and what does not.

use rust_mutants::cargo::Metadata;

#[test]
fn the_closure_of_a_package_is_what_goes_into_its_test_binary() {
    let document = r#"{
      "version": 1,
      "workspace_root": "/w",
      "target_directory": "/w/target",
      "workspace_members": ["app 0.1.0 (path+file:///w)"],
      "packages": [],
      "resolve": {
        "root": null,
        "nodes": [
          {
            "id": "app 0.1.0 (path+file:///w)",
            "deps": [
              { "pkg": "lib 1.0.0 (registry+x)", "dep_kinds": [{ "kind": null }] },
              { "pkg": "gen 1.0.0 (registry+x)", "dep_kinds": [{ "kind": "build" }] },
              { "pkg": "helper 1.0.0 (registry+x)", "dep_kinds": [{ "kind": "dev" }] }
            ]
          },
          {
            "id": "lib 1.0.0 (registry+x)",
            "deps": [
              { "pkg": "deep 1.0.0 (registry+x)", "dep_kinds": [{ "kind": null }] },
              { "pkg": "theirs 1.0.0 (registry+x)", "dep_kinds": [{ "kind": "dev" }] }
            ]
          },
          { "id": "gen 1.0.0 (registry+x)", "deps": [] },
          { "id": "helper 1.0.0 (registry+x)", "deps": [] },
          { "id": "deep 1.0.0 (registry+x)", "deps": [] },
          { "id": "theirs 1.0.0 (registry+x)", "deps": [] }
        ]
      }
    }"#;
    let metadata = Metadata::parse(document.as_bytes()).expect("the document parses");
    let closure = metadata.closure("app 0.1.0 (path+file:///w)");
    assert!(closure.contains(&"app 0.1.0 (path+file:///w)".to_owned()));
    assert!(
        closure.contains(&"lib 1.0.0 (registry+x)".to_owned()),
        "a normal dependency"
    );
    assert!(
        closure.contains(&"deep 1.0.0 (registry+x)".to_owned()),
        "transitively"
    );
    assert!(
        closure.contains(&"gen 1.0.0 (registry+x)".to_owned()),
        "a build dependency"
    );
    assert!(
        closure.contains(&"helper 1.0.0 (registry+x)".to_owned()),
        "the package's own tests link its development dependencies"
    );
    assert!(
        !closure.contains(&"theirs 1.0.0 (registry+x)".to_owned()),
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
        { "id": "a 0.1.0 (path+file:///w)", "name": "a", "version": "0.1.0",
          "manifest_path": "/w/Cargo.toml" }
      ]
    }"#;
    let metadata = Metadata::parse(document.as_bytes()).expect("the document parses");
    assert!(metadata.resolve.is_none());
    assert_eq!(
        metadata.closure("anything"),
        ["a 0.1.0 (path+file:///w)"],
        "with no graph to narrow with, everything counts"
    );
}
