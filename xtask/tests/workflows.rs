// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That the workflows say what they run in, and read only what the schemas declare.

#![expect(
    clippy::panic,
    reason = "the helper that reads the repository's own workflows is not itself a test: a \
              workflow that cannot be read leaves nothing to assert"
)]

use std::path::{Path, PathBuf};

fn workflows() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/workflows");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|error| panic!("entry under {}: {error}", dir.display())))
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|one| one == "yml"))
        .collect();
    found.sort();
    found
}

/// The composite actions the workflows use, one `action.yml` each.
fn actions() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/actions");
    let mut found: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{}: {error}", dir.display()))
        .map(|entry| entry.unwrap_or_else(|error| panic!("entry under {}: {error}", dir.display())))
        .map(|entry| entry.path().join("action.yml"))
        .collect();
    found.sort();
    found
}

/// The jobs of one workflow, as name and body.
fn jobs(source: &str) -> Vec<(String, String)> {
    let mut found: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        let named = line
            .strip_prefix("  ")
            .filter(|rest| !rest.starts_with(' ') && !rest.starts_with('#'))
            .and_then(|rest| rest.strip_suffix(':'))
            .filter(|name| !name.contains(' '));
        match named {
            Some(name) => found.push((name.to_owned(), String::new())),
            None => {
                if let Some(last) = found.last_mut() {
                    last.1.push_str(line);
                    last.1.push('\n');
                }
            }
        }
    }
    found
}

/// What a workflow says its steps run in when a step says nothing: `bash`, which GitHub runs as `bash -e -o pipefail` on every runner.
const DEFAULT_SHELL: &str = "defaults:\n  run:\n    shell: bash\n";

#[test]
fn every_step_runs_in_a_shell_that_stops_at_the_first_failure_even_inside_a_pipe() {
    let mut loose = Vec::new();
    for path in workflows() {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let file = path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_else(|| panic!("a workflow file name is not UTF-8: {}", path.display()));
        if !source.contains(DEFAULT_SHELL) {
            loose.push(format!("{file}: no top-level `{DEFAULT_SHELL}`"));
        }
    }
    for path in workflows().into_iter().chain(actions()) {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for (at, line) in source.lines().enumerate() {
            let other = line
                .trim_start()
                .trim_start_matches("- ")
                .strip_prefix("shell:")
                .map(str::trim)
                .filter(|shell| *shell != "bash");
            if let Some(shell) = other {
                loose.push(format!(
                    "{}:{}: `shell: {shell}`",
                    path.display(),
                    at.saturating_add(1)
                ));
            }
        }
    }

    assert!(
        loose.is_empty(),
        "a step with no shell of its own takes `bash -e {{0}}` on Linux and macOS, where a \
         command piped into `tee` has its status replaced by tee's, and PowerShell on \
         Windows, where a failing native command does not stop the script. The first let \
         `njutest verify` exit with any code at all while the soundness job read on; the \
         second reported a failing test forty-five minutes later as a cancelled job. \
         Declare `shell: bash` as the workflow's default, which GitHub runs with \
         `-o pipefail` on every runner, and override it with nothing else. {loose:?}"
    );
}

#[test]
fn the_real_kani_job_installs_one_exact_locked_version() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/workflows/ci.yml");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let body = jobs(&source)
        .into_iter()
        .find_map(|(name, body)| (name == "kani-verified").then_some(body))
        .unwrap_or_else(|| panic!("{} has no kani-verified job", path.display()));
    assert!(
        body.contains("cargo install --locked kani-verifier --version '=0.68.0'"),
        "the proof job must make Cargo's exact-version intent machine-readable"
    );
}

