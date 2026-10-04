// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![expect(
    clippy::expect_used,
    reason = "a toolchain test cannot assert anything when its fixture or command cannot be prepared"
)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use njutest_devkit::fixture::Fixture;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Claim {
    id: Option<String>,
    path: Option<String>,
    item: Option<String>,
    rule: Option<String>,
    original: Option<String>,
    line: Option<u32>,
    count: Option<u32>,
    reason: String,
    outcome: Option<String>,
    #[serde(rename = "where")]
    under: Option<toml::Value>,
}

impl Claim {
    fn label(&self) -> String {
        if let Some(id) = &self.id {
            return id.clone();
        }
        let path = self.path.as_deref().expect("a locator has a path");
        let item = self.item.as_deref().expect("a locator has an item");
        let rule = self.rule.as_deref().expect("a locator has a rule");
        let original = self
            .original
            .as_deref()
            .expect("a locator has original bytes");
        let mut label = format!("{path} {item} {rule} {original:?}");
        if let Some(line) = self.line {
            write!(label, " @{line}").expect("writing to a String cannot fail");
        }
        label
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CandidatesDocument {
    document_type: String,
    schema_version: u32,
    tool_version: String,
    count: usize,
    candidates: Vec<Candidate>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    id: String,
    display_id: String,
    path: String,
    item: String,
    rule: String,
    original: String,
    replacement: String,
    line: u32,
    column: u32,
}

enum Expected {
    Names(Vec<String>),
    Moved(Vec<String>, u32, u32),
    Elsewhere,
    Unmatched,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, njutest_macros::AllVariants)]
enum Case {
    Names,
    Moved,
    Uncompiled,
    Unmatched,
}

struct SuiteBuild {
    engine: PathBuf,
    compiled: usize,
}

fn suite_build(repository: &Path) -> SuiteBuild {
    let mut command = Command::new(njutest_devkit::paths::cargo_binary());
    command
        .args([
            "test",
            "--no-run",
            "--offline",
            "--locked",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--message-format",
            "json-render-diagnostics",
        ])
        .current_dir(repository);
    let output = njutest_devkit::cost::cargo(command, "the claims oracle's suite build")
        .expect("the suite's build starts");
    assert!(
        output.status.success(),
        "the suite's build completes: {}",
        output.stderr.escape_ascii()
    );
    let text = String::from_utf8(output.stdout).expect("cargo prints UTF-8");
    let mut engines = Vec::new();
    let mut compiled = 0_usize;
    for line in text.lines().filter(|line| !line.trim().is_empty()) {
        let message = xtask::strictjson::from_str(line).expect("cargo prints JSON messages");
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let artifact: cargo_metadata::Artifact =
            serde_json::from_value(message).expect("a compiler artifact message");
        if !artifact.fresh {
            compiled = compiled.checked_add(1).expect("the unit count fits");
        }
        if artifact.target.name == "rust-mutants"
            && artifact.target.is_bin()
            && !artifact.profile.test
            && let Some(executable) = artifact.executable
        {
            engines.push(executable.into_std_path_buf());
        }
    }
    let engine = engines.pop().expect("the suite's build makes the engine");
    assert!(engines.is_empty(), "the suite's build makes one engine");
    SuiteBuild { engine, compiled }
}

fn engine(repository: &Path) -> PathBuf {
    suite_build(repository).engine
}

fn listing(
    (engine, repository): (&Path, &Path),
    workspace: &Path,
    temporary: &Path,
    flags: &[&str],
) -> (i32, String) {
    let output = Command::new(engine)
        .args(["list", "--offline", "--locked", "--root"])
        .arg(workspace)
        .arg("--cargo")
        .arg(njutest_devkit::paths::cargo_binary())
        .args(flags)
        .current_dir(repository)
        .env_clear()
        .envs(njutest_devkit::paths::environment_for_a_run())
        .envs(njutest_devkit::paths::temporary_directory(temporary))
        .env("LLVM_PROFILE_FILE", njutest_devkit::paths::NULL_DEVICE)
        .output()
        .expect("the real command starts");
    let code = output.status.code().expect("the real command exits");
    assert!(
        code == 0 || code == 1,
        "the real command ended {code}: {}",
        output.stderr.escape_ascii()
    );
    (
        code,
        String::from_utf8(output.stdout).expect("the real command prints UTF-8"),
    )
}

fn claims(workspace: &Path) -> Vec<Claim> {
    let text = std::fs::read_to_string(workspace.join(".rust-mutants.toml"))
        .expect("the configuration reads");
    let configuration: toml::Value = toml::from_str(&text).expect("claims parse independently");
    let entries = configuration
        .get("mutation")
        .and_then(|value| value.get("expect"))
        .and_then(toml::Value::as_array)
        .expect("the configuration lists claims");
    entries
        .iter()
        .cloned()
        .map(|entry| {
            let claim: Claim = entry.try_into().expect("a claim has known fields");
            assert!(!claim.reason.is_empty(), "the claim has a reason");
            assert!(
                claim.under.is_none(),
                "this oracle does not model scoped claims"
            );
            assert!(
                claim
                    .outcome
                    .as_deref()
                    .is_none_or(|outcome| !outcome.is_empty()),
                "an explicit outcome is nonempty"
            );
            claim
        })
        .collect()
}

fn candidates(
    engine: (&Path, &Path),
    workspace: &Path,
    temporary: &Path,
    flags: &[&str],
) -> Vec<Candidate> {
    let (code, text) = listing(engine, workspace, temporary, flags);
    assert_eq!(code, 0, "candidate listing completes: {text}");
    let document: CandidatesDocument =
        xtask::strictjson::decode_str(&text).expect("candidate JSON parses");
    assert_eq!(document.document_type, "rust-mutants/candidates");
    assert_eq!(document.schema_version, 1);
    assert!(!document.tool_version.is_empty());
    assert_eq!(document.count, document.candidates.len());
    document.candidates
}

fn matching<'a>(claim: &Claim, candidates: &'a [Candidate]) -> Vec<&'a Candidate> {
    if let Some(id) = &claim.id {
        return candidates
            .iter()
            .filter(|candidate| candidate.id.starts_with(id))
            .collect();
    }
    let path = claim.path.as_deref().expect("a locator has a path");
    let item = claim.item.as_deref().expect("a locator has an item");
    let rule = claim.rule.as_deref().expect("a locator has a rule");
    let original = claim
        .original
        .as_deref()
        .expect("a locator has original bytes");
    candidates
        .iter()
        .filter(|candidate| {
            candidate.path == path
                && (candidate.item == item || candidate.item.ends_with(&format!("::{item}")))
                && candidate.rule == rule
                && candidate.original == original
        })
        .collect()
}

