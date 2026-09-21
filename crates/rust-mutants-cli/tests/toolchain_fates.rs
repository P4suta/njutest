// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every fixture's README against a run of that fixture: what it says a mutation's fate is, and what one is.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    reason = "the helpers that start the engine and read a fixture are not themselves tests, and \
              a document this test wrote itself is one it may index"
)]

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use njutest_devkit::fixture::{Fate, Fixture};
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

include!("support/directory.rs");
include!("support/metadata.rs");

/// The variable that rewrites the blocks rather than refusing them, as `UPDATE_GOLDEN` does for a golden.
const UPDATE: &str = "UPDATE_FATES";

fn fixtures() -> Vec<String> {
    njutest_devkit::fixture::names()
}

/// The fixture a climbing argument lands on, when it is one.
fn climbed(arg: &str) -> Option<&str> {
    let parts: Vec<&str> = arg.split('/').collect();
    let up = parts.iter().take_while(|part| **part == "..").count();
    if up == 0 || parts.len() != up + 1 {
        return None;
    }
    parts
        .get(up)
        .copied()
        .filter(|last| last.starts_with("fixture-"))
}

/// Every other fixture a block's arguments name, so a path that climbs out of the tree lands in something.
fn siblings(args: &[String]) -> Vec<&str> {
    args.iter().filter_map(|arg| climbed(arg)).collect()
}

/// The block's arguments with each climbing `fixture-…` spelled as the copy's own path.
fn resolved(fixture: &Fixture, args: &[String]) -> Vec<String> {
    args.iter()
        .map(|arg| {
            if climbed(arg).is_none() {
                return arg.clone();
            }
            let landed = normalized(&fixture.root().join(arg));
            njutest_devkit::paths::utf8(&landed).to_owned()
        })
        .collect()
}

/// `path` with every `.` removed and every `..` folded, without asking the filesystem.
fn normalized(path: &Path) -> PathBuf {
    let mut parts: Vec<OsString> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                if parts.len() > 1 {
                    parts.truncate(parts.len() - 1);
                }
            }
            other => parts.push(other.as_os_str().to_owned()),
        }
    }
    parts.iter().collect()
}

/// What a run of one fixture establishes, in the order a block states it.
fn recorded(fixture: &Fixture, args: &[String]) -> Vec<Fate> {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(["run"])
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all", "--offline", "--locked"])
            .chain(args.iter().map(String::as_str))
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    let code = output.status.code();
    let said = njutest_devkit::process::strict_utf8(&output.stderr);
    let directory = rust_mutants_cli::app::stored::Store::read(fixture.root()).root();
    if !test_directory(&directory) {
        assert_eq!(
            code,
            Some(2),
            "a run that wrote no report at all is a run that was refused, and nothing else: \
             {said}"
        );
        return Vec::new();
    }
    assert!(
        code.is_some_and(|code| code <= 2),
        "the run itself failed: {said}"
    );
    rows(&newest(&directory))
}

fn newest(directory: &Path) -> PathBuf {
    let mut runs: Vec<PathBuf> = std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("{}: {error}", directory.display()))
        .map(|entry| entry.expect("stored run directory entry"))
        .map(|entry| entry.path())
        .filter(|path| test_directory(path))
        .map(|path| path.join("run-report-v2.json"))
        .filter(|path| test_metadata(path).is_file())
        .collect();
    runs.sort();
    runs.pop().expect("one stored run")
}

fn rows(report: &Path) -> Vec<Fate> {
    let document: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(report).expect("the report"),
    )
    .expect("the report is a document");
    let text = |value: &serde_json::Value, key: &str| {
        value[key]
            .as_str()
            .unwrap_or_else(|| panic!("{key} is text in {value}"))
            .to_owned()
    };
    let number = |value: &serde_json::Value, key: &str| {
        u32::try_from(
            value[key]
                .as_u64()
                .unwrap_or_else(|| panic!("{key} is an unsigned integer in {value}")),
        )
        .unwrap_or_else(|error| panic!("{key} fits its schema in {value}: {error}"))
    };
    let mut found: Vec<Fate> = document["mutants"]
        .as_array()
        .expect("the rows")
        .iter()
        .map(|row| Fate {
            path: text(row, "path"),
            line: number(row, "line"),
            column: number(row, "column"),
            rule: text(row, "rule"),
            outcome: if row["unreached"].as_bool().unwrap_or(false) {
                "unreached".to_owned()
            } else {
                text(row, "outcome")
            },
        })
        .chain(
            document["rejections"]
                .as_array()
                .expect("the refusals")
                .iter()
                .map(|row| Fate {
                    path: text(row, "path"),
                    line: 0,
                    column: 0,
                    rule: text(row, "rule"),
                    outcome: "refused".to_owned(),
                }),
        )
        .collect();
    found.sort();
    found
}