/// Every tool `mise.toml` pins, split by the key that says who installs it.
///
/// A `cargo:` prefix is a crate and carries its exact version to `taiki-e/install-action`; a bare name is a tool mise fetches, and carries no version because mise reads the same pin this does.
/// Three are named by no job: the toolchain comes from `rust-toolchain.toml`, a runner runs no hooks, and the compiler wrapper is installed by the setup action itself because every job inherits it from `[env]`.
fn pinned() -> (Vec<String>, Vec<String>) {
    const NAMED_BY_NO_JOB: [&str; 3] = ["rust", "lefthook", "sccache"];

    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("mise.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let table = text
        .parse::<toml::Table>()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let Some(toml::Value::Table(tools)) = table.get("tools") else {
        panic!("mise.toml declares the tools it pins")
    };
    let mut crates = Vec::new();
    let mut fetched = Vec::new();
    for (name, version) in tools {
        let name = name.trim_matches('"');
        let Some(version) = version.as_str() else {
            panic!("{name} is pinned to one exact version")
        };
        if let Some(crate_name) = name.strip_prefix("cargo:") {
            crates.push(format!("{crate_name}@{version}"));
        } else if !NAMED_BY_NO_JOB.contains(&name) {
            fetched.push(name.to_owned());
        }
    }
    crates.sort();
    fetched.sort();
    (crates, fetched)
}

/// Every tool named to mise by a workflow, once each.
fn mise_installed() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for text in workflow_sources() {
        for line in text.lines() {
            let Some((_, listed)) = line.split_once("mise-tools:") else {
                continue;
            };
            found.extend(listed.split_whitespace().map(ToOwned::to_owned));
        }
    }
    found.sort();
    found.dedup();
    found
}

/// Every `.yml` under `.github`, read once.
fn workflow_sources() -> Vec<String> {
    let mut found = Vec::new();
    let mut pending = vec![
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join(".github"),
    ];
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
            if path.extension().is_none_or(|kind| kind != "yml") {
                continue;
            }
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
            found.push(text);
        }
    }
    found
}

/// Every crate the pipeline installs through the pinned installer, once each.
fn installed() -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for text in workflow_sources() {
        for line in text.lines() {
            if line.contains("mise-tools:") {
                continue;
            }
            let Some((_, listed)) = line.split_once("tools:") else {
                continue;
            };
            found.extend(
                listed
                    .split(',')
                    .map(str::trim)
                    .filter(|tool| tool.contains('@'))
                    .map(ToOwned::to_owned),
            );
        }
    }
    found.sort();
    found.dedup();
    found
}

#[test]
fn executable_tools_use_the_commit_pinned_installer_and_exact_versions() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/workflows/ci.yml");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let (crates, fetched) = pinned();
    let installed = installed();
    let by_mise = mise_installed();
    assert!(crates.len() > 4, "mise pins the crates: {crates:?}");
    assert!(
        installed.len() > 4,
        "the pipeline installs them: {installed:?}"
    );
    let adrift: Vec<&String> = installed
        .iter()
        .filter(|tool| !crates.contains(tool))
        .collect();
    assert!(
        adrift.is_empty(),
        "the pipeline hands install-action something mise.toml does not pin as a crate. \
         Either the version drifted, or the tool is pinned without a `cargo:` prefix and so \
         is one mise fetches rather than a crate — install-action would look for a crates.io \
         name it may not have, at a version it carries its own list of. Name it under \
         `mise-tools:` instead: {adrift:?} against {crates:?}"
    );
    let unpinned: Vec<&String> = by_mise
        .iter()
        .filter(|tool| !fetched.contains(tool))
        .collect();
    assert!(
        unpinned.is_empty(),
        "the pipeline asks mise for a tool mise.toml does not pin, so the runner resolves \
         a version nothing in this tree names: {unpinned:?} against {fetched:?}"
    );
    for unverified in ["curl ", "wget ", "Invoke-WebRequest", "| tar"] {
        assert!(
            !source.contains(unverified),
            "CI downloads an executable without an independently pinned digest ({unverified:?}): {source}"
        );
    }
}

/// Every program `mise.toml`'s `[env]` names, which activating mise puts into a job whether or not the runner has it.
fn environment_programs() -> Vec<(String, String)> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("mise.toml");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let table = text
        .parse::<toml::Table>()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let Some(toml::Value::Table(environment)) = table.get("env") else {
        return Vec::new();
    };
    let Some(toml::Value::Table(tools)) = table.get("tools") else {
        panic!("mise.toml declares the tools it pins")
    };
    let pinned: Vec<String> = tools
        .keys()
        .map(|name| name.trim_matches('"').to_owned())
        .collect();
    environment
        .iter()
        .filter_map(|(variable, value)| {
            let named = value.as_str()?;
            pinned
                .iter()
                .find(|one| {
                    named
                        .split(|c: char| !c.is_ascii_alphanumeric() && c != '-' && c != '_')
                        .any(|word| word == one.as_str())
                })
                .map(|program| (variable.clone(), program.clone()))
        })
        .collect()
}