fn expected(claim: &Claim, current: &[Candidate], other: &[Candidate]) -> Expected {
    let mut found = matching(claim, current);
    if found.len() > 1
        && let Some(line) = claim.line
    {
        found.retain(|candidate| candidate.line == line);
    }
    if found.is_empty() {
        return if matching(claim, other).is_empty() {
            Expected::Unmatched
        } else {
            Expected::Elsewhere
        };
    }
    if claim.count.map_or(found.len() != 1, |count| {
        !usize::try_from(count).is_ok_and(|count| count == found.len())
    }) {
        return Expected::Unmatched;
    }
    let ids = found
        .iter()
        .map(|candidate| candidate.display_id.clone())
        .collect();
    match (claim.line, found.first()) {
        (Some(from), Some(first)) if first.line != from => Expected::Moved(ids, from, first.line),
        _ => Expected::Names(ids),
    }
}

fn original_is_in_source(workspace: &Path, candidate: &Candidate) {
    assert_ne!(candidate.original, candidate.replacement);
    let relative = Path::new(&candidate.path);
    assert!(
        relative
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "candidate path is workspace-relative: {}",
        candidate.path
    );
    let source = std::fs::read(workspace.join(relative)).expect("candidate source reads");
    let line = usize::try_from(candidate.line)
        .expect("line fits usize")
        .checked_sub(1)
        .expect("line is one-based");
    let column = usize::try_from(candidate.column)
        .expect("column fits usize")
        .checked_sub(1)
        .expect("column is one-based");
    let before: usize = source
        .split_inclusive(|byte| *byte == b'\n')
        .take(line)
        .map(<[u8]>::len)
        .sum();
    let at = before
        .checked_add(column)
        .expect("source offset fits usize");
    assert!(
        source
            .get(at..)
            .is_some_and(|tail| tail.starts_with(candidate.original.as_bytes())),
        "{}:{}:{} does not hold {:?}",
        candidate.path,
        candidate.line,
        candidate.column,
        candidate.original
    );
}

