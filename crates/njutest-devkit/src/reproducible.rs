// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether this machine builds one tree to the same bytes twice.
//!
//! It is the premise every answer of the engine's equivalence layer rests on:
//! the layer builds a tree, builds it again with one mutation spliced in, and
//! reads the difference between the two as the mutation's doing. A machine
//! whose linker stamps what it writes renders one unchanged tree two ways, and
//! there the layer establishes nothing — which is a different answer from
//! "the compiler renders this mutation", and the suites have to be able to
//! tell which machine they are on before they can say which answer is right.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest as _, Sha256};

/// What one build produced, by target name, each executable digested.
type Built = BTreeMap<String, String>;

/// Whether a tree built, changed, and built back comes out as the bytes it came out as.
///
/// The build is the one the layer makes: the project's own test profile,
/// without the incremental state that would make what the compiler emits
/// depend on what it emitted before.
///
/// # Panics
/// When the fixture cannot be copied, which is a setup failure rather than an
/// answer.
#[must_use]
pub fn builds_the_same_twice() -> bool {
    let fixture = crate::fixture::Fixture::copy("fixture-equivalent");
    let target = fixture.temp().join("twice");
    let source = fixture.root().join("src/lib.rs");
    let Ok(original) = std::fs::read(&source) else {
        return false;
    };

    let first = built(fixture.root(), &target);
    let mut changed = original.clone();
    changed.extend_from_slice(b"\npub const A_THING_NOTHING_READS: u8 = 7;\n");
    if std::fs::write(&source, &changed).is_err() {
        return false;
    }
    let _between = built(fixture.root(), &target);
    if std::fs::write(&source, &original).is_err() {
        return false;
    }
    let again = built(fixture.root(), &target);

    !first.is_empty() && first == again
}

/// One build of the tree at `root`, or nothing at all when it did not build.
fn built(root: &Path, target: &Path) -> Built {
    let mut command = crate::paths::command(&crate::paths::cargo_binary());
    let _configured = command
        .env("CARGO_INCREMENTAL", "0")
        .args(["test", "--no-run", "--message-format=json", "--offline"])
        .arg("--locked")
        .arg("--target-dir")
        .arg(target)
        .current_dir(root);
    let Ok(said) = command.output() else {
        return Built::new();
    };
    if !said.status.success() {
        return Built::new();
    }
    let mut found = Built::new();
    for line in String::from_utf8_lossy(&said.stdout).lines() {
        let Ok(message) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let (Some(name), Some(executable)) = (
            message
                .get("target")
                .and_then(|target| target.get("name"))
                .and_then(serde_json::Value::as_str),
            message
                .get("executable")
                .and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(executable) else {
            return Built::new();
        };
        let _replaced = found.insert(name.to_owned(), hex::encode(Sha256::digest(&bytes)));
    }
    found
}
