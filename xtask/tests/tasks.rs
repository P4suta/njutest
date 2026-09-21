// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That the gates a person runs are the gates the pipeline runs, and that the inner loop is the fast half of the suite.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own files are not themselves tests: a task \
              file that cannot be read leaves nothing to assert"
)]

use std::collections::BTreeSet;
use std::path::Path;

use njutest_devkit::result::{OptionState, option_state};

fn repository(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(name))
        .unwrap_or_else(|error| panic!("{name}: {error}"))
}

fn task(name: &str) -> String {
    let text = repository("mise.toml");
    let start = text
        .find(&format!("[tasks.{name}]"))
        .unwrap_or_else(|| panic!("no task {name}"));
    let rest = text.get(start..).unwrap_or_default();
    let end = rest
        .get(1..)
        .and_then(|after| after.find("\n[tasks"))
        .map_or(rest.len(), |at| at.saturating_add(1));
    rest.get(..end).unwrap_or_default().to_owned()
}

/// Every gate `cargo xtask all` runs, which is what CI runs.
/// Every job `ci-success` waits for, and the local task that answers it first.
///
/// `None` is a job this machine cannot answer, with the reason it cannot. The
/// list is the whole of what a push has to wait for CI to find out, so adding
/// to it is a decision rather than an omission.
const GATED: [(&str, Option<&str>); 10] = [
    ("test", Some("mise run test")),
    ("lint", Some("mise run lint")),
    ("deny", Some("mise run deny")),
    ("audit", Some("mise run audit")),
    ("book", Some("mise run book")),
    ("package-install", Some("mise run package")),
    ("coverage", None),
    ("soundness", None),
    ("kani-verified", Some("mise run kani:verified")),
    ("action-smoke", None),
];

#[test]
fn the_gates_a_person_runs_are_the_gates_the_pipeline_runs() {
    let local = task("gates");
    assert!(
        local.contains("cargo xtask all"),
        "`mise run gates` lists gates of its own rather than running the one command CI \
         runs, which is a second enumeration of a set `gates::all` already holds and \
         `every_gate_that_needs_no_argument_is_one_all_runs` already closes: {local}"
    );
    let hooks = repository("lefthook.yml");
    assert!(
        hooks.contains("scripts/pre-push-check.sh"),
        "the pre-push hook runs something other than every local gate: {hooks}"
    );
    assert!(
        hooks.contains("run: scripts/pre-push-check.sh\n      use_stdin: true"),
        "the exact-SHA gate does not receive git's ref updates on stdin: {hooks}"
    );
    let pre_push = repository("scripts/pre-push-check.sh");
    for held in [
        "git rev-parse --verify HEAD",
        "git -C \"${repository}\" worktree add --quiet --detach",
        "git -C \"${checkout}\" status --porcelain=v1 --untracked-files=all",
        "mise run check",
        "require_exact_tree\n(cd \"${checkout}\" && NJUTEST_COMMITTED_HEAD=\"${head}\" mise run check)",
        "(cd \"${checkout}\" && mise run check:cold)",
    ] {
        assert!(
            pre_push.contains(held),
            "the pre-push gate no longer constructs and proves the checked tree around the full \
             check ({held:?} is absent): {pre_push}"
        );
    }
}

