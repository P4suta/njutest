// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That every edit a catalog records is the source its mutant compiles to, so what a person is shown is what ran.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, and splices by the spans it was given"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, discover_file};

static REGISTRY: Registry = Registry::canonical();

/// Every Rust source under `root`, outside a build directory.
fn sources(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory).expect("a readable fixture tree") {
            let entry = entry.expect("a readable fixture entry");
            let path = entry.path();
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            if entry.file_type().expect("an entry's kind").is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

#[test]
fn every_catalogued_edit_applied_to_its_file_is_still_rust() {
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let mut broken = Vec::new();
    let mut checked = 0_usize;
    for path in sources(&njutest_devkit::paths::fixtures_dir()) {
        let bytes = std::fs::read(&path).expect("a fixture source");
        let name = path.display().to_string();
        let Ok(discovery) = discover_file(&name, &bytes, &selection) else {
            continue;
        };
        for found in &discovery.candidates {
            let candidate = &found.candidate;
            let start = usize::try_from(candidate.span.start).expect("a span");
            let end = usize::try_from(candidate.span.end).expect("a span");
            let word = |byte: Option<&u8>| {
                byte.is_some_and(|byte| byte.is_ascii_alphanumeric() || *byte == b'_')
            };
            let fuses = (word(bytes[..start].last()) && word(candidate.replacement.first()))
                || (word(candidate.replacement.last()) && word(bytes[end..].first()));
            if fuses {
                broken.push(format!(
                    "{name}:{} {}: the replacement fuses with the text beside it",
                    candidate.span.start, candidate.rule.name
                ));
            }
            let mut edited = bytes[..start].to_vec();
            edited.extend_from_slice(&candidate.replacement);
            edited.extend_from_slice(&bytes[end..]);
            checked = checked.saturating_add(1);
            let text = String::from_utf8(edited).expect("an edit keeps the file text");
            if syn::parse_file(&text).is_err() {
                broken.push(format!(
                    "{name}:{} {}",
                    candidate.span.start, candidate.rule.name
                ));
            }
        }
    }
    assert!(checked > 0, "the fixtures hold candidates");
    assert!(
        broken.is_empty(),
        "a catalog's edit is what explain, the diff and a person replaying it are shown; an edit \
         that is not the mutant's source shows them code that never ran. {broken:#?}"
    );
}
