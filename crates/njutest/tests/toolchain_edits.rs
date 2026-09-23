// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every edit `fixture-edits` carries breaks exactly the targets its README names, established by building and running every target and by nothing a selection says.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads cargo's documents by the names cargo puts there"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use njutest_devkit::fixture::Fixture;

/// The fixture whose edits are the ground truth.
const FIXTURE: &str = "fixture-edits";

/// The fence that opens the block of the fixture's README stating what each edit breaks.
const EDITS_FENCE: &str = "```edits";

/// What the README says each edit breaks, by the name of its directory under `edits/`.
fn stated(root: &Path) -> BTreeMap<String, BTreeSet<String>> {
    let readme = std::fs::read_to_string(root.join("README.md")).expect("the fixture's README");
    let (_, after) = readme
        .split_once(EDITS_FENCE)
        .expect("the README states what each edit breaks");
    let (block, _) = after.split_once("```").expect("the edits block is closed");
    block
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (edit, breaks) = line
                .split_once(" breaks ")
                .unwrap_or_else(|| panic!("`<edit> breaks <targets>`: {line}"));
            let broken = match breaks.trim() {
                "nothing" => BTreeSet::new(),
                named => named.split(", ").map(str::to_owned).collect(),
            };
            (edit.trim().to_owned(), broken)
        })
        .collect()
}

/// Every edit the fixture carries, by the name of its directory.
fn carried(root: &Path) -> BTreeSet<String> {
    std::fs::read_dir(root.join("edits"))
        .expect("the fixture's edits")
        .map(|entry| {
            entry
                .expect("an edit")
                .file_name()
                .into_string()
                .expect("an edit's name is text")
        })
        .collect()
}

/// `cargo` in `root`, building into the fixture's own target directory, offline and locked.
fn cargo(fixture: &Fixture, arguments: &[&str]) -> Command {
    let mut command = Command::new(njutest_devkit::paths::cargo_binary());
    command
        .args(arguments)
        .args(["--offline", "--locked"])
        .current_dir(fixture.root())
        .env("CARGO_TARGET_DIR", fixture.temp().join("target"));
    command
}

/// Every package of the fixture, by the directory its manifest is in.
fn packages(fixture: &Fixture) -> BTreeMap<PathBuf, String> {
    let listed = cargo(fixture, &["metadata", "--format-version", "1", "--no-deps"])
        .output()
        .expect("cargo metadata runs");
    assert!(listed.status.success(), "cargo metadata: {listed:?}");
    let text = String::from_utf8(listed.stdout).expect("cargo metadata prints text");
    let metadata: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&text).expect("cargo metadata prints JSON");
    metadata["packages"]
        .as_array()
        .expect("the packages")
        .iter()
        .map(|package| {
            let manifest = PathBuf::from(package["manifest_path"].as_str().expect("a manifest"));
            let directory = manifest.parent().expect("a manifest has a directory");
            (
                directory.to_path_buf(),
                package["name"].as_str().expect("a name").to_owned(),
            )
        })
        .collect()
}

/// Whether each target passes when the fixture is built with `features`, by `package/kind/name`.
///
/// The doctests of every package run in one invocation, and again one package at a time only when that one fails, since which of them failed is all it cannot say.
fn outcomes(
    fixture: &Fixture,
    packages: &BTreeMap<PathBuf, String>,
    features: &[String],
) -> BTreeMap<String, bool> {
    let mut arguments = vec!["test", "--no-run", "--workspace", "--message-format=json"];
    let joined = features.join(",");
    if !features.is_empty() {
        arguments.extend(["--features", joined.as_str()]);
    }
    let built = cargo(fixture, &arguments)
        .output()
        .expect("cargo builds the tests");
    assert!(
        built.status.success(),
        "every edit here compiles, so a failure to build is the fixture's: {}",
        njutest_devkit::process::strict_utf8(&built.stderr)
    );
    let mut passed = BTreeMap::new();
    let text = String::from_utf8(built.stdout).expect("cargo prints text");
    for line in text.lines() {
        let message: serde_json::Value =
            njutest_devkit::strictjson::decode_str(line).expect("cargo prints JSON lines");
        if message["reason"] != "compiler-artifact" || message["profile"]["test"] != true {
            continue;
        }
        let Some(executable) = message["executable"].as_str() else {
            continue;
        };
        let manifest = PathBuf::from(message["manifest_path"].as_str().expect("a manifest"));
        let directory = manifest.parent().expect("a manifest has a directory");
        let package = packages
            .get(directory)
            .unwrap_or_else(|| panic!("a package the metadata names: {}", directory.display()));
        let kind = message["target"]["kind"][0].as_str().expect("a kind");
        let name = message["target"]["name"].as_str().expect("a name");
        let ran = Command::new(executable)
            .current_dir(directory)
            .output()
            .expect("the test executable runs");
        passed.insert(format!("{package}/{kind}/{name}"), ran.status.success());
    }
    let doctests = |selected: &[&str]| {
        let mut arguments = vec!["test", "--doc"];
        arguments.extend_from_slice(selected);
        if !features.is_empty() {
            arguments.extend(["--features", joined.as_str()]);
        }
        cargo(fixture, &arguments)
            .output()
            .expect("cargo runs the doctests")
            .status
            .success()
    };
    let every = doctests(&["--workspace"]);
    for package in packages.values() {
        let one = every || doctests(&["-p", package.as_str()]);
        passed.insert(format!("{package}/doc/{package}"), one);
    }
    passed
}