#[test]
fn committed_checks_the_same_unique_to_head_range_locally_in_hooks_and_ci() {
    let range = task("\"committed:range\"");
    for held in [
        "${NJUTEST_COMMITTED_BASE_REF:-origin/main}",
        "${NJUTEST_COMMITTED_HEAD:-HEAD}",
        "committed \"${base_ref}..${head_ref}\"",
    ] {
        assert!(
            range.contains(held),
            "the local commit gate does not describe the unique-to-head range ({held:?}): {range}"
        );
    }
    assert!(
        task("lint").contains("committed:range"),
        "the local lint gate does not run the commit-range check"
    );
    let hook = repository("scripts/pre-push-check.sh");
    assert!(
        hook.contains("NJUTEST_COMMITTED_HEAD=\"${head}\" mise run check"),
        "pre-push does not bind the commit check to the exact object being pushed: {hook}"
    );

    let workflow = repository(".github/workflows/ci.yml");
    for held in [
        "fetch-depth: 0",
        "BASE_REF: ${{ github.event.pull_request.base.ref }}",
        "HEAD_SHA: ${{ github.event.pull_request.head.sha }}",
        "git fetch --no-tags origin \"refs/heads/${BASE_REF}:refs/remotes/origin/${BASE_REF}\"",
        "git cat-file -e \"${HEAD_SHA}^{commit}\"",
        "origin/${BASE_REF}..${HEAD_SHA}",
    ] {
        assert!(
            workflow.contains(held),
            "CI does not derive the same unique-to-head set ({held:?}): {workflow}"
        );
    }
    assert!(
        !workflow.contains("BASE_SHA:") && !workflow.contains("--no-merge-commit"),
        "CI revived the stale pull-request base SHA or rejects intentional merge commits: {workflow}"
    );
    assert!(
        !workflow.contains("refs/remotes/origin/${BASE_REF}\" \"$HEAD_SHA\""),
        "a fork pull request cannot require its head SHA to be fetchable from the base repository: {workflow}"
    );
}

#[test]
fn package_install_deny_and_typos_are_exact_local_ci_pairs() {
    let package = task("package");
    for held in [
        "mktemp -d",
        "export CARGO_TARGET_DIR=\"$install_root/target\"",
        "cargo package --locked --workspace",
        "cargo install --locked --path crates/njutest-cli --root \"$install_root\"",
        "cargo install --locked --path crates/rust-mutants-cli --root \"$install_root\"",
        "\"$install_root/bin/njutest\" --version",
        "\"$install_root/bin/rust-mutants\" --version",
        "cargo njutest --version",
        "cargo rust-mutants --version",
        "cargo xtask sbom",
    ] {
        assert!(
            package.contains(held),
            "the local package-install proof is missing {held:?}: {package}"
        );
    }
    for forbidden in ["cargo publish", "gh release"] {
        assert!(
            !package.contains(forbidden),
            "an ordinary package check performs a release operation {forbidden:?}: {package}"
        );
    }

    let deny = task("deny");
    assert!(
        deny.contains("cargo deny --locked --all-features check"),
        "the local dependency policy omits feature-enabled edges: {deny}"
    );
    let workflow = repository(".github/workflows/ci.yml");
    for held in ["run: typos", "cargo deny --locked --all-features check"] {
        assert!(
            workflow.contains(held),
            "CI drifted from the pinned local policy ({held:?}): {workflow}"
        );
    }
}

#[test]
fn pre_push_type_checks_windows_cfg_with_the_pinned_target() {
    let setup = task("\"setup:windows-target\"");
    assert!(
        setup.contains("rustup target add --toolchain 1.98.0 x86_64-pc-windows-msvc"),
        "the cross-target standard library is not tied to the pinned compiler: {setup}"
    );
    let windows = task("\"check:windows\"");
    assert!(
        windows.contains("depends = [\"setup:windows-target\"]")
            && windows.contains("--target x86_64-pc-windows-msvc")
            && windows.contains("--all-targets")
            && !windows.contains("--all-features"),
        "the Windows cfg check can run without its exact target, omit a non-benchmark target or \
         feature, or pull in the host-only benchmark toolchain: {windows}"
    );
    let selected_features = windows
        .split_once("--features ")
        .and_then(|(_before, features)| features.split_whitespace().next())
        .map(|features| features.split(',').collect::<BTreeSet<_>>())
        .unwrap_or_default();
    let mut command = cargo_metadata::MetadataCommand::new();
    command
        .manifest_path(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("..")
                .join("Cargo.toml"),
        )
        .no_deps()
        .other_options(["--locked".to_owned(), "--offline".to_owned()]);
    let metadata = command
        .exec()
        .unwrap_or_else(|error| panic!("workspace metadata: {error}"));
    let non_benchmark_features = metadata
        .workspace_packages()
        .into_iter()
        .flat_map(|package| {
            package
                .features
                .keys()
                .filter(|feature| feature.as_str() != "benchmarks")
                .map(|feature| format!("{}/{feature}", package.name))
        })
        .collect::<BTreeSet<_>>();
    assert_eq!(
        selected_features,
        non_benchmark_features
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>(),
        "the Windows cfg check's explicitly selected features are not exactly every workspace \
         feature except the host-only benchmark feature: {windows}"
    );

    let check = task("check");
    let build = check.find("mise run build").unwrap_or(usize::MAX);
    let cross = check.find("mise run check:windows").unwrap_or(usize::MAX);
    let lint = check.find("mise run lint").unwrap_or(usize::MAX);
    assert!(
        build < cross && cross < lint,
        "pre-push must cross-typecheck after the native build and before the broader lint/test work: {check}"
    );
    assert!(
        repository(".github/workflows/ci.yml").contains("windows-2025"),
        "the cross check complements rather than replaces the real Windows job"
    );
}

