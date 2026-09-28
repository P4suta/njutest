// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That the gates a person runs are the gates the pipeline runs, and that the inner loop is the fast half of the suite.

#![expect(
    clippy::panic,
    reason = "the helpers that read the repository's own files are not themselves tests: a task \
              file that cannot be read leaves nothing to assert"
)]

use std::collections::{BTreeMap, BTreeSet};
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

fn advisories_ignore_is_empty(deny: &str) -> bool {
    let Ok(configuration) = toml::from_str::<toml::Value>(deny) else {
        return false;
    };
    configuration
        .get("advisories")
        .and_then(|advisories| advisories.get("ignore"))
        .and_then(toml::Value::as_array)
        .is_some_and(Vec::is_empty)
}

fn mise_runs<'a>(configuration: &'a toml::Value, name: &str) -> Vec<&'a str> {
    let run = configuration
        .get("tasks")
        .and_then(|tasks| tasks.get(name))
        .and_then(|task| task.get("run"))
        .unwrap_or_else(|| panic!("mise task {name} has no run command"));
    match run {
        toml::Value::String(command) => vec![command],
        toml::Value::Array(commands) => commands
            .iter()
            .map(|command| {
                command
                    .as_str()
                    .unwrap_or_else(|| panic!("mise task {name} has a non-command run entry"))
            })
            .collect(),
        _ => panic!("mise task {name} has an unknown run shape"),
    }
}

fn ci_run_steps(workflow: &str, job: &str) -> Vec<(String, String)> {
    let header = format!("  {job}:");
    let mut in_job = false;
    let mut steps: Vec<Vec<&str>> = Vec::new();
    for line in workflow.lines() {
        if !in_job {
            in_job = line == header;
            continue;
        }
        if line
            .strip_prefix("  ")
            .is_some_and(|tail| !tail.starts_with([' ', '#']) && tail.ends_with(':'))
        {
            break;
        }
        if let Some(first) = line.strip_prefix("      - ") {
            steps.push(vec![first]);
        } else if let Some(step) = steps.last_mut() {
            step.push(line);
        }
    }
    assert!(in_job, "CI has no job {job}");
    let mut found = Vec::new();
    for step in steps {
        let mut lines = step.into_iter();
        let first = lines.next().unwrap_or_default();
        let mut run = None;
        let mut block = Vec::new();
        let mut in_block = false;
        for line in lines {
            if let Some(command) = line.strip_prefix("        run: ") {
                run = Some(command);
                in_block = command == "|";
            } else if in_block && (line.starts_with("          ") || line.is_empty()) {
                block.push(line.strip_prefix("          ").unwrap_or_default());
            } else {
                in_block = false;
            }
        }
        if first.starts_with("run: ") || run.is_some() {
            let name = first
                .strip_prefix("name: ")
                .unwrap_or_else(|| panic!("CI {job} has an unnamed run step"));
            let command = run.unwrap_or_else(|| panic!("CI {job}/{name} has no run"));
            let script = if command == "|" {
                block.join("\n")
            } else {
                command.to_owned()
            };
            found.push((name.to_owned(), script.trim_end().to_owned()));
        }
    }
    found
}

const FETCH_LOCKED: &str =
    "cargo fetch --locked\ncargo fetch --locked --manifest-path fuzz/Cargo.toml";
const DENY_FETCH: &str = "for attempt in 1 2 3 4 5; do\n  cargo deny --locked --all-features fetch db && exit 0\n  sleep $((attempt * 15))\ndone\nexit 1";
const AUDIT_FETCH: &str = "for attempt in 1 2 3 4 5; do\n  rm -rf \"$HOME/.cargo/advisory-db\"\n  git clone --depth 1 https://github.com/RustSec/advisory-db.git \"$HOME/.cargo/advisory-db\" && exit 0\n  sleep $((attempt * 15))\ndone\nexit 1";
const COMMITTED_CI: &str = "git fetch --no-tags origin \"refs/heads/${BASE_REF}:refs/remotes/origin/${BASE_REF}\"\ngit cat-file -e \"${HEAD_SHA}^{commit}\"\ncommitted \"origin/${BASE_REF}..${HEAD_SHA}\"";