#[test]
fn a_program_the_activated_environment_names_is_one_every_job_has() {
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/actions/setup-rust/action.yml");
    let source = std::fs::read_to_string(&setup)
        .unwrap_or_else(|error| panic!("{}: {error}", setup.display()));
    assert!(
        source.contains("jdx/mise-action"),
        "this law is about what activating mise brings with it"
    );
    let unaccounted: Vec<String> = environment_programs()
        .into_iter()
        .filter(|(variable, program)| {
            let installed = source.contains(&format!("mise install {program}"));
            let cleared = source.contains(&format!("{variable}=\" >>"));
            !installed && !cleared
        })
        .map(|(variable, program)| format!("{variable}={program}"))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "the setup action activates mise, so every job inherits `[env]` from mise.toml. \
         A variable naming a program the runner does not have fails every cargo invocation \
         in that job, including one that only reads metadata, with `could not execute \
         process ... (never executed)`. The action that activates the environment either \
         installs the program or clears the variable, and says which: {unaccounted:?}"
    );
}

#[test]
fn what_the_setup_action_downloads_is_restored_before_it_is_fetched() {
    let setup = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(".github/actions/setup-rust/action.yml");
    let source = std::fs::read_to_string(&setup)
        .unwrap_or_else(|error| panic!("{}: {error}", setup.display()));
    let activation = source
        .find("jdx/mise-action")
        .expect("this law is about the step that fetches mise");
    let before = source.get(..activation).unwrap_or_default();
    let restored = before
        .rfind("actions/cache@")
        .and_then(|at| before.get(at..))
        .is_some_and(|step| step.contains(".local/share/mise") && step.contains("mise.toml"));
    assert!(
        restored,
        "mise-action saves its cache only after it installs, and this action installs \
         nothing through it, so no job ever found one: every job fetched mise from GitHub \
         releases, and one HTTP 500 there cost the whole pipeline. The mise directory is \
         restored by a cache keyed on mise.toml before the step that would fetch it"
    );
}

/// Whether `pointer` names something the report schema declares, following `$ref`, `oneOf`, and arrays.
fn resolves(schema: &serde_json::Value, node: &serde_json::Value, pointer: &[&str]) -> bool {
    let Some((head, rest)) = pointer.split_first() else {
        return true;
    };
    if let Some(reference) = node.get("$ref").and_then(serde_json::Value::as_str) {
        let Some(name) = reference.strip_prefix("#/$defs/") else {
            return false;
        };
        let Some(target) = schema.get("$defs").and_then(|defs| defs.get(name)) else {
            return false;
        };
        return resolves(schema, target, pointer);
    }
    if let Some(branches) = node.get("oneOf").and_then(serde_json::Value::as_array) {
        return branches
            .iter()
            .any(|branch| resolves(schema, branch, pointer));
    }
    if *head == "[]" {
        return node
            .get("items")
            .is_some_and(|items| resolves(schema, items, rest));
    }
    node.get("properties")
        .and_then(|properties| properties.get(head))
        .is_some_and(|next| resolves(schema, next, rest))
}

/// Every `.a.b[].c` a jq filter reads, as pointer segments.
fn jq_pointers(filter: &str) -> Vec<Vec<String>> {
    let mut found = Vec::new();
    let mut characters = filter.chars().peekable();
    let mut previous = ' ';
    while let Some(character) = characters.next() {
        if character != '.' || previous.is_alphanumeric() || previous == '_' {
            previous = character;
            continue;
        }
        let mut segments: Vec<String> = Vec::new();
        loop {
            let mut name = String::new();
            while characters
                .peek()
                .is_some_and(|next| next.is_alphanumeric() || *next == '_')
            {
                if let Some(next) = characters.next() {
                    name.push(next);
                }
            }
            if name.is_empty() {
                break;
            }
            segments.push(name);
            if characters.next_if_eq(&'[').is_some() && characters.next_if_eq(&']').is_some() {
                segments.push("[]".to_owned());
            }
            if characters.next_if_eq(&'.').is_none() {
                break;
            }
        }
        previous = ' ';
        if !segments.is_empty() {
            found.push(segments);
        }
    }
    found
}

