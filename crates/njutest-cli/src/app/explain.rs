// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest explain`: everything a run recorded about one mutant.

use std::io::Write;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Explain as Arguments};
use crate::report::lines::escape;

/// Prints what a run established about one mutant.
///
/// # Errors
/// Returns the output stream's write failure.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> std::io::Result<u8> {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };
    let report = match runs::report(&run) {
        Ok(report) => report,
        Err(error) => {
            super::complain(stderr, &error, error.code())?;
            return Ok(EXIT_ERROR);
        }
    };

    let conclusion = match report.conclusion() {
        Ok(conclusion) => conclusion,
        Err(error) => {
            super::complain(stderr, &error, crate::error::REPORT_UNSOUND)?;
            return Ok(EXIT_ERROR);
        }
    };
    let every: Vec<&crate::report::ProjectedMutant> = conclusion.mutants.iter().collect();
    let matching = crate::naming::matching(&every, &arguments.mutant);
    let Some(mutant) = one(&matching, &arguments.mutant, stderr)? else {
        return Ok(EXIT_ERROR);
    };

    write_explanation(
        stdout,
        &Explanation {
            root,
            run: run.id(),
            conclusion: &conclusion,
            mutant,
        },
    )?;
    Ok(EXIT_ASSURED)
}

struct Explanation<'a> {
    root: &'a std::path::Path,
    run: &'a rust_mutants::id::StoredRunId,
    conclusion: &'a crate::report::Conclusion,
    mutant: &'a crate::report::ProjectedMutant,
}

fn write_explanation(stdout: &mut dyn Write, explanation: &Explanation<'_>) -> std::io::Result<()> {
    let (root, run, conclusion, mutant) = (
        explanation.root,
        explanation.run,
        explanation.conclusion,
        explanation.mutant,
    );
    super::say(stdout, &format!("RUN\t{run}"))?;
    super::say(
        stdout,
        &format!("MUTANT\t{}\t{}", mutant.id(), mutant.display_id()),
    )?;
    super::say(
        stdout,
        &format!(
            "WHERE\t{}:{}:{}",
            escape(mutant.path()),
            mutant.position().line,
            mutant.position().column
        ),
    )?;
    super::say(stdout, &format!("RULE\t{}", escape(mutant.rule())))?;
    super::say(
        stdout,
        &format!("DECISION\t{}", escape(mutant.decision().name())),
    )?;
    for fact in mutant.by_build() {
        super::say(
            stdout,
            &format!(
                "BUILD-OUTCOME\t{}\t{}",
                fact.build(),
                escape(fact.outcome().name())
            ),
        )?;
        if let Some(by) = fact.outcome().decided_by() {
            super::say(
                stdout,
                &format!("DECIDED-BY\t{}\t{}", fact.build(), escape(by)),
            )?;
        }
        if let Some(provenance) = fact.reuse().0.read_back() {
            super::say(
                stdout,
                &format!("REUSED-FROM\t{}\t{}", fact.build(), escape(provenance)),
            )?;
        }
    }
    for finding in conclusion
        .findings
        .iter()
        .filter(|finding| finding.subject == mutant.display_id())
    {
        super::say(
            stdout,
            &format!(
                "FINDING\t{}\t{}",
                finding.kind_name(),
                escape(&finding.detail)
            ),
        )?;
    }
    if matches!(
        mutant.decision(),
        crate::report::Decision::Unnoticed | crate::report::Decision::Unreached
    ) {
        super::say(stdout, &acceptance(root, mutant))?;
    }
    Ok(())
}

/// What a reviewer recorded about this survivor, or how to record something.
///
/// Reading "somebody accepted it" off the absence of a finding says a person
/// signed this off when all that is known is that nothing complained, which is
/// also what a shard, a suppressed kind and a merged report look like. The
/// acceptances are in the configuration, so the question is asked of them, and
/// where there is no answer the reader is told how to write one.
fn acceptance(root: &std::path::Path, mutant: &crate::report::ProjectedMutant) -> String {
    let accepted = match crate::config::Config::load(root) {
        Ok(config) => config
            .acceptance
            .into_iter()
            .find(|one| mutant.id().starts_with(&one.id)),
        Err(error) => {
            return format!("ACCEPTANCE\tconfiguration could not be read: {error}");
        }
    };
    let Some(one) = accepted else {
        return format!(
            "ACCEPTANCE\tnobody has recorded a reason for this one\tnjutest accept {} \
             --reason \"...\" writes one into .njutest.toml",
            crate::naming::locator(mutant)
        );
    };
    format!(
        "ACCEPTANCE\t{}\treason={}\towner={}\texpires={}",
        one.id,
        escape(&one.reason),
        one.owner.as_deref().unwrap_or("(nobody named)"),
        one.expires
            .map_or_else(|| String::from("(never)"), |at| at.to_string())
    )
}

/// The one mutant a prefix names, or a diagnostic saying why it names none or several.
fn one<'a>(
    matching: &[&'a crate::report::ProjectedMutant],
    prefix: &str,
    stderr: &mut dyn Write,
) -> std::io::Result<Option<&'a crate::report::ProjectedMutant>> {
    match matching {
        [only] => Ok(Some(only)),
        [] => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: no mutant of that run starts with {prefix}",
                    crate::error::RUN_NOT_FOUND.code
                ),
            )?;
            Ok(None)
        }
        several => {
            let names: Vec<&str> = several.iter().map(|mutant| mutant.display_id()).collect();
            super::diagnose(
                stderr,
                &format!(
                    "{}: {prefix} names {} mutants: {}",
                    crate::error::RUN_NOT_FOUND.code,
                    several.len(),
                    names.join(", ")
                ),
            )?;
            Ok(None)
        }
    }
}
