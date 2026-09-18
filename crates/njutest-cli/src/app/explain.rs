// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `njutest explain`: everything a run recorded about one mutant.

use std::io::Write;

use crate::app::runs;
use crate::cli::{EXIT_ASSURED, EXIT_ERROR, Environment, Explain as Arguments};
use crate::report::lines::escape;

/// Prints what a run established about one mutant.
pub fn run(
    arguments: &Arguments,
    environment: &Environment,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> u8 {
    let root = &environment.working_directory;
    let run = match runs::resolve(root, arguments.run.as_deref()) {
        Ok(run) => run,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };
    let report = match runs::report(root, &run) {
        Ok(report) => report,
        Err(error) => {
            super::complain(stderr, &error, error.code());
            return EXIT_ERROR;
        }
    };

    let every: Vec<&crate::report::MutantRecord> = report.mutants.iter().collect();
    let matching = crate::naming::matching(&every, &arguments.mutant);
    let Some(mutant) = one(&matching, &arguments.mutant, stderr) else {
        return EXIT_ERROR;
    };

    super::say(stdout, &format!("RUN\t{run}"));
    super::say(
        stdout,
        &format!("MUTANT\t{}\t{}", mutant.id, mutant.display_id),
    );
    super::say(
        stdout,
        &format!(
            "WHERE\t{}:{}:{}",
            escape(&mutant.path),
            mutant.position.line,
            mutant.position.column
        ),
    );
    super::say(stdout, &format!("RULE\t{}", escape(&mutant.rule)));
    super::say(
        stdout,
        &format!("OUTCOME\t{}", escape(mutant.outcome.name())),
    );
    if let Some(by) = mutant.outcome.decided_by() {
        super::say(stdout, &format!("DECIDED-BY\t{}", escape(by)));
    }
    if let Some(provenance) = &mutant.source_run_id {
        super::say(stdout, &format!("REUSED-FROM\t{}", escape(provenance)));
    }
    for finding in report
        .findings
        .iter()
        .filter(|finding| finding.subject == mutant.display_id)
    {
        super::say(
            stdout,
            &format!(
                "FINDING\t{}\t{}",
                finding.kind_name(),
                escape(&finding.detail)
            ),
        );
    }
    if mutant.outcome.outcome() == crate::report::Outcome::Survived {
        super::say(stdout, &acceptance(root, mutant));
    }
    EXIT_ASSURED
}

/// What a reviewer recorded about this survivor, or how to record something.
///
/// Reading "somebody accepted it" off the absence of a finding says a person
/// signed this off when all that is known is that nothing complained, which is
/// also what a shard, a suppressed kind and a merged report look like. The
/// acceptances are in the configuration, so the question is asked of them, and
/// where there is no answer the reader is told how to write one.
fn acceptance(root: &std::path::Path, mutant: &crate::report::MutantRecord) -> String {
    let accepted = crate::config::Config::load(root).ok().and_then(|config| {
        config
            .acceptance
            .into_iter()
            .find(|one| mutant.id.starts_with(&one.id))
    });
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
    matching: &[&'a crate::report::MutantRecord],
    prefix: &str,
    stderr: &mut dyn Write,
) -> Option<&'a crate::report::MutantRecord> {
    match matching {
        [only] => Some(only),
        [] => {
            super::diagnose(
                stderr,
                &format!(
                    "{}: no mutant of that run starts with {prefix}",
                    crate::error::RUN_NOT_FOUND.code
                ),
            );
            None
        }
        several => {
            let names: Vec<&str> = several
                .iter()
                .map(|mutant| mutant.display_id.as_str())
                .collect();
            super::diagnose(
                stderr,
                &format!(
                    "{}: {prefix} names {} mutants: {}",
                    crate::error::RUN_NOT_FOUND.code,
                    several.len(),
                    names.join(", ")
                ),
            );
            None
        }
    }
}