#[test]
fn a_workflow_that_reads_a_report_names_paths_the_schema_declares() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(root.join("schema/njutest-assurance-report-v1.json"))
            .unwrap_or_else(|error| panic!("the report schema: {error}")),
    )
    .unwrap_or_else(|error| panic!("the report schema is a document: {error}"));
    let source = std::fs::read_to_string(root.join(".github/workflows/ci.yml"))
        .unwrap_or_else(|error| panic!("ci.yml: {error}"));

    let Some(step) = source.split("$RUNNER_TEMP/interpreted.out").nth(2) else {
        panic!("the soundness job no longer reads the report it wrote");
    };
    let filter = step
        .split_once("jq --exit-status")
        .unwrap_or_else(|| panic!("the soundness job no longer asks jq about the report"))
        .1;
    let filter = filter
        .split_once("' \"")
        .unwrap_or_else(|| panic!("the jq filter is not the quoted form this reads"))
        .0;

    let mut unresolved = Vec::new();
    let mut checked = 0_usize;
    for pointer in jq_pointers(filter) {
        let borrowed: Vec<&str> = pointer.iter().map(String::as_str).collect();
        checked = checked.saturating_add(1);
        if !resolves(&schema, &schema, &borrowed) {
            unresolved.push(pointer.join("."));
        }
    }
    assert!(checked > 0, "no path was read out of {filter:?}");
    assert!(
        unresolved.is_empty(),
        "the soundness job asks jq for a path the report schema does not declare, so the \
         assertion is false whatever the run did and the job fails for a reason that is not \
         about soundness. The report is enveloped: `document_type` beside `report`, and the \
         accounting is per build and per part: {unresolved:?}"
    );
}

/// Every page a reader copies commands and workflows from: the book and the README.
fn pages() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut found: Vec<PathBuf> = walkdir::WalkDir::new(root.join("docs"))
        .into_iter()
        .map(|entry| entry.unwrap_or_else(|error| panic!("docs: {error}")))
        .map(walkdir::DirEntry::into_path)
        .filter(|path| path.extension().is_some_and(|extension| extension == "md"))
        .collect();
    found.push(root.join("README.md"));
    found.sort();
    found
}

/// The shell text a workflow, an action, a task or a script runs: the whole file, since a line that is not shell holds no `||` outside the expressions [`shell_text`] removes.
fn shell_sources() -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(root.join("scripts"))
        .unwrap_or_else(|error| panic!("scripts: {error}"))
        .map(|entry| {
            entry
                .unwrap_or_else(|error| panic!("scripts: {error}"))
                .path()
        })
        .filter(|path| path.extension().is_some_and(|extension| extension == "sh"))
        .collect();
    scripts.sort();
    let mut found = Vec::new();
    for path in workflows()
        .into_iter()
        .chain(actions())
        .chain([root.join("mise.toml")])
        .chain(scripts)
    {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        found.push((path, source));
    }
    for path in pages() {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let mut inside = false;
        let mut examples = String::new();
        for line in source.lines() {
            if line.trim_start().starts_with("```") {
                inside = line.trim_start().starts_with("```yaml");
                examples.push('\n');
                continue;
            }
            if inside {
                examples.push_str(line);
            }
            examples.push('\n');
        }
        found.push((path, examples));
    }
    found
}