#[test]
fn benchmarks_are_one_explicit_optional_feature_per_benchmarking_crate() {
    for (manifest_name, expected_benches) in [
        ("crates/njutest-cli/Cargo.toml", ["report", "evidence"]),
        ("crates/rust-mutants/Cargo.toml", ["foundation", "pipeline"]),
    ] {
        let manifest_text = repository(manifest_name);
        let manifest: toml::Value = toml::from_str(&manifest_text)
            .unwrap_or_else(|error| panic!("{manifest_name}: {error}"));
        let criterion = manifest
            .get("dependencies")
            .and_then(|dependencies| dependencies.get("criterion"));
        assert_eq!(
            criterion.and_then(|dependency| dependency.get("optional")),
            Some(&toml::Value::Boolean(true)),
            "{manifest_name}: criterion must be an optional normal dependency so required-features can omit every benchmark from cross compilation"
        );
        assert!(
            manifest
                .get("dev-dependencies")
                .and_then(|dependencies| dependencies.get("criterion"))
                .is_none(),
            "{manifest_name}: an unconditional criterion dev-dependency defeats the Windows benchmark boundary"
        );
        let benchmark_feature = manifest
            .get("features")
            .and_then(|features| features.get("benchmarks"))
            .and_then(toml::Value::as_array);
        assert!(
            benchmark_feature.is_some_and(|feature| {
                feature.as_slice() == [toml::Value::String("dep:criterion".to_owned())]
            }),
            "{manifest_name}: the benchmark feature must own exactly criterion"
        );
        let benches = manifest
            .get("bench")
            .and_then(toml::Value::as_array)
            .unwrap_or_else(|| panic!("{manifest_name}: no bench declarations"));
        assert_eq!(benches.len(), expected_benches.len(), "{manifest_name}");
        for expected in expected_benches {
            let bench = benches
                .iter()
                .find(|bench| bench.get("name").and_then(toml::Value::as_str) == Some(expected))
                .unwrap_or_else(|| panic!("{manifest_name}: no {expected} bench"));
            let required = bench
                .get("required-features")
                .and_then(toml::Value::as_array);
            assert!(
                required.is_some_and(|features| {
                    features.as_slice() == [toml::Value::String("benchmarks".to_owned())]
                }),
                "{manifest_name}: {expected} can be selected without the benchmark feature"
            );
        }
    }
}