#[derive(Clone, Copy)]
enum CiCommand {
    Task(&'static str),
    Runs(&'static str, &'static [usize]),
    CiOnly(&'static str, &'static str),
}

fn lint_ci_steps(fetch: CiCommand) -> Vec<(&'static str, CiCommand)> {
    use CiCommand::{CiOnly, Runs};
    vec![
        ("Fetch the locked dependency graph", fetch),
        ("fmt", Runs("fmt:check", &[0])),
        ("clippy", Runs("clippy", &[0])),
        ("doc", Runs("doc", &[0])),
        (
            "repository gates (seam ratchet, dependency direction, fixtures, release consistency)",
            Runs("gates", &[0]),
        ),
        (
            "fuzz targets satisfy the root Clippy policy",
            Runs("fuzz:clippy", &[0]),
        ),
        ("typos", Runs("lint", &[0])),
        ("taplo", Runs("fmt:check", &[1, 2])),
        ("actionlint", Runs("lint", &[1])),
        (
            "every workflow the documentation shows passes actionlint against this repository's actions",
            Runs("lint", &[2]),
        ),
        (
            "committed (pull request commits)",
            CiOnly(
                COMMITTED_CI,
                "CI fetches the PR base and checks its head SHA before the local committed-range rule",
            ),
        ),
    ]
}

fn expected_ci_steps(job: &str) -> Vec<(&'static str, CiCommand)> {
    use CiCommand::{CiOnly, Runs, Task};
    let fetch = CiOnly(
        FETCH_LOCKED,
        "CI primes both lockfiles before an offline run",
    );
    match job {
        "test" => vec![
            ("Fetch the locked dependency graph", fetch),
            ("Test every target", Task("test:ci")),
            ("Doctests", Runs("test:doc", &[0])),
            (
                "Lint what only this platform compiles",
                Runs("clippy", &[0]),
            ),
        ],
        "lint" => lint_ci_steps(fetch),
        "deny" => vec![
            (
                "Fetch the advisory database",
                CiOnly(
                    DENY_FETCH,
                    "CI retries the network fetch before the shared offline deny check",
                ),
            ),
            ("Check dependency policy", Runs("deny", &[1])),
        ],
        "audit" => vec![
            (
                "Fetch the advisory database",
                CiOnly(
                    AUDIT_FETCH,
                    "CI uses its isolated Cargo home; the local audit task uses a target-local database",
                ),
            ),
            (
                "Audit dependency advisories",
                CiOnly(
                    "cargo audit --no-fetch --deny warnings",
                    "CI audits its Cargo-home database; the local audit task passes its target-local database with --db",
                ),
            ),
        ],
        "package-install" => vec![
            ("Fetch the locked dependency graph", fetch),
            (
                "Package both CLIs, install them, and run them from the archive a release would publish",
                Task("package"),
            ),
        ],
        "book" => vec![(
            "build the book, which refuses a summary naming a page nobody holds",
            Runs("book", &[0]),
        )],
        "kani-verified" => vec![
            ("Fetch the locked dependency graph", fetch),
            (
                "Install the exact verifier and backend",
                CiOnly(
                    "cargo install --locked kani-verifier --version '=0.68.0'\ncargo kani setup",
                    "CI installs the verifier that the local Kani task requires",
                ),
            ),
            (
                "Prove the production mutation outcome laws",
                Task("kani:laws"),
            ),
            (
                "Exercise generated harnesses and the retained-result protocol with real Kani",
                Task("kani:verified"),
            ),
        ],
        _ => panic!("locally answerable CI job {job} has no step-command correspondence"),
    }
}

#[test]
fn every_locally_answerable_ci_step_runs_its_mise_command() {
    let workflow = repository(".github/workflows/ci.yml");
    let mise: toml::Value = toml::from_str(&repository("mise.toml"))
        .unwrap_or_else(|error| panic!("mise.toml: {error}"));
    let mut used: BTreeMap<&str, BTreeSet<usize>> = BTreeMap::new();
    for (job, local) in GATED {
        if local.is_none() {
            continue;
        }
        let actual = ci_run_steps(&workflow, job);
        let expected = expected_ci_steps(job);
        assert_eq!(
            actual
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            expected.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
            "CI {job} added, removed, or reordered a run step without accounting for its local command"
        );
        for ((name, script), (_, command)) in actual.iter().zip(expected) {
            let (wanted, reason) = match command {
                CiCommand::Task(task) => {
                    assert!(!mise_runs(&mise, task).is_empty());
                    (format!("mise run {task}"), "CI invokes the task itself")
                }
                CiCommand::Runs(task, indices) => {
                    let runs = mise_runs(&mise, task);
                    used.entry(task)
                        .or_default()
                        .extend(indices.iter().copied());
                    let selected = indices
                        .iter()
                        .map(|index| {
                            *runs
                                .get(*index)
                                .unwrap_or_else(|| panic!("mise task {task} lost command {index}"))
                        })
                        .collect::<Vec<_>>()
                        .join(" && ");
                    (selected, "CI runs the same command as the local task")
                }
                CiCommand::CiOnly(script, reason) => (script.to_owned(), reason),
            };
            assert_eq!(script, &wanted, "CI {job}/{name}: {reason}");
        }
    }
    let deny = mise_runs(&mise, "deny");
    let deny_fetch = deny
        .first()
        .unwrap_or_else(|| panic!("mise deny has no fetch command"));
    assert!(DENY_FETCH.contains(&format!("{deny_fetch} && exit 0")));
    used.entry("deny").or_default().insert(0);
    for (task, selected) in used {
        let every: BTreeSet<usize> = (0..mise_runs(&mise, task).len()).collect();
        assert_eq!(
            selected, every,
            "mise task {task} has a command CI does not run"
        );
    }
    let audit = mise_runs(&mise, "audit");
    assert_eq!(audit.len(), 1, "CI does not run an added audit command");
    let audit_run = audit
        .first()
        .unwrap_or_else(|| panic!("mise audit has no run command"));
    assert!(audit_run.lines().any(|line| line.trim() == "git clone --quiet --depth 1 https://github.com/RustSec/advisory-db.git \"${db}\" && fetched=true"));
    assert!(audit_run.lines().any(|line| line.trim() == "cargo audit --no-fetch --deny warnings --db \"${db}\" || {"));
}

/// Every job `required` waits for, and the local task that answers it first.
///
/// `None` is a job this machine cannot answer, with the reason it cannot.
/// The list is the whole of what a push has to wait for CI to find out, so adding to it is a decision rather than an omission.
const GATED: [(&str, Option<&str>); 11] = [
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
    ("action-smoke-rust-mutants", None),
];

#[test]
fn the_gates_a_person_runs_are_the_gates_the_pipeline_runs() {
    let local = task("gates");
    assert!(
        local.contains("cargo xtask all"),
        "`mise run gates` lists gates of its own rather than running the one command CI \
         runs, which is a second enumeration of the typed repository gate set: {local}"
    );
    let hooks = repository("lefthook.yml");
    assert!(
        hooks.contains("run: cargo xtask pre-push\n      use_stdin: true"),
        "the pre-push hook runs something other than the exact-object gate, or does not hand it \
         git's ref updates on stdin; what the gate does with them is held by pre_push.rs: {hooks}"
    );
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
    let hooks = repository("lefthook.yml");
    assert!(
        hooks.contains("run: cargo xtask pre-push"),
        "pre-push no longer runs the gate that binds the commit check to the exact object being \
         pushed; `a_check_is_told_which_commit_it_answers_for` in pre_push.rs holds what that \
         gate hands the check: {hooks}"
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
        "cargo install --locked --path crates/njutest --root \"$install_root\"",
        "cargo install --locked --path crates/rust-mutants-cli --root \"$install_root\"",
        "\"$install_root/bin/njutest\" --version",
        "\"$install_root/bin/rust-mutants\" --version",
        "cargo njutest --version",
        "cargo rust-mutants --version",
        "cargo xtask sbom",
        "target=\"${NJUTEST_BUNDLE_TARGET:-$(rustc --print host-tuple)}\"",
        "cargo xtask bundle --target \"${target}\" --out \"$install_root/dist\"",
        "tar -xzf \"$install_root/dist/\"*.tar.gz -C \"$install_root/unpacked\"",
        "\"$program\" --version",
        "cp rust-toolchain.toml \"$install_root/assured/\"",
        "\"$bundle/rust-mutants\" run --root \"$install_root/assured\" --offline --locked --no-cache",
        "\"$bundle/njutest\" verify --directory \"$install_root/assured\" --offline --locked --no-cache --ui=plain",
    ] {
        assert!(
            package.contains(held),
            "the local package-install proof is missing {held:?}: {package}"
        );
    }
    for forbidden in [
        "cargo publish",
        "gh release",
        "git tag",
        "git push",
        "gh workflow",
    ] {
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
fn the_windows_cfg_is_linted_with_the_pinned_target() {
    let setup = task("\"setup:windows-target\"");
    assert!(
        setup.contains("rustup target add --toolchain 1.98.0 x86_64-pc-windows-msvc"),
        "the cross-target standard library is not tied to the pinned compiler: {setup}"
    );
    let windows = task("\"lint:windows\"");
    assert!(
        windows.contains("depends = [\"setup:windows-target\"]")
            && windows.contains("--target x86_64-pc-windows-msvc")
            && windows.contains("--all-targets")
            && !windows.contains("--all-features"),
        "the Windows cfg check can run without its exact target, omit a non-benchmark target or \
         feature, or pull in the host-only benchmark toolchain: {windows}"
    );
    assert!(
        windows.contains("cargo clippy") && windows.contains("-- -D warnings"),
        "a type check reads code behind `cfg(windows)` and passes a lint CI's Windows job fails \
         on, as `missing_const_for_fn` on a function the other platform's branch made const did; \
         the Windows reading is clippy with every warning an error: {windows}"
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

    let cross = task("\"check:cross\"");
    assert!(
        cross.contains("mise run lint:windows") && cross.contains("mise run lint:linux"),
        "the cross-target reads are no longer one task a person can ask for: {cross}"
    );
    let full = task("\"check:full\"");
    assert!(
        full.contains("mise run check:cross"),
        "nothing local runs the cross-target reads any more: {full}"
    );
    let ci = repository(".github/workflows/ci.yml");
    for runner in ["windows-2025", "ubuntu-24.04"] {
        assert!(
            ci.contains(runner),
            "the cross check complements rather than replaces the real {runner} job"
        );
    }
}

#[test]
fn benchmarks_are_one_explicit_optional_feature_per_benchmarking_crate() {
    for (manifest_name, expected_benches) in [
        ("crates/njutest/Cargo.toml", ["report", "evidence"]),
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
        .split_once("  required:")
        .and_then(|(_before, rest)| rest.split_once("needs:"))
        .and_then(|(_before, rest)| rest.split_once(']'))
        .map(|(list, _rest)| list.to_owned());
    assert_eq!(
        option_state(needs.as_ref()),
        OptionState::Present,
        "required names the jobs it waits for"
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
        "a job was added to or taken from `required` without saying whether a push can \
         find out about it first, which is how a twenty-minute answer becomes the only \
         answer"
    );

    let check = task("check");
    let full = task("\"check:full\"");
    let cross = task("\"check:cross\"");
    let release = task("\"check:release\"");
    let reachable = format!("{check}{full}{cross}{release}");
    for (job, locally) in GATED {
        let Some(locally) = locally else {
            continue;
        };
        assert!(
            reachable.contains(locally),
            "`{job}` is a gate this machine can answer and nothing local runs `{locally}` any \
             more, so the only way to hear it is to push and wait: {reachable}"
        );
    }
    for chained in [
        "mise run check",
        "mise run check:cross",
        "mise run check:release",
    ] {
        assert!(
            full.contains(chained),
            "`check:full` no longer reaches `{chained}`, so a list a push skips has nothing \
             that runs it: {full}"
        );
    }
}

#[test]
fn every_ci_platform_runs_the_same_complete_suite() {
    let workflow = repository(".github/workflows/ci.yml");
    let test_job = workflow
        .split_once("  test:")
        .and_then(|(_, tail)| tail.split_once("\n  lint:"))
        .map(|(job, _)| job)
        .expect("the test job precedes the lint job");
    assert!(
        test_job.contains("os: [macos-26, macos-15, windows-2025]"),
        "each supported platform has a full test row"
    );
    assert!(
        !test_job.contains("matrix.part") && !test_job.contains("part:"),
        "no CI test row selects only part of the suite"
    );
    assert_eq!(
        test_job.matches("mise run test:ci").count(),
        1,
        "one complete suite run per row"
    );
    let local = task("\"test:ci\"");
    assert!(
        local.contains("cargo xtask tidy -- cargo nextest run --locked --workspace --all-targets --all-features --no-fail-fast --status-level fail --test-threads 2"),
        "the CI task covers every target: {local}"
    );
    assert!(
        !local.contains(" -E ") && !local.contains("--exclude"),
        "the CI test row must not filter or shard the suite"
    );
}

#[test]
fn coverage_runs_the_whole_suite_once_without_sharding() {
    let workflow = repository(".github/workflows/ci.yml");
    let steps = ci_run_steps(&workflow, "coverage");
    let collect = steps
        .iter()
        .find(|(name, _)| name == "Collect coverage (run the suite once)")
        .map(|(_, script)| script.as_str())
        .expect("the coverage collection step");
    let local = task("coverage");
    for script in [collect, &local] {
        assert_eq!(
            script
                .matches("cargo xtask tidy -- cargo nextest run")
                .count(),
            1,
            "coverage runs its complete suite once: {script}"
        );
        assert!(
            script.contains("--workspace --all-targets --all-features")
                && !script.contains(" -E ")
                && !script.contains("--exclude"),
            "coverage does not filter the suite: {script}"
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
            let every_gate = source.contains("gates::all(");
            let verified_in_process = (source.contains("Fixture::copy")
                || source.contains("copy_tree("))
                && source.contains("\"verify\"");
            let starts_cargo = !scripted
                && (source.contains("Workspace::open")
                    || against_a_fixture
                    || every_gate
                    || verified_in_process);
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

    seed_copier_takes_the_hidden_one_too();
}

/// Runs the seed copier against a fixture and checks it took the hidden seed as well as the visible one.
///
/// The copier is a POSIX shell script, and the workflow that runs it builds fuzz targets with a nightly toolchain and a sanitizer, which is unix-only.
/// On Windows `bash` is whatever the search path happens to name, and what it names here is a WSL relay with no distribution behind it.
#[cfg(not(unix))]
const fn seed_copier_takes_the_hidden_one_too() {}

/// Runs the seed copier against a fixture and checks it took the hidden seed as well as the visible one.
#[cfg(unix)]
fn seed_copier_takes_the_hidden_one_too() {
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

/// The copier is a POSIX shell script run where fuzz targets build, which is unix.
#[cfg(unix)]
#[test]
fn every_fuzz_run_replays_the_crashes_an_earlier_run_found_before_it_explores() {
    let fixture = tempfile::tempdir()
        .unwrap_or_else(|error| panic!("could not make a fuzz-regression fixture: {error}"));
    let kept = fixture.path().join("regressions/example");
    std::fs::create_dir_all(&kept)
        .unwrap_or_else(|error| panic!("could not make {}: {error}", kept.display()));
    std::fs::write(kept.join("crash-1"), b"crashed once")
        .unwrap_or_else(|error| panic!("could not write a kept crash: {error}"));
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
    let copied = fixture.path().join("corpus/example/crash-1");
    let actual = std::fs::read_to_string(&copied).unwrap_or_else(|error| {
        panic!(
            "{} was not copied, so a run starts without the input that crashed the last one \
             and a crash that came back is found again only by luck: {error}",
            copied.display()
        )
    });
    assert_eq!(actual, "crashed once");
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
    for name in ["\"test:fast\"", "\"test:slow\"", "\"test:ci\"", "coverage"] {
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
    assert!(
        workflow.contains("run: mise run test:ci"),
        "CI must use the task that builds its scripted example before testing"
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
        advisories_ignore_is_empty(&deny),
        "a waived advisory is a decision somebody made about a vulnerability, and an \
         ignore list that grows without a number going up in a file of its own is the \
         ledger this repository refuses everywhere else: {deny}"
    );
}

#[test]
fn an_advisory_ignore_cannot_be_hidden_behind_a_comment() {
    for deny in [
        "[advisories]\n# ignore = []\nignore = [\"RUSTSEC-2026-0001\"]\n",
        "[advisories]\n# ignore = []\n",
    ] {
        assert!(!advisories_ignore_is_empty(deny), "{deny}");
    }
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
const PLUMBING: [&str; 5] = [
    "cargo fetch --locked",
    "rustup toolchain install",
    "cargo +nightly miri setup",
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

#[test]
fn every_suite_leaves_nothing_in_a_temporary_directory_nobody_owns() {
    for place in ["mise.toml", ".github/workflows/ci.yml"] {
        let text = repository(place);
        let bare: Vec<&str> = text
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter(|line| line.contains("cargo nextest run"))
            .filter(|line| !line.contains("cargo xtask tidy -- cargo nextest run"))
            .collect();
        assert!(
            bare.is_empty(),
            "{place} runs a suite without `cargo xtask tidy`, so a test that leaves a directory \
             in the shared temporary directory passes: 1,545 of them in one day on this machine \
             before the rule existed, and the slow lane's toolchain tests leave the most \
             (ADR 0006):\n  {}",
            bare.join("\n  ")
        );
    }
}

#[test]
fn a_nested_toolchain_run_shares_the_machine_with_the_ones_beside_it() {
    let config = repository(".config/nextest.toml");
    let grouped = format!(
        "max-threads = {}",
        njutest_devkit::paths::TOOLCHAIN_TESTS_AT_ONCE
    );
    assert!(
        config.contains("[test-groups]")
            && config.contains(&grouped)
            && config.contains("test-group = \"toolchain\""),
        "a toolchain suite run by any nextest invocation, not only `mise run test:slow`, runs at \
         most {} of its tests at once, each starting its own cargo: {config}",
        njutest_devkit::paths::TOOLCHAIN_TESTS_AT_ONCE
    );
    for environment in [
        njutest_devkit::paths::environment_for_a_run(),
        njutest_devkit::paths::environment_for_a_toolchain_run(&[]),
    ] {
        let jobs: Vec<_> = environment
            .iter()
            .filter(|(name, _value)| name == "CARGO_BUILD_JOBS")
            .map(|(_name, value)| value.clone())
            .collect();
        assert_eq!(
            jobs,
            [std::ffi::OsString::from(
                njutest_devkit::paths::nested_build_jobs().to_string()
            )],
            "a cargo a test starts takes every core as its own, so the tests running beside it \
             multiply the machine by their number"
        );
    }
}

#[test]
fn claims_oracle_runs_without_competing_nested_cargos() {
    let text = repository(".config/nextest.toml");
    let config: toml::Value = toml::from_str(&text).expect("the nextest configuration parses");
    let overrides = config
        .get("profile")
        .and_then(|profile| profile.get("default"))
        .and_then(|default| default.get("overrides"))
        .and_then(toml::Value::as_array)
        .expect("the nextest default profile has overrides");
    let filter = "binary(=toolchain_gates) & test(/^claims_oracle::/)";
    let mut matching = overrides.iter().enumerate().filter(|(_, override_)| {
        override_.get("filter").and_then(toml::Value::as_str) == Some(filter)
    });
    let (at, specific) = matching.next().expect("the claims oracle override exists");
    assert!(
        matching.next().is_none(),
        "claims oracle has one exact override: {text}"
    );
    assert_eq!(
        specific
            .get("threads-required")
            .and_then(toml::Value::as_str),
        Some("num-test-threads"),
        "the claims oracle reserves the entire test pool: {text}"
    );
    let broad = overrides
        .iter()
        .position(|override_| {
            override_.get("filter").and_then(toml::Value::as_str) == Some("binary(/^toolchain_/)")
        })
        .expect("the toolchain override exists");
    assert!(
        at < broad,
        "the narrower override precedes the broad toolchain override: {text}"
    );
}

/// Every command in `place` that compiles the workspace in the dev profile for a build or a test.
fn dev_builds(place: &str) -> Vec<String> {
    repository(place)
        .lines()
        .map(str::trim_start)
        .filter(|line| !line.starts_with('#'))
        .filter_map(|line| {
            ["cargo build ", "cargo test ", "cargo nextest run "]
                .iter()
                .find_map(|verb| line.find(verb))
                .and_then(|at| line.get(at..))
        })
        .filter(|command| !command.contains("--release"))
        .map(str::to_owned)
        .collect()
}

#[test]
fn every_dev_build_compiles_the_one_feature_set_the_suite_does() {
    let mut divergent: Vec<String> = ["mise.toml", ".github/workflows/ci.yml"]
        .iter()
        .flat_map(|place| {
            dev_builds(place)
                .into_iter()
                .map(move |command| format!("{place}: {command}"))
        })
        .filter(|command| {
            let selects = command.contains("--workspace") && command.contains("--all-features");
            let profiled = !command.contains("--examples") || command.contains("--profile test");
            !(selects && profiled)
        })
        .collect();
    let bacon = repository("bacon.toml");
    for job in bacon.split("\n[jobs.").skip(1) {
        let compiles = job.contains("\"nextest\"") || job.contains("\"build\"");
        if compiles && !(job.contains("\"--workspace\"") && job.contains("\"--all-features\"")) {
            divergent.push(format!(
                "bacon.toml: [jobs.{}",
                job.lines().next().unwrap_or_default()
            ));
        }
    }
    assert!(
        divergent.is_empty(),
        "each of these compiles the core crates under a feature set or a profile of its own, so a \
         push builds them once per variant rather than once, and a person's own run warms \
         nothing the gate reads; an examples build takes `--profile test` so its library is the \
         one the suite links: {divergent:#?}"
    );
}

#[test]
fn an_advisory_check_reads_a_database_fetched_by_a_step_that_tries_again() {
    let workflow = repository(".github/workflows/ci.yml");
    for (check, offline, fetch) in [
        ("cargo deny", "check --disable-fetch", "fetch db"),
        (
            "cargo audit",
            "--no-fetch",
            "git clone --depth 1 https://github.com/RustSec/advisory-db.git",
        ),
    ] {
        let runs: Vec<&str> = workflow
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
            .filter(|line| line.contains(check) && !line.contains(fetch))
            .filter(|line| !line.contains("tools:"))
            .collect();
        assert!(
            !runs.is_empty() && runs.iter().all(|line| line.contains(offline)),
            "{check} fetched the advisory database itself, once, so a network error on that one \
             fetch failed the job, twice on 2026-09-24; it reads a database a step of its own \
             fetched and tried again for ({offline}): {runs:?}"
        );
        assert!(
            workflow.contains(fetch) && workflow.contains("for attempt in 1 2 3 4 5"),
            "and that step is there, and tries again: {fetch}"
        );
    }
}

#[test]
fn a_gate_step_that_fails_says_which_it_was_and_how() {
    let check = task("check");
    let steps: Vec<&str> = check
        .lines()
        .filter(|line| line.contains("mise run"))
        .collect();
    assert!(
        !steps.is_empty() && steps.iter().all(|line| line.starts_with("step mise run")),
        "a push gate failed on 2026-09-25 with a bare `exit status 1` after the licence check, \
         and nobody could tell which step had failed; every step of `check` goes through `step`, \
         which names it and its exit status: {steps:?}"
    );
    let audit = task("audit");
    assert!(
        audit.contains("exited ${status}")
            && audit.contains("--db \"${db}\"")
            && audit.contains("not a fresh one"),
        "`audit` says how each attempt ended, reads a database kept under its own target \
         directory, since gates of several sessions fetching into the one shared checkout at \
         once is a failure nobody's change caused, and says so when it checks against a copy \
         it could not refresh: {audit}"
    );
}

#[test]
fn a_loop_of_durable_writes_answers_to_its_own_watch_and_not_the_cpu_clock() {
    let config = repository(".config/nextest.toml");
    let filter = "filter = 'test(/_while_a_run_replaces_it_is_one_whole_/)'";
    let durable = config.find(filter);
    let in_process = config.find("filter = 'not binary(/^toolchain_/)'");
    assert!(
        config.contains("durable = { max-threads = 1 }")
            && durable.is_some_and(|at| in_process.is_some_and(|then| at < then))
            && config.contains("test-group = \"durable\""),
        "a test that replaces an entry hundreds of times under a reader makes a thousand durable \
         writes, and on a machine busy with other builds each one waits on the disk: it ran past \
         the in-process minute at load 26 and passed alone in under two seconds. Such tests run \
         one at a time, ahead of the in-process override, under a clock their own progress \
         watch is the detector for: {config}"
    );
    let named = ["njutest", "rust-mutants"]
        .iter()
        .filter(|crate_| {
            repository(&format!("crates/{crate_}/tests/replace.rs"))
                .contains("_while_a_run_replaces_it_is_one_whole_")
        })
        .count();
    assert_eq!(
        named, 2,
        "and the filter names the tests that are that loop, in both crates that keep entries"
    );
}