/// `source` with what is not a shell command blanked, line lengths kept: comments, quoted strings, `${{ … }}` expressions, and `[[ … ]]` tests.
fn shell_text(source: &str) -> String {
    let characters: Vec<char> = source.chars().collect();
    let mut kept = String::with_capacity(source.len());
    let mut at = 0_usize;
    while let Some(&character) = characters.get(at) {
        let rest: String = characters.iter().skip(at).take(3).collect();
        if rest == "\"\"\"" {
            kept.push_str(&rest);
            at = at.saturating_add(3);
            continue;
        }
        let closing = if rest.starts_with("${{") {
            Some("}}")
        } else if rest.starts_with("[[") {
            Some("]]")
        } else if character == '\'' || character == '"' {
            Some(if character == '\'' { "'" } else { "\"" })
        } else if character == '#'
            && (at == 0
                || characters
                    .get(at.saturating_sub(1))
                    .is_some_and(|before| before.is_whitespace()))
        {
            Some("\n")
        } else {
            None
        };
        let Some(closing) = closing else {
            kept.push(character);
            at = at.saturating_add(1);
            continue;
        };
        let opened = if closing == "}}" || closing == "]]" {
            2
        } else {
            1
        };
        let mut end = at.saturating_add(opened);
        while end < characters.len() {
            let here: String = characters.iter().skip(end).take(closing.len()).collect();
            if here == closing {
                break;
            }
            end = end.saturating_add(1);
        }
        let through = if closing == "\n" {
            end
        } else {
            end.saturating_add(closing.len())
        };
        for blanked in characters.iter().take(through).skip(at) {
            kept.push(if *blanked == '\n' { '\n' } else { ' ' });
        }
        at = through;
    }
    kept
}

/// Whether the command after a `||` still ends non-zero, or records the failure for a later decision, rather than turning it into a success.
fn answers_the_failure(right: &str, left_is_a_test: bool) -> bool {
    let right = right.trim_start();
    if let Some(group) = right.strip_prefix('{') {
        let body = group.split('}').next().unwrap_or("");
        return body
            .split([';', '\n'])
            .map(str::trim)
            .rfind(|command| !command.is_empty())
            .is_some_and(ends_non_zero);
    }
    let command: String = right
        .chars()
        .take_while(|character| !matches!(character, ';' | '\n' | '&' | '|' | ')'))
        .collect();
    let command = command.trim();
    command.starts_with("case ")
        || ends_non_zero(command)
        || command.split_once('=').is_some_and(|(name, _value)| {
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
        || (left_is_a_test && matches!(command, "continue" | "break"))
}

/// Whether `command` ends its shell non-zero.
fn ends_non_zero(command: &str) -> bool {
    command == "false"
        || ["exit ", "return "].iter().any(|keyword| {
            command
                .strip_prefix(keyword)
                .is_some_and(|code| code.trim() != "0" && !code.trim().is_empty())
        })
}

/// Every `||` in `text` whose right side turns the left side's failure into a success, and every `set +e`, by line.
fn swallowed(text: &str) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, line) in text.lines().enumerate() {
        if line
            .split_whitespace()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|pair| pair == ["set", "+e"])
            || line.contains("continue-on-error: true")
        {
            found.push(at.saturating_add(1));
        }
    }
    let mut from = 0_usize;
    while let Some(offset) = text.get(from..).and_then(|rest| rest.find("||")) {
        let at = from.saturating_add(offset);
        let before = text.get(..at).unwrap_or("");
        let left_start = before
            .rfind(['\n', ';', '(', '{', '&', '|'])
            .map_or(0, |boundary| boundary.saturating_add(1));
        let left = before.get(left_start..).unwrap_or("").trim_start();
        let left_is_a_test = ["[ ", "test ", "if ", "elif ", "while ", "until "]
            .iter()
            .any(|opening| left.starts_with(opening))
            || left.trim().is_empty();
        let right = text.get(at.saturating_add(2)..).unwrap_or("");
        let right = right.trim_start_matches([' ', '\t', '\\', '\n']);
        if !left_is_a_test && !answers_the_failure(right, left_is_a_test) {
            found.push(before.matches('\n').count().saturating_add(1));
        }
        from = at.saturating_add(2);
    }
    found.sort_unstable();
    found.dedup();
    found
}

#[test]
fn no_command_turns_its_own_failure_into_a_success() {
    let mut swallowing = Vec::new();
    for (path, source) in shell_sources() {
        for line in swallowed(&shell_text(&source)) {
            swallowing.push(format!("{}:{line}", path.display()));
        }
    }
    assert!(
        swallowing.is_empty(),
        "a command that cannot fail says nothing when it goes wrong: the dogfood parts passed a \
         flag the engine does not have, every part refused to run, and `|| true` reported each \
         one as a part that measured. After `||`, end non-zero, record the status for a later \
         decision, or accept the exit codes that are answers by name in a `case`; `set +e` and \
         `continue-on-error` are refused outright. {swallowing:#?}"
    );
}