#[test]
fn every_gate_the_pipeline_waits_for_is_one_this_machine_answered_first() {
    let workflow = repository(".github/workflows/ci.yml");
    let needs = workflow
        .split_once("  ci-success:")
        .and_then(|(_before, rest)| rest.split_once("needs:"))
        .and_then(|(_before, rest)| rest.split_once(']'))
        .map(|(list, _rest)| list.to_owned());
    assert_eq!(
        option_state(needs.as_ref()),
        OptionState::Present,
        "ci-success names the jobs it waits for"
    );
    let Some(needs) = needs else {
        return;
    };
    let waited: Vec<String> = needs
        .trim_start()
        .trim_start_matches('[')
        .split(',')
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .collect();
    let mut named: Vec<String> = GATED.iter().map(|(job, _task)| (*job).to_owned()).collect();
    let mut waited = waited;
    waited.sort();
    named.sort();
    assert_eq!(
        waited, named,
        "a job was added to or taken from `ci-success` without saying whether a push can \
         find out about it first, which is how a twenty-minute answer becomes the only \
         answer"
    );

    let check = task("check");
    for (job, locally) in GATED {
        let Some(locally) = locally else {
            continue;
        };
        assert!(
            check.contains(locally),
            "`{job}` is a gate this machine can answer, but `mise run check` does not run \
             `{locally}`, so a push waits for the pipeline to say what it knew: {check}"
        );
    }
}

#[test]
fn the_inner_loop_starts_no_toolchain_and_the_whole_suite_still_runs_everything() {
    let fast = task("\"test:fast\"");
    assert!(
        fast.contains("not binary(/^toolchain_/)"),
        "the fast suite is the one that starts no cargo: {fast}"
    );
    let slow = task("\"test:slow\"");
    assert!(slow.contains("binary(/^toolchain_/)"), "{slow}");
    let whole = task("test");
    for half in ["test:fast", "test:slow", "test:doc"] {
        assert!(
            whole.contains(half),
            "`mise run test` leaves out {half}, so something is only ever run in the pipeline: \
             {whole}"
        );
    }
}

