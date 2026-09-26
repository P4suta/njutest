// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every claim the repository's configuration makes names what it says, asked of the engine's own locator on every push.

use std::path::Path;

use crate::gates::GateError;

/// The fixture the planted claims are written against.
const FIXTURE: &str = "fixtures/fixture-simple";

/// A claim of the fixture that names nothing, which the gate must refuse.
const ROTTEN: (&str, &str, &str) = ("max", "eq-to-neq", "==");

/// A claim of the fixture that names its one mutation, which the gate must not refuse.
const SOUND: (&str, &str, &str) = ("is_even", "eq-to-neq", "==");

/// A claim of the fixture that names its one mutation on a line it is not on, which the gate must refuse.
const MOVED: (&str, &str, &str) = ("is_even", "rem-to-mul", "%");

/// The line the moved claim holds, which the fixture's source does not reach.
const MOVED_FROM: u32 = 99;

/// The kinds of line on which `rust-mutants list --claims` names a claim it refuses.
const REFUSED: [&str; 2] = ["unmatched ", "moved "];

/// What `rust-mutants list --claims` said of one workspace.
struct Listed {
    refused: bool,
    said: String,
}

impl Listed {
    /// How many claims it said on a line of the kind `kind`.
    fn counted(&self, kind: &str) -> usize {
        self.said
            .lines()
            .filter(|line| line.starts_with(kind))
            .count()
    }

    /// Whether it named the claim written for `(item, rule, original)` of `src/lib.rs` on a line of the kind `kind`.
    fn named(&self, kind: &str, (item, rule, original): (&str, &str, &str)) -> bool {
        let claim = format!("src/lib.rs {item} {rule} {original:?}");
        self.said
            .lines()
            .any(|line| line.starts_with(kind) && line.contains(&claim))
    }
}

/// The configuration text of one claim of the fixture.
fn planted((item, rule, original): (&str, &str, &str)) -> String {
    format!(
        "[[mutation.expect]]\npath = \"src/lib.rs\"\nitem = {item:?}\nrule = {rule:?}\n\
         original = {original:?}\noutcome = \"survived\"\nreason = \"planted by the claims gate\"\n"
    )
}

/// Asks the engine built from `repository` to resolve every claim of the workspace at `workspace`, putting every temporary artifact under `temporary`.
fn listed(repository: &Path, workspace: &Path, temporary: &Path) -> Result<Listed, GateError> {
    let asked = std::process::Command::new("cargo")
        .args([
            "run",
            "--locked",
            "--quiet",
            "--package",
            "rust-mutants-cli",
            "--bin",
            "rust-mutants",
            "--",
            "list",
            "--offline",
            "--locked",
            "--claims",
            "--root",
        ])
        .arg(workspace)
        .current_dir(repository)
        .envs(super::TEMPORARY_VARIABLES.map(|name| (name, temporary)))
        .output()
        .map_err(|error| GateError(format!("claims: cargo run could not start: {error}")))?;
    let said = String::from_utf8(asked.stdout).map_err(|_not_text| {
        GateError("claims: rust-mutants list --claims printed bytes that are not text".to_owned())
    })?;
    let refused = match asked.status.code() {
        Some(0) => false,
        Some(1) => true,
        _ => {
            let complaint = match String::from_utf8(asked.stderr) {
                Ok(said) => said,
                Err(_not_text) => "its diagnostics are not text".to_owned(),
            };
            return Err(GateError(format!(
                "claims: rust-mutants list --claims on {} ended {}, so no claim was resolved:\n{}",
                workspace.display(),
                asked.status,
                complaint.trim()
            )));
        }
    };
    if refused
        != said
            .lines()
            .any(|line| REFUSED.iter().any(|kind| line.starts_with(kind)))
    {
        return Err(GateError(format!(
            "claims: rust-mutants list --claims on {} exited {} and printed a list that says \
             otherwise:\n{said}",
            workspace.display(),
            u8::from(refused)
        )));
    }
    Ok(Listed { refused, said })
}

/// Every claim of the repository's configuration names as many mutations as it says, once the gate has refused a planted claim that names nothing and passed one that names its mutation.
///
/// # Errors
/// The planted claims are not told apart, the engine cannot be built or run, or a claim of the repository names nothing or not as many as it says.
pub fn claims(root: &Path) -> Result<String, GateError> {
    let scratch = tempfile::tempdir()
        .map_err(|error| GateError(format!("claims: a scratch directory: {error}")))?;
    let fixture = scratch.path().join("fixture");
    crate::repository::copy(&root.join(FIXTURE), &fixture)?;
    let configuration = fixture.join(".rust-mutants.toml");
    std::fs::write(
        &configuration,
        format!(
            "{}\n{}\n{}line = {MOVED_FROM}\n",
            planted(ROTTEN),
            planted(SOUND),
            planted(MOVED)
        ),
    )
    .map_err(|error| GateError(format!("claims: {}: {error}", configuration.display())))?;
    let control = listed(root, &fixture, scratch.path())?;
    if !(control.refused
        && control.named("unmatched ", ROTTEN)
        && control.named("moved ", MOVED)
        && control.named("names ", SOUND))
    {
        return Err(GateError(format!(
            "claims: of three claims planted in {FIXTURE}, the one naming nothing or the one \
             holding a line its mutation is not on is not refused, or the one naming its \
             mutation is, so the gate's silence about the repository would not be evidence:\n{}",
            control.said
        )));
    }
    let repository = listed(root, root, scratch.path())?;
    if repository.refused {
        return Err(GateError(format!(
            "claims: a claim of .rust-mutants.toml names nothing, not as many as it says, or a \
             line its mutation left; write the line it names, re-point it with `rust-mutants \
             explain`, or drop it where its code is gone:\n{}",
            repository.said.trim_end()
        )));
    }
    Ok(format!(
        "claims: {} of .rust-mutants.toml name what they say and {} a file only another build \
         reads, after planted claims naming nothing and a line their mutation left were refused",
        repository.counted("names "),
        repository.counted("elsewhere ")
    ))
}