#[test]
fn the_swallowing_rule_tells_a_refusal_from_a_decision() {
    for swallowing in [
        "cmd || true",
        "cmd ||true",
        "cmd || :",
        "cmd || echo failed",
        "cmd || exit 0",
        "a || b || true",
        "set +e",
        "continue-on-error: true",
    ] {
        assert!(
            !swallowed(&shell_text(swallowing)).is_empty(),
            "{swallowing}"
        );
    }
    for deciding in [
        "cmd || exit 1",
        "cmd || { echo why; exit 1; }",
        "cmd || case \"$?\" in 1) ;; *) exit 1 ;; esac",
        "cmd || status=1",
        "[ -f x ] || continue",
        "if a || b; then c; fi",
        "while read -r line || [[ -n \"${line}\" ]]; do :; done",
        "ref: ${{ inputs.tag || github.ref }}",
        "echo 'a || true'",
        "# a || true",
    ] {
        assert!(swallowed(&shell_text(deciding)).is_empty(), "{deciding}");
    }
}

/// Every long option a help golden lists for `program`'s `command`, with whether it takes a value.
fn options(program: &str, command: Option<&str>) -> Option<Vec<(String, bool)>> {
    let crate_dir = match program {
        "njutest" => "crates/njutest",
        _ => "crates/rust-mutants-cli",
    };
    if command.is_some_and(|command| {
        !command.chars().all(|character| {
            character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-'
        })
    }) {
        return None;
    }
    let name = command.map_or_else(
        || "help.golden".to_owned(),
        |command| format!("help-{command}.golden"),
    );
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_dir)
        .join("tests/testdata")
        .join(name);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(missing) if missing.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => panic!("{}: {error}", path.display()),
    };
    let mut found = Vec::new();
    for line in text.lines() {
        let mut words = line.split_whitespace().peekable();
        while let Some(word) = words.next() {
            let written = word.trim_end_matches(',');
            let (flag, optional) = written
                .split_once("[=")
                .map_or((written, false), |(flag, _value)| (flag, true));
            if flag.starts_with("--") && flag.len() > 2 {
                let valued = optional || words.peek().is_some_and(|next| next.starts_with('<'));
                found.push((flag.to_owned(), valued));
            }
        }
    }
    Some(found)
}

/// The commands a text runs, one logical line each: a line continued by `\`, or a folded YAML block, is one line.
fn logical_lines(source: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut folded: Option<usize> = None;
    for line in source.lines() {
        let indent = line.len().saturating_sub(line.trim_start().len());
        if let Some(depth) = folded {
            if !line.trim().is_empty() && indent >= depth {
                current.push(' ');
                current.push_str(line.trim());
                continue;
            }
            lines.push(std::mem::take(&mut current));
            folded = None;
        }
        let trimmed = line.trim_end();
        if trimmed.ends_with(": >-") || trimmed.ends_with(": >") {
            folded = Some(indent.saturating_add(1));
            continue;
        }
        if let Some(continued) = trimmed.strip_suffix('\\') {
            current.push_str(continued);
            current.push(' ');
            continue;
        }
        current.push_str(trimmed);
        lines.push(std::mem::take(&mut current));
    }
    lines.push(current);
    lines
}

/// The programs this repository ships.
const PROGRAMS: [&str; 2] = ["njutest", "rust-mutants"];

