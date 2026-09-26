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
    if up == 0 || parts.len().checked_sub(up) != Some(1) {
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
                if let Some(last) = parts.len().checked_sub(1)
                    && last > 0
                {
                    parts.truncate(last);
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
        .map(|path| path.join("run-report-v1.json"))
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

/// The README a fixture states its fates in.
fn readme(name: &str) -> PathBuf {
    njutest_devkit::paths::workspace_root()
        .join("fixtures")
        .join(name)
        .join("README.md")
}

/// Rewrites the block of the README at `path`, keeping everything around it.
fn rewrite(path: &Path, found: &[Fate]) {
    let name = path.display();
    let text = std::fs::read_to_string(path).expect("the README");
    let (before, rest) = text
        .split_once(njutest_devkit::fixture::FATES_FENCE)
        .expect("the block");
    let (fence, rest) = rest.split_once('\n').expect("the fence line");
    let (old, after) = rest.split_once("```").expect("the end of the block");
    assert!(
        !old.contains("```"),
        "the fence that closes the block is the first one after it, so what is replaced \
         is the block and never the page: {name}"
    );
    let mut block = String::new();
    for one in found {
        block.push_str(&one.to_string());
        block.push('\n');
    }
    std::fs::write(
        path,
        format!(
            "{before}{}{fence}\n{block}```{after}",
            njutest_devkit::fixture::FATES_FENCE
        ),
    )
    .expect("rewriting the README");
}

/// A run of `name` establishes exactly the fates its README states, or, with [`UPDATE`] set, the README is made to state them.
fn holds(name: &str) {
    let stated = njutest_devkit::fixture::stated_fates(name);
    assert!(
        stated.stated,
        "{name}: the README states no fates, which `cargo xtask fixtures` refuses"
    );
    let fixture = Fixture::copy_with_siblings(name, &siblings(&stated.args));
    let found = recorded(&fixture, &resolved(&fixture, &stated.args));
    if std::env::var_os(UPDATE).is_some() {
        rewrite(&readme(name), &found);
        return;
    }
    let said: Vec<String> = stated.rows.iter().map(ToString::to_string).collect();
    let is: Vec<String> = found.iter().map(ToString::to_string).collect();
    assert!(
        found == stated.rows,
        "{name}: the README and a run of it disagree; read the difference, then rewrite this \
         fixture's block with {UPDATE}=1 if the run is right\n  the README says:\n    {}\n  the run \
         establishes:\n    {}",
        said.join("\n    "),
        is.join("\n    ")
    );
}

#[test]
fn fixture_2021() {
    holds("fixture-2021");
}

#[test]
fn fixture_annotated() {
    holds("fixture-annotated");
}

#[test]
fn fixture_apparatus() {
    holds("fixture-apparatus");
}

#[test]
fn fixture_assured() {
    holds("fixture-assured");
}

#[test]
fn fixture_balanced_fails_then_hangs() {
    holds("fixture-balanced-fails-then-hangs");
}

#[test]
fn fixture_bare_cargo() {
    holds("fixture-bare-cargo");
}

#[test]
fn fixture_baseline() {
    holds("fixture-baseline");
}

#[test]
fn fixture_build_script() {
    holds("fixture-build-script");
}

#[test]
fn fixture_carry() {
    holds("fixture-carry");
}

#[test]
fn fixture_child_refuses() {
    holds("fixture-child-refuses");
}

#[test]
fn fixture_cleared_child() {
    holds("fixture-cleared-child");
}

#[test]
fn fixture_cleared_under_mutant() {
    holds("fixture-cleared-under-mutant");
}

#[test]
fn fixture_climbs_dep_lib() {
    holds("fixture-climbs-dep-lib");
}

#[test]
fn fixture_coverage() {
    holds("fixture-coverage");
}

#[test]
fn fixture_custom_harness() {
    holds("fixture-custom-harness");
}

#[test]
fn fixture_declines() {
    holds("fixture-declines");
}

#[test]
fn fixture_doctest() {
    holds("fixture-doctest");
}

#[test]
fn fixture_drifts() {
    holds("fixture-drifts");
}

#[test]
fn fixture_durable() {
    holds("fixture-durable");
}

#[test]
fn fixture_edits() {
    holds("fixture-edits");
}

#[test]
fn fixture_entered() {
    holds("fixture-entered");
}

#[test]
fn fixture_environment() {
    holds("fixture-environment");
}

#[test]
fn fixture_equivalent() {
    holds("fixture-equivalent");
}

#[test]
fn fixture_fails_then_hangs() {
    holds("fixture-fails-then-hangs");
}

#[test]
fn fixture_families() {
    holds("fixture-families");
}

#[test]
fn fixture_faulted() {
    holds("fixture-faulted");
}

#[test]
fn fixture_faulted_failure_writes() {
    holds("fixture-faulted-failure-writes");
}

#[test]
fn fixture_faulted_writes() {
    holds("fixture-faulted-writes");
}

#[test]
fn fixture_features() {
    holds("fixture-features");
}

#[test]
fn fixture_forbid() {
    holds("fixture-forbid");
}

#[test]
fn fixture_guarded_or() {
    holds("fixture-guarded-or");
}

#[test]
fn fixture_hang() {
    holds("fixture-hang");
}

#[test]
fn fixture_home() {
    holds("fixture-home");
}

#[test]
fn fixture_hollow() {
    holds("fixture-hollow");
}

#[test]
fn fixture_hollow_only() {
    holds("fixture-hollow-only");
}

#[test]
fn fixture_ignored() {
    holds("fixture-ignored");
}

#[test]
fn fixture_include() {
    holds("fixture-include");
}

#[test]
fn fixture_item_reach() {
    holds("fixture-item-reach");
}

#[test]
fn fixture_killer_last() {
    holds("fixture-killer-last");
}

#[test]
fn fixture_links_nowhere() {
    holds("fixture-links-nowhere");
}

#[test]
fn fixture_macros() {
    holds("fixture-macros");
}

#[test]
fn fixture_modern() {
    holds("fixture-modern");
}

#[test]
fn fixture_no_std() {
    holds("fixture-no-std");
}

#[test]
fn fixture_no_std_freestanding() {
    holds("fixture-no-std-freestanding");
}

#[test]
fn fixture_order_dependent() {
    holds("fixture-order-dependent");
}

#[test]
fn fixture_outside() {
    holds("fixture-outside");
}

#[test]
fn fixture_outside_dep() {
    holds("fixture-outside-dep");
}

#[test]
fn fixture_outside_dep_lib() {
    holds("fixture-outside-dep-lib");
}

#[test]
fn fixture_panics() {
    holds("fixture-panics");
}

#[test]
fn fixture_probeable() {
    holds("fixture-probeable");
}

#[test]
fn fixture_reads_tree() {
    holds("fixture-reads-tree");
}

#[test]
fn fixture_rejectable() {
    holds("fixture-rejectable");
}

#[test]
fn fixture_scheduled() {
    holds("fixture-scheduled");
}

#[test]
fn fixture_scripted() {
    holds("fixture-scripted");
}

#[test]
fn fixture_shared_path() {
    holds("fixture-shared-path");
}

#[test]
fn fixture_silent_kill() {
    holds("fixture-silent-kill");
}

#[test]
fn fixture_simple() {
    holds("fixture-simple");
}

#[test]
fn fixture_stop_status() {
    holds("fixture-stop-status");
}

#[test]
fn fixture_strict_lints() {
    holds("fixture-strict-lints");
}

#[test]
fn fixture_subprocess() {
    holds("fixture-subprocess");
}

#[test]
fn fixture_targets() {
    holds("fixture-targets");
}

#[test]
fn fixture_threaded() {
    holds("fixture-threaded");
}

#[test]
fn fixture_two_bodies() {
    holds("fixture-two-bodies");
}

#[test]
fn fixture_uncompiled() {
    holds("fixture-uncompiled");
}

#[test]
fn fixture_unicode() {
    holds("fixture-unicode");
}

#[test]
fn fixture_unreached() {
    holds("fixture-unreached");
}

#[test]
fn fixture_verify_fails() {
    holds("fixture-verify-fails");
}

#[test]
fn fixture_wired() {
    holds("fixture-wired");
}

#[test]
fn fixture_witness_downstream() {
    holds("fixture-witness-downstream");
}

#[test]
fn fixture_workspace() {
    holds("fixture-workspace");
}

#[test]
fn fixture_writes_tree() {
    holds("fixture-writes-tree");
}

#[test]
fn nested_fixture_climbs_dep() {
    holds("nested/fixture-climbs-dep");
}

#[test]
fn every_fixture_has_a_fates_test_of_its_own() {
    let path = njutest_devkit::paths::workspace_root().join(file!());
    let source = std::fs::read_to_string(&path).expect("this suite's own source");
    let tested: std::collections::BTreeSet<&str> = source
        .match_indices("holds(\"")
        .filter_map(|(at, opening)| source.get(at + opening.len()..))
        .filter_map(|rest| rest.split_once('"').map(|(name, _rest)| name))
        .collect();
    let committed: Vec<String> = fixtures();
    let committed: std::collections::BTreeSet<&str> =
        committed.iter().map(String::as_str).collect();
    let untested: Vec<&&str> = committed.difference(&tested).collect();
    let gone: Vec<&&str> = tested.difference(&committed).collect();
    assert!(
        untested.is_empty() && gone.is_empty(),
        "every fixture is one test here, so one fixture can be run and rewritten alone and a slow \
         one delays nobody; add `#[test] fn <name>() {{ holds(\"<fixture>\"); }}` for each of \
         {untested:?}, and remove the ones for fixtures that are gone: {gone:?}"
    );
}

#[test]
fn an_update_rewrites_the_block_of_the_fixture_it_ran_and_nothing_else() {
    let scratch = tempfile::tempdir().expect("a directory for two READMEs");
    let [ran, beside] = ["fixture-simple", "fixture-hang"].map(|name| {
        let copy = scratch.path().join(format!("{name}.md"));
        std::fs::copy(readme(name), &copy).expect("a copy of a real README");
        copy
    });
    let untouched = std::fs::read(&beside).expect("the README beside it");
    let before = std::fs::read_to_string(&ran).expect("the README that is rewritten");
    let rows = njutest_devkit::fixture::stated_fates("fixture-simple").rows;
    let kept = rows
        .get(..1)
        .expect("fixture-simple states at least one fate");
    rewrite(&ran, kept);
    let after = std::fs::read_to_string(&ran).expect("the rewritten README");
    assert_eq!(
        std::fs::read(&beside).expect("the README beside it"),
        untouched,
        "rewriting one fixture's block touched another fixture's README"
    );
    let outside = |text: &str| {
        let (head, rest) = text
            .split_once(njutest_devkit::fixture::FATES_FENCE)
            .expect("the block");
        let tail = rest
            .split_once("```")
            .map(|(_block, tail)| tail.to_owned())
            .expect("the end of the block");
        (head.to_owned(), tail)
    };
    assert_eq!(
        outside(&after),
        outside(&before),
        "an update rewrote the page around the block, not only the block"
    );
    assert!(
        after.contains(
            &kept
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        ),
        "the block does not state what the run found: {after}"
    );
}

#[test]
fn every_fixture_is_driven_by_a_test_that_names_it() {
    let root = njutest_devkit::paths::workspace_root();
    let mut sources = String::new();
    for crate_name in [
        "rust-mutants",
        "rust-mutants-cli",
        "njutest",
        "njutest",
        "njutest-devkit",
    ] {
        for directory in ["tests", "src", "benches"] {
            let base = root.join("crates").join(crate_name).join(directory);
            for entry in walk(&base)
                .into_iter()
                .filter(|entry| !entry.ends_with(file!()))
            {
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
    sources.match_indices(name).any(|(at, matched)| {
        at.checked_add(matched.len())
            .and_then(|after| sources.get(after..))
            .and_then(|rest| rest.chars().next())
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
        cargo: None,
        ci: rust_mutants_cli::CiHost::None,
    }
}