#[test]
fn every_suite_that_starts_a_toolchain_says_so_in_its_name() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut wrong = Vec::new();
    let members = njutest_devkit::census::members(&root);
    let suited: Vec<&njutest_devkit::census::Member> = members
        .iter()
        .filter(|member| !member.suites().is_empty())
        .collect();
    assert!(
        suited.len() > 3,
        "the crates are read from cargo so that the crate somebody adds next is covered \
         the day it arrives, and this found almost none: {suited:?}"
    );
    for member in &suited {
        let crate_name = &member.name;
        for (name, path) in member.suites() {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            let scripted = source.contains("fake_cargo::");
            let against_a_fixture = source.contains("Fixture::copy")
                && (source.contains("cargo_binary()") || source.contains("CARGO_BIN_EXE"));
            let starts_cargo =
                !scripted && (source.contains("Workspace::open") || against_a_fixture);
            if starts_cargo && !name.starts_with("toolchain_") {
                wrong.push(format!("{crate_name}/{name}"));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "these suites build a cargo project and are in the inner loop, which is the half \
         of the pipeline that is supposed to answer in seconds: {wrong:?}"
    );
}

#[test]
fn every_task_that_asks_for_pipefail_says_which_shell_it_is_asking() {
    let text = repository("mise.toml");
    let mut without = Vec::new();
    for block in text.split("\n[tasks.") {
        let Some((name, body)) = block.split_once(']') else {
            continue;
        };
        if body.contains("set -euo pipefail") && !body.contains("shell = \"bash -c\"") {
            without.push(name.to_owned());
        }
    }
    assert!(
        without.is_empty(),
        "`sh` is not bash on every machine, and a task that asks for pipefail without saying \
         which shell fails there for a reason that is not the task: {without:?}"
    );
}

#[test]
fn every_disallowed_clippy_shape_is_an_enabled_lint() {
    let workspace = repository("Cargo.toml");
    let configured = repository("clippy.toml");
    for lint in ["disallowed_methods", "disallowed_types"] {
        assert!(
            workspace.contains(&format!("{lint} = \"deny\"")),
            "clippy.toml configures {lint}, but the workspace never enables it: {configured}"
        );
    }
}

#[test]
fn the_independent_fuzz_workspace_derives_the_root_clippy_policy() {
    let lint = task("lint");
    assert!(
        lint.contains("fuzz:clippy"),
        "the ordinary lint task can no longer leave the independent fuzz workspace unchecked: {lint}"
    );
    let fuzz = task("\"fuzz:clippy\"");
    assert!(
        fuzz.contains("cargo xtask fuzz-clippy"),
        "the fuzz task must derive, rather than copy, the root policy: {fuzz}"
    );
    let workflow = repository(".github/workflows/ci.yml");
    assert!(
        workflow.contains("run: cargo xtask fuzz-clippy"),
        "CI must ask the same derived-policy command as a local check"
    );
}

#[test]
fn local_and_weekly_fuzz_runs_copy_the_committed_seeds_into_the_real_corpus() {
    let invocation = "bash ../scripts/seed-fuzz-corpus.sh \"$FUZZ_TARGET\"";
    let workflow = repository(".github/workflows/fuzz.yml");
    assert!(
        workflow.contains("working-directory: fuzz") && workflow.contains(invocation),
        "weekly fuzzing does not seed the corpus from its actual working directory: {workflow}"
    );
    assert!(
        !workflow.contains("fuzz/seeds/$FUZZ_TARGET")
            && !workflow.contains("fuzz/corpus/$FUZZ_TARGET"),
        "a path in the fuzz working directory still redundantly starts with fuzz/: {workflow}"
    );
    let smoke = task("\"fuzz:smoke\"");
    assert!(
        smoke.contains("bash ../scripts/seed-fuzz-corpus.sh \"$target\"")
            && smoke.find("seed-fuzz-corpus").unwrap_or(usize::MAX)
                < smoke.find("cargo +nightly fuzz run").unwrap_or(usize::MAX),
        "the local smoke run does not seed each target before invoking cargo-fuzz: {smoke}"
    );

    let fixture = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("could not make a fuzz-seed fixture: {error}"));
    let seed = fixture.path().join("seeds/example");
    std::fs::create_dir_all(&seed)
        .unwrap_or_else(|error| panic!("could not make {}: {error}", seed.display()));
    std::fs::write(seed.join("visible"), b"seed")
        .unwrap_or_else(|error| panic!("could not write a visible seed: {error}"));
    std::fs::write(seed.join(".hidden"), b"hidden seed")
        .unwrap_or_else(|error| panic!("could not write a hidden seed: {error}"));
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("scripts/seed-fuzz-corpus.sh");
    let output = std::process::Command::new("bash")
        .arg(&script)
        .arg("example")
        .current_dir(fixture.path())
        .output()
        .unwrap_or_else(|error| panic!("could not run {}: {error}", script.display()));
    assert!(
        output.status.success(),
        "seed copier failed: {:?}",
        output.stderr
    );
    for (name, expected) in [("visible", "seed"), (".hidden", "hidden seed")] {
        let copied = fixture.path().join("corpus/example").join(name);
        let actual = std::fs::read_to_string(&copied)
            .unwrap_or_else(|error| panic!("{} was not copied: {error}", copied.display()));
        assert_eq!(actual, expected);
    }
}

#[test]
fn fallible_values_cannot_be_erased_through_convenience_methods() {
    let configured = repository("clippy.toml");
    for method in [
        "core::result::Result::ok",
        "core::result::Result::unwrap_or",
        "core::result::Result::unwrap_or_default",
        "core::result::Result::map_or",
        "core::result::Result::map_or_else",
        "core::result::Result::or",
        "core::result::Result::or_else",
        "std::path::Path::exists",
        "std::path::Path::is_file",
        "std::path::Path::is_dir",
    ] {
        assert!(
            configured.contains(&format!("path = \"{method}\"")),
            "{method} erases an error and must stay in the compiler-backed disallowed-methods gate"
        );
    }
}

#[test]
fn the_gate_catalogue_names_the_compiler_backed_methods_the_policy_holds() {
    let configured = repository("clippy.toml");
    let named: Vec<&str> = configured
        .lines()
        .filter_map(|line| line.split_once("path = \""))
        .filter_map(|(_, rest)| rest.split_once('"'))
        .map(|(path, _)| path)
        .filter(|path| path.starts_with("core::result::Result::"))
        .filter_map(|path| path.rsplit("::").next())
        .collect();
    assert!(named.len() > 4, "the policy names them: {named:?}");
    let page = repository("docs/development.md");
    let unstated: Vec<&&str> = named
        .iter()
        .filter(|method| !page.contains(&format!("`Result::{method}`")))
        .collect();
    assert!(
        unstated.is_empty(),
        "the gate catalogue is what a reader consults before writing the shape it \
         refuses, and a method the policy refuses that the page does not name is one \
         they meet as a compiler error instead: {unstated:?}"
    );
}