/// Whether `word` names a subcommand of `program`'s `command`, whose own flags no golden lists.
fn nests(program: &str, command: Option<&str>, word: &str) -> bool {
    let crate_dir = match program {
        "njutest" => "crates/njutest",
        _ => "crates/rust-mutants-cli",
    };
    let Some(command) = command else {
        return false;
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(crate_dir)
        .join("tests/testdata")
        .join(format!("help-{command}.golden"));
    std::fs::read_to_string(path).is_ok_and(|text| {
        text.split_once("Commands:")
            .is_some_and(|(_before, listed)| {
                listed
                    .lines()
                    .skip(1)
                    .take_while(|line| !line.trim().is_empty())
                    .any(|line| line.split_whitespace().next() == Some(word))
            })
    })
}

/// Every flag `line` passes to one of this repository's programs, as program, command, and the flag as written.
fn invocations(line: &str) -> Vec<(String, Option<String>, String)> {
    let spaced = line.replace(['`', '(', ')'], " ` ");
    let words: Vec<&str> = spaced.split_whitespace().collect();
    let mut found = Vec::new();
    let mut at = 0;
    while let Some(word) = words.get(at) {
        at = at.saturating_add(1);
        let Some(program) = PROGRAMS.iter().copied().find(|program| {
            *word == *program
                || ["/release/", "/debug/", "/bin/"]
                    .iter()
                    .any(|built| word.ends_with(&format!("{built}{program}")))
        }) else {
            continue;
        };
        let installed = at >= 2
            && words
                .get(at.saturating_sub(2))
                .is_some_and(|before| *before == "install");
        if installed {
            continue;
        }
        let command = words
            .get(at)
            .filter(|next| options(program, Some(next)).is_some())
            .map(|next| (*next).to_owned());
        if command.is_some() {
            at = at.saturating_add(1);
        }
        let nested = words.get(at).is_some_and(|next| {
            !next.starts_with('-')
                && next.chars().all(|c| c.is_ascii_lowercase() || c == '-')
                && command.is_some()
                && nests(program, command.as_deref(), next)
        });
        if nested {
            continue;
        }
        while let Some(word) = words.get(at) {
            if ["`", "|", "||", "&&", ";", ">", "2>&1"].contains(word) {
                break;
            }
            let flag = word.trim_end_matches([',', '.', ':', ';', '"', '\'']);
            if flag.starts_with("--") && flag.len() > 2 {
                found.push((program.to_owned(), command.clone(), flag.to_owned()));
            }
            at = at.saturating_add(1);
        }
    }
    found
}

#[test]
fn every_flag_a_workflow_or_a_page_passes_is_one_the_command_has() {
    let mut unknown = Vec::new();
    for path in workflows().into_iter().chain(actions()).chain(pages()) {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        for line in logical_lines(&source) {
            for (program, command, written) in invocations(&line) {
                let (flag, given) = written
                    .split_once('=')
                    .map_or((written.as_str(), false), |(flag, _value)| (flag, true));
                let mut known = options(&program, None)
                    .unwrap_or_else(|| panic!("{program} has no top-level help golden"));
                if let Some(of_command) = command
                    .as_deref()
                    .and_then(|command| options(&program, Some(command)))
                {
                    known.extend(of_command);
                }
                let accepted = known
                    .iter()
                    .any(|(name, valued)| name == flag && (*valued || !given));
                if !accepted {
                    unknown.push(format!(
                        "{}: `{program} {} {written}`",
                        path.display(),
                        command.as_deref().unwrap_or("")
                    ));
                }
            }
        }
    }
    assert!(
        unknown.is_empty(),
        "a flag the command does not have is refused when it runs, and a page that shows one \
         teaches a reader to be refused; the help goldens are what the command has. {unknown:#?}"
    );
}
#[test]
fn code_only_one_platform_compiles_is_linted_on_that_platform() {
    let path = workflows()
        .into_iter()
        .find(|path| path.ends_with("ci.yml"))
        .unwrap_or_else(|| panic!("ci.yml is one of the workflows"));
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let test = jobs(&source)
        .into_iter()
        .find_map(|(name, body)| (name == "test").then_some(body))
        .unwrap_or_else(|| panic!("ci.yml has the test matrix"));
    let linted = test.split("\n      - ").any(|step| {
        step.contains("cargo clippy")
            && step.contains("--all-targets")
            && step.contains("--all-features")
            && step.contains("-D warnings")
            && step
                .lines()
                .find_map(|line| line.trim_start().strip_prefix("if:"))
                .is_none_or(|condition| condition.trim() == "runner.os != 'Linux'")
    });
    assert!(
        linted,
        "clippy ran only on Linux, so every `cfg(windows)` and `cfg(target_os = \"macos\")` \
         line in the tree was compiled on its own platform and linted nowhere; the matrix \
         lints on every platform the lint job does not stand on"
    );
}
