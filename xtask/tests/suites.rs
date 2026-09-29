// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every integration test file compiles: into its crate's one suite, as a toolchain binary of its own, or as the one binary whose subject is this repository.

#![expect(
    clippy::expect_used,
    reason = "a workspace this law cannot read is a failure it reports"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The one test file of a crate whose subject is the committed tree rather than code, a binary apart from the suite so a measurement that rewrites the tree can leave it out.
const THIS_REPOSITORY: &str = "this_repository.rs";

/// The members whose integration tests keep a layout of their own, each with why.
const APART: [(&str, &str); 1] = [(
    "crates/njutest-macros",
    "trybuild reads its compile-fail cases from tests/ui, and all_variants.rs is the one binary \
     that drives them",
)];

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
}

/// Every member keeping integration tests but the ones kept apart, as a directory relative to the root, read from cargo so the crate added next is held to the law the day it arrives.
fn suited(root: &Path) -> Vec<String> {
    let canonical = std::fs::canonicalize(root).expect("the workspace root resolves");
    let keeping: Vec<String> = njutest_devkit::census::members(root)
        .into_iter()
        .filter(|member| !member.suites().is_empty())
        .map(|member| {
            let directory = std::fs::canonicalize(&member.directory).expect("a member resolves");
            directory
                .strip_prefix(&canonical)
                .expect("a member lies inside the workspace")
                .to_str()
                .expect("a member directory is named in UTF-8")
                .replace('\\', "/")
        })
        .collect();
    for (apart, why) in APART {
        assert!(
            keeping.iter().any(|member| member == apart),
            "{apart} is kept apart because {why}, and it is no member keeping integration tests"
        );
    }
    let suited: Vec<String> = keeping
        .into_iter()
        .filter(|member| !APART.iter().any(|(apart, _why)| apart == member))
        .collect();
    assert!(
        suited.len() > 3,
        "the crates are read from cargo, and this found almost none: {suited:?}"
    );
    suited
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).expect("a file the law reads")
}

/// The top-level `.rs` files of `tests`, but the suite itself.
fn test_files(tests: &Path) -> BTreeSet<String> {
    std::fs::read_dir(tests)
        .expect("a tests directory")
        .map(|entry| entry.expect("an entry of the tests directory"))
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .map(|entry| {
            entry
                .file_name()
                .into_string()
                .expect("a test file named in UTF-8")
        })
        .filter(|name| {
            Path::new(name)
                .extension()
                .is_some_and(|extension| extension == "rs")
                && name != "suite.rs"
        })
        .collect()
}

/// Whether a manifest turns autotests off, and the path of every `[[test]]` it declares, read as the TOML it is.
fn manifested(manifest: &str) -> (bool, Vec<String>) {
    let table: toml::Table = toml::from_str(manifest).expect("a manifest is TOML");
    let off = table
        .get("package")
        .and_then(|package| package.get("autotests"))
        .and_then(toml::Value::as_bool)
        == Some(false);
    let paths = match table.get("test").and_then(toml::Value::as_array) {
        Some(tests) => tests
            .iter()
            .filter_map(|test| test.get("path").and_then(toml::Value::as_str))
            .map(str::to_owned)
            .collect(),
        None => Vec::new(),
    };
    (off, paths)
}

/// The file each module of a suite is compiled from, as its `#[path]` attribute names it.
fn moduled(suite: &str) -> Vec<String> {
    let file = njutest_devkit::lexed::file(suite).expect("a suite is Rust");
    file.items
        .iter()
        .filter_map(|item| match item {
            syn::Item::Mod(module) => module.attrs.iter().find_map(|attribute| {
                match (&attribute.meta, attribute.path().is_ident("path")) {
                    (syn::Meta::NameValue(pair), true) => match &pair.value {
                        syn::Expr::Lit(syn::ExprLit {
                            lit: syn::Lit::Str(text),
                            ..
                        }) => Some(text.value()),
                        _ => None,
                    },
                    _ => None,
                }
            }),
            _ => None,
        })
        .collect()
}

#[test]
fn a_module_a_suite_only_mentions_in_a_comment_is_compiled_from_nothing() {
    let suite = "#[path = \"kept.rs\"]\nmod kept;\n/*\n#[path = \"gone.rs\"]\nmod gone;\n*/\n";
    assert_eq!(
        moduled(suite),
        ["kept.rs".to_owned()],
        "a module inside a block comment is compiled by nothing, and a reader of lines counts it"
    );
}

#[test]
fn every_test_file_is_compiled_by_the_suite_or_as_a_toolchain_binary() {
    let root = root();
    let mut refused = Vec::new();
    for crate_dir in suited(&root) {
        let crate_dir = crate_dir.as_str();
        let manifest = read(&root.join(crate_dir).join("Cargo.toml"));
        let (autotests_off, paths) = manifested(&manifest);
        if !autotests_off {
            refused.push(format!(
                "{crate_dir}: autotests is not off, so every file is a binary"
            ));
        }
        let declared: Vec<String> = paths
            .into_iter()
            .filter_map(|path| path.strip_prefix("tests/").map(str::to_owned))
            .collect();
        if !declared.iter().any(|path| path == "suite.rs") {
            refused.push(format!("{crate_dir}: no [[test]] compiles tests/suite.rs"));
        }
        let suite = read(&root.join(crate_dir).join("tests/suite.rs"));
        let moduled: Vec<String> = moduled(&suite);
        let files = test_files(&root.join(crate_dir).join("tests"));
        for file in &files {
            let binary = declared.contains(file);
            let module = moduled.contains(file);
            let slow = file.starts_with("toolchain_");
            let apart = file == THIS_REPOSITORY;
            match (slow || apart, binary, module) {
                (true, true, false) | (false, false, true) => {}
                (true, _, _) => refused.push(format!(
                    "{crate_dir}/tests/{file} needs a toolchain or is about this repository, so it \
                     is a [[test]] of its own and not a module of the suite"
                )),
                (false, _, _) => refused.push(format!(
                    "{crate_dir}/tests/{file} is compiled by nothing: add `#[path = \"{file}\"] mod \
                     …;` to tests/suite.rs"
                )),
            }
        }
        for named in declared.iter().chain(&moduled) {
            if named != "suite.rs" && !files.contains(named) {
                refused.push(format!(
                    "{crate_dir}: tests/{named} is named and does not exist"
                ));
            }
        }
    }
    assert!(
        refused.is_empty(),
        "with autotests off a test file nothing names is never compiled and never fails, so every \
         one is named exactly once:\n  {}",
        refused.join("\n  ")
    );
}
