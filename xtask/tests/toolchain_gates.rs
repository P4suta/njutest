// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The set of gates `cargo xtask all` runs, run over this repository, which asks cargo for its metadata along the way.

use xtask::gates;

#[test]
fn every_gate_that_needs_no_argument_is_one_all_runs() {
    let root = gates::workspace_root();
    let source = std::fs::read_to_string(root.join("xtask/src/lib.rs")).expect("xtask/src/lib.rs");
    let declaration = source
        .find("enum Gate {")
        .and_then(|at| source.get(at..))
        .expect("xtask/src/lib.rs declares the gates");
    let declaration = match declaration.find("\n}\n") {
        Some(end) => declaration.get(..end).expect("the declaration ends"),
        None => declaration,
    };

    let bare: Vec<String> = declaration
        .lines()
        .filter_map(|line| line.trim().strip_suffix(','))
        .filter(|name| {
            name.chars().next().is_some_and(char::is_uppercase)
                && name.chars().all(char::is_alphanumeric)
        })
        .filter(|name| *name != "All")
        .map(kebab)
        .collect();
    assert!(
        bare.len() >= 5,
        "the gates that need no argument are the ones a person runs as a set: {bare:?}"
    );

    let report = gates::all(&root).expect("every gate passes on this tree");
    let unrun: Vec<&String> = bare
        .iter()
        .filter(|name| !report.contains(&format!("{name}:")))
        .collect();
    assert!(
        unrun.is_empty(),
        "a gate `all` does not run is a gate `mise run check` does not run and continuous \
         integration does not run: it holds nothing, and the only sign is that it is still in \
         the help. {unrun:?} is declared and `all` never calls it:\n{report}"
    );
}

/// The name a gate answers to on the command line, from the name of its variant.
fn kebab(variant: &str) -> String {
    let mut said = String::new();
    for (at, character) in variant.char_indices() {
        if character.is_uppercase() && at > 0 {
            said.push('-');
        }
        said.extend(character.to_lowercase());
    }
    said
}