#[test]
fn every_task_that_runs_the_suite_builds_the_scripted_toolchain_first() {
    for name in ["\"test:fast\"", "\"test:slow\"", "coverage"] {
        let body = task(name);
        let depends = body
            .lines()
            .find(|line| line.starts_with("depends = ["))
            .unwrap_or_default();
        assert!(
            depends.contains("\"build:examples\""),
            "`cargo test --all-targets` builds an example as a test harness rather than as the \
             program it is, so a suite that drives one fails on a clean checkout without this: \
             {body}"
        );
    }
    let builder = task("\"build:examples\"");
    assert!(builder.contains("--examples"), "{builder}");
}

#[test]
fn the_pipeline_builds_the_scripted_toolchain_before_it_runs_the_suite() {
    let workflow = repository(".github/workflows/ci.yml");
    let built = workflow
        .find("cargo build --locked --examples")
        .unwrap_or_else(|| panic!("no step builds the examples: {workflow}"));
    let tested = workflow
        .find("cargo nextest run --locked --workspace")
        .unwrap_or_else(|| panic!("no step runs the suite: {workflow}"));
    assert!(
        built < tested,
        "the suite runs before the scripted cargo it drives is built"
    );
}

/// Every file under `.github/workflows`, and `mise.toml`, as one text.
fn everything_that_runs_a_gate() -> String {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found = repository("mise.toml");
    let Ok(entries) = std::fs::read_dir(root.join(".github/workflows")) else {
        panic!("the workflows are readable")
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        found.push('\n');
        found.push_str(&text);
    }
    found
}

#[test]
fn nothing_that_runs_a_gate_turns_a_comparison_into_a_recording() {
    let running = everything_that_runs_a_gate();
    for lever in ["UPDATE_GOLDEN", "UPDATE_FATES", "TRYBUILD"] {
        assert!(
            !running.contains(lever),
            "a task or a workflow setting {lever} turns 59 goldens from a comparison \
             into a recording, and every golden test passes forever after. It is set by \
             hand, by somebody reading the diff it writes, and never by a pipeline"
        );
    }
}

#[test]
fn nothing_shrinks_what_a_gate_sees_from_a_file_of_its_own() {
    let nextest = repository(".config/nextest.toml");
    assert!(
        !nextest.contains("default-filter"),
        "a default filter in the nextest profile removes tests from the local run and \
         from the pipeline at once, and both stay green: what a run does not execute is \
         named where somebody asks for it, on the command line: {nextest}"
    );
    let cargo = repository(".cargo/config.toml");
    for key in ["paths", "rustflags", "[patch", "[source"] {
        assert!(
            !cargo.contains(key),
            "{key} in .cargo/config.toml redirects or de-lints what every gate reads, \
             from a file no gate reads: the closed source universe proves what it can \
             see, and this is the one place that can change what that is: {cargo}"
        );
    }
    let deny = repository("deny.toml");
    assert!(
        deny.contains("ignore = []"),
        "a waived advisory is a decision somebody made about a vulnerability, and an \
         ignore list that grows without a number going up in a file of its own is the \
         ledger this repository refuses everywhere else: {deny}"
    );
}