fn check_rows(
    workspace: &Path,
    claims: &[Claim],
    (current, other): (&[Candidate], &[Candidate]),
    report: &str,
) -> Vec<Expected> {
    let rows: Vec<&str> = report
        .lines()
        .filter(|line| {
            ["names      ", "moved      ", "elsewhere  ", "unmatched  "]
                .iter()
                .any(|prefix| line.starts_with(prefix))
        })
        .collect();
    assert_eq!(
        rows.len(),
        claims.len(),
        "one row per input claim: {report}"
    );
    claims
        .iter()
        .zip(rows)
        .map(|(claim, row)| {
            let predicted = expected(claim, current, other);
            let label = claim.label();
            match &predicted {
                Expected::Names(ids) => {
                    assert_eq!(row, format!("names      {label}  {}", ids.join(" ")));
                    for candidate in matching(claim, current) {
                        original_is_in_source(workspace, candidate);
                    }
                }
                Expected::Moved(ids, from, to) => {
                    assert_eq!(row, format!("moved      {label}  {}  is on line {to} now, not {from}; write `line = {to}`", ids.join(" ")));
                    for candidate in matching(claim, current) {
                        original_is_in_source(workspace, candidate);
                    }
                }
                Expected::Elsewhere => assert_eq!(
                    row,
                    format!("elsewhere  {label}  a file no unit of this build reads")
                ),
                Expected::Unmatched => {
                    assert!(row.starts_with(&format!("unmatched  {label}  ")), "{row}");
                }
            }
            predicted
        })
        .collect()
}

fn observed_resolution_states(repository: &Path) -> Vec<String> {
    let source = std::fs::read_to_string(repository.join("crates/rust-mutants-adapt/src/claim.rs"))
        .expect("resolution source reads");
    let syntax = njutest_devkit::lexed::file(&source).expect("resolution source parses");
    let resolution = syntax.items.iter().find_map(|item| match item {
        syn::Item::Enum(item) if item.ident == "Resolution" => Some(item),
        _ => None,
    });
    resolution
        .expect("resolution is a closed enum")
        .variants
        .iter()
        .map(|variant| variant.ident.to_string())
        .collect()
}

fn claim_text(case: Case) -> String {
    let item = if case == Case::Unmatched {
        "missing"
    } else {
        "is_even"
    };
    let line = if case == Case::Moved {
        "line = 99\n"
    } else {
        ""
    };
    format!(
        "[[mutation.expect]]\npath = \"src/dormant.rs\"\nitem = {item:?}\nrule = \"eq-to-neq\"\noriginal = \"==\"\n{line}reason = \"oracle fixture\"\n"
    )
}

#[test]
fn the_engine_the_oracle_runs_is_the_one_the_suite_built_and_leaves_that_build_current() {
    let repository = xtask::gates::workspace_root();
    let established = suite_build(&repository);
    let built_by_the_suite = njutest_devkit::reproducible::digest(&established.engine);
    let engine = engine(&repository);
    assert_eq!(
        njutest_devkit::reproducible::digest(&established.engine),
        built_by_the_suite,
        "the oracle built the engine again under an environment of its own and put that build \
         where the suite's engine was, so it ran a binary the suite never built and every test \
         after it runs that one too"
    );
    assert_eq!(engine, established.engine);
    let after = suite_build(&repository);
    assert_eq!(
        after.compiled, 0,
        "the suite's own build is no longer current after the oracle took its engine"
    );
}

