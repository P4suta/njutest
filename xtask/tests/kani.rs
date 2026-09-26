// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The verifier task proves every production harness, neither a hand-picked subset nor stale names.

#![expect(
    clippy::expect_used,
    reason = "a repository-contract test reports an unreadable or malformed input by panicking"
)]

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..")
}

fn task(text: &str, name: &str) -> String {
    let heading = format!("[tasks.\"{name}\"]");
    let start = text.find(&heading).expect("the verifier task exists");
    let rest = text
        .get(start..)
        .expect("the task begins on a UTF-8 boundary");
    let end = rest
        .get(1..)
        .and_then(|after| after.find("\n[tasks."))
        .map_or(rest.len(), |at| at.saturating_add(1));
    rest.get(..end)
        .expect("the task ends on a UTF-8 boundary")
        .to_owned()
}

fn module_path(relative: &Path) -> Vec<String> {
    let mut parts = Vec::new();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            continue;
        };
        let text = component.to_str().expect("Rust source paths are UTF-8");
        if text == "lib.rs" || text == "mod.rs" {
            continue;
        }
        parts.push(text.strip_suffix(".rs").unwrap_or(text).to_owned());
    }
    parts
}

fn is_proof(attribute: &syn::Attribute) -> bool {
    let mut segments = attribute.path().segments.iter();
    matches!(segments.next(), Some(segment) if segment.ident == "kani")
        && matches!(segments.next(), Some(segment) if segment.ident == "proof")
        && segments.next().is_none()
}

fn collect_items(items: &[syn::Item], within: &mut Vec<String>, found: &mut BTreeSet<String>) {
    for item in items {
        match item {
            syn::Item::Fn(function) if function.attrs.iter().any(is_proof) => {
                let mut path = within.clone();
                path.push(function.sig.ident.to_string());
                assert!(
                    found.insert(path.join("::")),
                    "two production proof harnesses have the same module path"
                );
            }
            syn::Item::Mod(module) => {
                if let Some((_brace, nested)) = &module.content {
                    within.push(module.ident.to_string());
                    collect_items(nested, within, found);
                    let removed = within.pop();
                    assert!(removed.is_some(), "the module stack stays balanced");
                }
            }
            _ => {}
        }
    }
}

fn production_harnesses() -> BTreeSet<String> {
    let source_root = root().join("crates/rust-mutants/src");
    let mut found = BTreeSet::new();
    for entry in walkdir::WalkDir::new(&source_root) {
        let entry = entry.expect("the production source tree is readable");
        let path = entry.path();
        if !entry.file_type().is_file()
            || path.extension().is_none_or(|extension| extension != "rs")
        {
            continue;
        }
        let source = std::fs::read_to_string(path).expect("production Rust source is readable");
        let syntax = syn::parse_file(&source).expect("production Rust source parses");
        let relative = path
            .strip_prefix(&source_root)
            .expect("the walk yields paths under its own root");
        let mut within = module_path(relative);
        collect_items(&syntax.items, &mut within, &mut found);
    }
    found
}

#[test]
fn every_production_kani_harness_is_proved_by_the_exact_local_and_ci_task() {
    let mise = std::fs::read_to_string(root().join("mise.toml")).expect("mise.toml is readable");
    let laws = task(&mise, "kani:laws");
    assert!(
        laws.contains("cargo xtask kani-laws --cache"),
        "the local and CI gate proves through the one task that knows every harness"
    );
    let arguments: Vec<String> = xtask::kanilaws::arguments(Path::new("export.json"))
        .into_iter()
        .map(|argument| argument.into_string().expect("an argument is text"))
        .collect();
    let spoken = arguments.join(" ");
    assert!(
        spoken.starts_with("kani -p rust-mutants --lib --exact --no-assertion-reach-checks"),
        "the verifier must reject an ambiguous harness name: {spoken}"
    );
    assert!(
        spoken.ends_with("-Z unstable-options --export-json export.json"),
        "Kani success is not sufficient: its raw export is what the audit reads: {spoken}"
    );
    let asked: BTreeSet<String> = xtask::kanilaws::harnesses()
        .into_iter()
        .map(str::to_owned)
        .collect();
    assert_eq!(
        asked.len(),
        xtask::kanilaws::harnesses().len(),
        "a duplicated harness is not another proof"
    );
    assert_eq!(
        asked,
        production_harnesses(),
        "the harnesses the task proves, which are the ones the audit requires, and the production \
         #[kani::proof] inventory must be exactly the same set"
    );

    let verified = task(&mise, "kani:verified");
    assert!(
        verified.contains("depends = [\"kani:laws\"]"),
        "the retained-result protocol may not pass without proving the production laws first"
    );
    let workflow =
        std::fs::read_to_string(root().join(".github/workflows/ci.yml")).expect("CI is readable");
    assert!(
        workflow.contains("run: mise run kani:laws"),
        "CI must invoke the same exact harness inventory as the local gate"
    );
}