#[test]
fn every_task_a_page_or_a_workflow_names_is_one_mise_declares() {
    let declared = repository("mise.toml");
    let mut named: Vec<String> = Vec::new();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut pending = vec![root.join("docs"), root.join(".github")];
    while let Some(directory) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let path = entry.path();
            if std::fs::metadata(&path).is_ok_and(|one| one.is_dir()) {
                pending.push(path);
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            for part in text.split("mise run ").skip(1) {
                let task: String = part
                    .chars()
                    .take_while(|one| one.is_ascii_alphanumeric() || *one == ':' || *one == '-')
                    .collect();
                if !task.is_empty() {
                    named.push(task);
                }
            }
        }
    }
    named.sort();
    named.dedup();
    assert!(named.len() > 5, "the pages name the tasks: {named:?}");
    let gone: Vec<&String> = named
        .iter()
        .filter(|task| {
            !declared.contains(&format!("[tasks.{task}]"))
                && !declared.contains(&format!("[tasks.\"{task}\"]"))
        })
        .collect();
    assert!(
        gone.is_empty(),
        "a page or a workflow tells a reader to run a task mise does not declare, which \
         is prose about a command that does not exist and, where the page says a gate \
         covers something, a gate nobody runs: {gone:?}"
    );
}

/// The commands a workflow runs that are the pipeline's own plumbing rather than a gate.
const PLUMBING: [&str; 4] = [
    "cargo fetch --locked",
    "rustup toolchain install",
    "cargo llvm-cov report",
    "cargo xtask sbom",
];

#[test]
fn every_gate_the_pipeline_runs_is_one_this_machine_can_run() {
    let workflow = repository(".github/workflows/ci.yml");
    let declared = repository("mise.toml");
    let mut asked = Vec::new();
    for line in workflow.lines() {
        let Some((_, command)) = line.split_once("run: ") else {
            continue;
        };
        let command = command.trim();
        if command.is_empty() || command.starts_with(['>', '|']) {
            continue;
        }
        if PLUMBING.iter().any(|one| command.starts_with(one)) {
            continue;
        }
        asked.push(command.to_owned());
    }
    asked.sort();
    asked.dedup();
    assert!(asked.len() > 8, "the pipeline runs gates: {asked:?}");
    let answered = |command: &str| -> bool {
        if let Some(task) = command.strip_prefix("mise run ") {
            return declared.contains(&format!("[tasks.{task}]"))
                || declared.contains(&format!("[tasks.\"{task}\"]"));
        }
        command
            .split("&&")
            .map(str::trim)
            .all(|part| declared.contains(part))
    };
    let unanswerable: Vec<&String> = asked.iter().filter(|command| !answered(command)).collect();
    assert!(
        unanswerable.is_empty(),
        "the pipeline runs a gate no mise task runs, so a developer cannot answer it \
         before pushing and learns about it twenty minutes later. The job-to-task \
         mapping is by name and says nothing about what either one does: `mise run \
         lint` omitted cargo fmt and taplo for exactly this long. {unanswerable:?}"
    );
}

/// The crate each coverage floor is about, by the order the ratchets appear; the empty name is the whole workspace.
const MEASURED: [&str; 5] = [
    "",
    "crates/rust-mutants",
    "crates/rust-mutants-cli",
    "crates/njutest-cli",
    "xtask",
];

/// Every place in this tree that holds Rust a coverage report can count.
const PLACES: [&str; 8] = [
    "crates/rust-mutants",
    "crates/rust-mutants-cli",
    "crates/njutest",
    "crates/njutest-cli",
    "crates/njutest-macros",
    "crates/njutest-devkit",
    "xtask",
    "fuzz",
];

/// The paths one `--ignore-filename-regex` leaves out, with its one alternation spelled out.
fn left_out(pattern: &str) -> Vec<String> {
    let mut alternatives: Vec<String> = Vec::new();
    let rest = match pattern.split_once('(') {
        None => pattern.to_owned(),
        Some((head, after)) => {
            let (group, tail) = after
                .split_once(')')
                .unwrap_or_else(|| panic!("the alternation closes: {pattern}"));
            let (suffix, beyond) = tail.split_once('|').unwrap_or((tail, ""));
            alternatives.extend(
                group
                    .split('|')
                    .map(|member| format!("{head}{member}{suffix}")),
            );
            beyond.to_owned()
        }
    };
    alternatives.extend(rest.split('|').map(ToOwned::to_owned));
    alternatives.retain(|one| !one.is_empty());
    alternatives
}