/// A file an edit wrote over, and what it held before, or nothing where the edit added it.
struct Saved {
    path: PathBuf,
    before: Option<Vec<u8>>,
}

/// An edit put into the copy: what to put back, and the features to build with.
struct Applied {
    saved: Vec<Saved>,
    features: Vec<String>,
}

/// Puts every file of the edit named `edit` into the copy.
fn apply(root: &Path, edit: &str) -> Applied {
    let directory = root.join("edits").join(edit);
    let mut applied = Applied {
        saved: Vec::new(),
        features: Vec::new(),
    };
    let mut pending = vec![directory.clone()];
    while let Some(next) = pending.pop() {
        for entry in std::fs::read_dir(&next).expect("an edit's directory") {
            let entry = entry.expect("an entry");
            let path = entry.path();
            if entry.file_type().expect("an entry's type").is_dir() {
                pending.push(path);
                continue;
            }
            let relative = path.strip_prefix(&directory).expect("inside the edit");
            if relative == Path::new("FEATURES") {
                let named = std::fs::read_to_string(&path).expect("the features");
                applied
                    .features
                    .extend(named.split_whitespace().map(str::to_owned));
                continue;
            }
            let target = root.join(relative);
            let before = match std::fs::read(&target) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => panic!("{}: {error}", target.display()),
            };
            std::fs::write(&target, std::fs::read(&path).expect("the edited file"))
                .expect("the edit");
            applied.saved.push(Saved {
                path: target,
                before,
            });
        }
    }
    applied
}

/// Takes an edit back out.
fn restore(saved: Vec<Saved>) {
    for Saved { path, before } in saved {
        match before {
            Some(bytes) => std::fs::write(&path, bytes).expect("the file as it was"),
            None => std::fs::remove_file(&path).expect("a file the edit added"),
        }
    }
}

#[test]
fn every_edit_breaks_exactly_the_targets_its_readme_names() {
    let fixture = Fixture::copy(FIXTURE);
    let stated = stated(fixture.root());
    assert_eq!(
        stated.keys().cloned().collect::<BTreeSet<_>>(),
        carried(fixture.root()),
        "every edit under edits/ is named in the README's block, and the block names nothing \
         that is not there"
    );
    let packages = packages(&fixture);
    let before = outcomes(&fixture, &packages, &[]);
    let failing: Vec<&String> = before
        .iter()
        .filter_map(|(target, passed)| (!passed).then_some(target))
        .collect();
    assert!(
        failing.is_empty(),
        "every target passes before any edit, or what an edit breaks is not the edit's: \
         {failing:?}"
    );
    for (edit, breaks) in &stated {
        for target in breaks {
            assert!(
                before.contains_key(target),
                "{edit} names {target}, which the fixture does not build: {before:?}"
            );
        }
        let Applied { saved, features } = apply(fixture.root(), edit);
        let after = outcomes(&fixture, &packages, &features);
        restore(saved);
        let broken: BTreeSet<String> = after
            .into_iter()
            .filter_map(|(target, passed)| (!passed).then_some(target))
            .collect();
        assert_eq!(
            &broken, breaks,
            "{edit} breaks exactly the targets the README names, which is what a selection \
             asked about it has to run"
        );
    }
}