#[test]
fn repository_claims_are_rederived_from_source_and_candidate_json() {
    let repository = xtask::gates::workspace_root();
    let temporary = tempfile::tempdir().expect("an owned temporary directory for the oracle");
    let claims = claims(&repository);
    assert!(!claims.is_empty(), "the repository has claims to check");
    let engine = engine(&repository);
    let current = candidates(
        (&engine, &repository),
        &repository,
        temporary.path(),
        &["--json"],
    );
    let (code, report) = listing(
        (&engine, &repository),
        &repository,
        temporary.path(),
        &["--claims"],
    );
    assert_eq!(code, 0, "repository claims are accepted: {report}");
    let predicted = check_rows(&repository, &claims, (&current, &[]), &report);
    assert!(
        predicted
            .iter()
            .all(|one| matches!(one, Expected::Names(_))),
        "every current repository claim resolves to a compiled source candidate"
    );
}

#[test]
fn every_claim_resolution_is_observed_across_real_build_inputs() {
    let repository = xtask::gates::workspace_root();
    assert_eq!(
        Case::ALL.map(|case| format!("{case:?}")).to_vec(),
        observed_resolution_states(&repository),
        "every production resolution variant has one real input case"
    );
    let fixture = Fixture::copy("fixture-simple");
    let manifest = fixture.root().join("Cargo.toml");
    let mut written = std::fs::read_to_string(&manifest).expect("fixture manifest reads");
    written.push_str("\n[features]\ndormant = []\n");
    std::fs::write(&manifest, written).expect("fixture feature writes");
    let library = fixture.root().join("src/lib.rs");
    let mut written = std::fs::read_to_string(&library).expect("fixture library reads");
    written.push_str("\n#[cfg(feature = \"dormant\")]\nmod dormant;\n");
    std::fs::write(&library, written).expect("fixture module writes");
    std::fs::write(
        fixture.root().join("src/dormant.rs"),
        "pub fn is_even(n: i32) -> bool { n % 2 == 0 }\n",
    )
    .expect("fixture source writes");
    let engine = engine(&repository);
    let enabled = candidates(
        (&engine, &repository),
        fixture.root(),
        fixture.temp(),
        &["--json", "--features", "dormant"],
    );
    let disabled = candidates(
        (&engine, &repository),
        fixture.root(),
        fixture.temp(),
        &["--json"],
    );
    for case in Case::ALL {
        std::fs::write(fixture.root().join(".rust-mutants.toml"), claim_text(case))
            .expect("fixture claim writes");
        let selected = if case == Case::Uncompiled {
            &disabled
        } else {
            &enabled
        };
        let other = if case == Case::Uncompiled {
            &enabled
        } else {
            &disabled
        };
        let mut flags = vec!["--claims"];
        if case != Case::Uncompiled {
            flags.extend(["--features", "dormant"]);
        }
        let (code, report) = listing(
            (&engine, &repository),
            fixture.root(),
            fixture.temp(),
            &flags,
        );
        let expected_code = match case {
            Case::Names | Case::Uncompiled => 0,
            Case::Moved | Case::Unmatched => 1,
        };
        assert_eq!(code, expected_code, "the claim listing completed: {report}");
        let rows = check_rows(
            fixture.root(),
            &claims(fixture.root()),
            (selected, other),
            &report,
        );
        assert_eq!(rows.len(), 1);
        assert!(
            matches!(
                (case, rows.first()),
                (Case::Names, Some(Expected::Names(_)))
                    | (Case::Moved, Some(Expected::Moved(_, _, _)))
                    | (Case::Uncompiled, Some(Expected::Elsewhere))
                    | (Case::Unmatched, Some(Expected::Unmatched))
            ),
            "the case chooses its observed resolution: {report}"
        );
    }
}