#[test]
fn every_coverage_floor_measures_the_one_crate_it_is_about() {
    let coverage = task("coverage");
    let patterns: Vec<&str> = coverage
        .lines()
        .filter_map(|line| line.split_once("--ignore-filename-regex '"))
        .filter_map(|(_, rest)| rest.split_once('\''))
        .map(|(pattern, _)| pattern)
        .collect();
    assert_eq!(
        patterns.len(),
        MEASURED.len(),
        "a floor was added or removed and this table did not follow: {patterns:?}"
    );
    let under = |place: &str| format!("{place}/");
    for (measured, pattern) in MEASURED.into_iter().zip(patterns) {
        let out = left_out(pattern);
        let left_in: Vec<&str> = PLACES
            .into_iter()
            .filter(|place| !out.iter().any(|one| under(place).starts_with(one.as_str())))
            .collect();
        if measured.is_empty() {
            assert_eq!(
                left_in.len(),
                PLACES.len(),
                "the workspace floor leaves a place out, so what it prints is not the \
                 workspace's coverage: {pattern}"
            );
            continue;
        }
        assert_eq!(
            left_in,
            [measured],
            "a floor is a number about one crate, and this one is an average over what \
             it leaves in: raising one crate's tests moves another crate's floor, and a \
             package added anywhere joins every floor silently. {pattern}"
        );
    }
}

#[test]
fn no_double_spells_a_package_identity_the_way_cargo_stopped_spelling_one() {
    let root = std::fs::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
        .unwrap_or_else(|error| panic!("the workspace root: {error}"));
    let mut written = Vec::new();
    for member in njutest_devkit::census::members(&root) {
        let mut files: Vec<std::path::PathBuf> = member
            .suites()
            .into_iter()
            .map(|(_name, path)| path)
            .collect();
        files.extend(rust_sources_under(&member.directory.join("src")));
        for path in files {
            let relative = match path.strip_prefix(&root) {
                Ok(at) => at.display().to_string().replace('\\', "/"),
                Err(error) => panic!("{}: {error}", path.display()),
            };
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            if source.lines().any(retired_identity) {
                written.push(relative);
            }
        }
    }
    written.sort();
    assert!(
        written.is_empty(),
        "cargo stopped spelling a package identity `<name> <version> (<source>)` in 1.77 \
         and spells one `path+file:///abs#version`, or `#name@version` where the \
         directory is not named after the package. Five spellings of one identity were \
         written by hand across this tree and one of them was cargo's, so every double \
         carrying another was asking the reader about a document cargo does not print. \
         `njutest_devkit::cargo_double::package_id` is the one held against real cargo by \
         `rust-mutants::toolchain_metadata_double`: {written:?}"
    );
}

/// Whether a line carries `<name> <version> (<source>)`, the identity cargo printed before 1.77.
fn retired_identity(line: &str) -> bool {
    ["(path+", "(registry+", "(git+"]
        .iter()
        .filter_map(|source| line.split_once(source))
        .any(|(before, _after)| {
            let mut words = before.split_whitespace().rev();
            words.next().is_some_and(is_a_version) && words.next().is_some()
        })
}

/// Whether a word is three dot-separated numbers, which is how cargo wrote a version into one.
fn is_a_version(word: &str) -> bool {
    let parts: Vec<&str> = word.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty() && part.chars().all(|character| character.is_ascii_digit())
        })
}

/// Every `.rs` file under `at`, however deep, and nothing when there is no such directory.
fn rust_sources_under(at: &Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    let entries = match std::fs::read_dir(at) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return found,
        Err(error) => panic!("{}: {error}", at.display()),
    };
    for entry in entries {
        let entry = entry.unwrap_or_else(|error| panic!("{}: {error}", at.display()));
        let path = entry.path();
        match entry.file_type() {
            Ok(kind) if kind.is_dir() => found.extend(rust_sources_under(&path)),
            Ok(_file) if path.extension().is_some_and(|one| one == "rs") => found.push(path),
            Ok(_other) => {}
            Err(error) => panic!("{}: {error}", path.display()),
        }
    }
    found.sort();
    found
}