/// Rewrites the block of one README, keeping everything around it.
fn rewrite(name: &str, found: &[Fate]) {
    let path = njutest_devkit::paths::workspace_root()
        .join("fixtures")
        .join(name)
        .join("README.md");
    let text = std::fs::read_to_string(&path).expect("the README");
    let (before, rest) = text
        .split_once(njutest_devkit::fixture::FATES_FENCE)
        .expect("the block");
    let (fence, rest) = rest.split_once('\n').expect("the fence line");
    let (old, after) = rest.split_once("```").expect("the end of the block");
    assert!(
        !old.is_empty(),
        "the README contained the fate block being replaced"
    );
    let mut block = String::new();
    for one in found {
        block.push_str(&one.to_string());
        block.push('\n');
    }
    std::fs::write(
        &path,
        format!(
            "{before}{}{fence}\n{block}```{after}",
            njutest_devkit::fixture::FATES_FENCE
        ),
    )
    .expect("rewriting the README");
}

#[test]
fn every_fixture_readme_fate_is_the_recorded_one() {
    let updating = std::env::var_os(UPDATE).is_some();
    let mut wrong = Vec::new();
    for name in fixtures() {
        let stated = njutest_devkit::fixture::stated_fates(&name);
        assert!(
            stated.stated,
            "{name}: the README states no fates, which `cargo xtask fixtures` refuses"
        );
        let fixture = Fixture::copy_with_siblings(&name, &siblings(&stated.args));
        let found = recorded(&fixture, &resolved(&fixture, &stated.args));
        if updating {
            rewrite(&name, &found);
            continue;
        }
        if found != stated.rows {
            let said: Vec<String> = stated.rows.iter().map(ToString::to_string).collect();
            let is: Vec<String> = found.iter().map(ToString::to_string).collect();
            wrong.push(format!(
                "{name}\n  the README says:\n    {}\n  the run establishes:\n    {}",
                said.join("\n    "),
                is.join("\n    ")
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "a fixture's README and a run of it disagree; read the difference, then rewrite the \
         blocks with {UPDATE}=1 if the run is right:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn every_fixture_is_driven_by_a_test_that_names_it() {
    let root = njutest_devkit::paths::workspace_root();
    let mut sources = String::new();
    for crate_name in [
        "rust-mutants",
        "rust-mutants-cli",
        "njutest-cli",
        "njutest",
        "njutest-devkit",
    ] {
        for directory in ["tests", "src", "benches"] {
            let base = root.join("crates").join(crate_name).join(directory);
            for entry in walk(&base) {
                sources.push_str(
                    &std::fs::read_to_string(&entry)
                        .unwrap_or_else(|error| panic!("{}: {error}", entry.display())),
                );
            }
        }
    }
    let orphans: Vec<String> = fixtures()
        .into_iter()
        .filter(|name| !names_it(&sources, name))
        .collect();
    assert!(
        orphans.is_empty(),
        "these fixtures are committed and no test names them: {orphans:?}"
    );
}

/// Whether `sources` names this fixture, and not merely a longer name starting with it.
fn names_it(sources: &str, name: &str) -> bool {
    sources.match_indices(name).any(|(at, _)| {
        let after = at + name.len();
        sources[after..]
            .chars()
            .next()
            .is_none_or(|next| !next.is_ascii_alphanumeric() && next != '-')
    })
}

/// Every Rust file under `base`, however deep.
fn walk(base: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(base) else {
        return found;
    };
    for entry in entries {
        let entry = entry.expect("fixture tree directory entry");
        let path = entry.path();
        if test_metadata(&path).is_dir() {
            found.extend(walk(&path));
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            found.push(path);
        }
    }
    found
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}
