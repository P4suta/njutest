// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every flag a refusal tells its reader to pass is one this release parses.

use clap::CommandFactory as _;

/// Every long flag `command` or any of its subcommands accepts.
fn accepted(command: &clap::Command, into: &mut std::collections::BTreeSet<String>) {
    into.extend(
        command
            .get_arguments()
            .filter_map(clap::Arg::get_long)
            .map(|long| format!("--{long}")),
    );
    for subcommand in command.get_subcommands() {
        accepted(subcommand, into);
    }
}

#[test]
fn every_flag_a_refusal_names_is_one_this_release_parses() {
    let mut command = rust_mutants_cli::cli::Cli::command();
    command.build();
    let mut known = std::collections::BTreeSet::new();
    accepted(&command, &mut known);
    let mut named = 0_usize;
    let mut missing = Vec::new();
    for code in rust_mutants::error::error_codes() {
        let said = match code.remedy {
            Some(remedy) => vec![code.summary, remedy],
            None => vec![code.summary],
        };
        for text in said {
            for flag in njutest_devkit::named_flags::named_flags(text, "rust-mutants") {
                named = named.saturating_add(1);
                if !known.contains(&flag) {
                    missing.push(format!("{}: {flag}", code.code));
                }
            }
        }
    }
    assert!(
        named > 0,
        "the refusals name flags, so a law that reads none is reading nothing"
    );
    assert!(
        missing.is_empty(),
        "a refusal whose way out is a flag this release does not have leaves its reader with no \
         way out at all: {missing:?}"
    );
}

#[test]
fn a_flag_in_another_command_s_code_span_is_that_command_s() {
    assert_eq!(
        njutest_devkit::named_flags::named_flags(
            "run `cargo test --no-run` yourself, or pass --root, or `rust-mutants run --tier all`; x--y is not one",
            "rust-mutants"
        ),
        vec!["--root".to_owned(), "--tier".to_owned()]
    );
}
