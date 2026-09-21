// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line contract of the `rust-mutants` binary: what `--version` and `--help` print, and the exit codes of usage errors.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking and reads a document as a table"
)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Output;

use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};

fn rust_mutants(args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied())
            .map(OsString::from),
        &environment(),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}

/// The environment a command that reads no tree is answered in.
#[expect(
    clippy::panic,
    reason = "the command-line contract cannot run without a working directory"
)]
fn environment() -> Environment {
    let working_directory = match std::env::current_dir() {
        Ok(working_directory) => working_directory,
        Err(error) => panic!("the test process has no working directory: {error}"),
    };
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: std::env::temp_dir(),
        program: PathBuf::from("this test never runs it"),
        cache_directory: std::env::temp_dir(),
        working_directory,
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    }
}

#[test]
fn version_flag_prints_the_binary_name_and_its_version() {
    let output = rust_mutants(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        njutest_devkit::process::strict_utf8(&output.stdout),
        format!("rust-mutants {}\n", rust_mutants::VERSION)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_flag_matches_the_recorded_help_text() {
    let output = rust_mutants(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    njutest_devkit::golden::golden(&golden, &output.stdout).expect("help text is the recorded one");
}

/// Every command the top-level help lists, which is every command there is.
fn subcommands() -> Vec<String> {
    let help = njutest_devkit::process::strict_utf8(&rust_mutants(&["--help"]).stdout).into_owned();
    let listing = help
        .split_once("Commands:\n")
        .map_or(String::new(), |(_before, rest)| {
            rest.split("\n\n").next().unwrap_or_default().to_owned()
        });
    let named: Vec<String> = listing
        .lines()
        .filter_map(|line| line.strip_prefix("  "))
        .filter(|line| !line.starts_with(' '))
        .filter_map(|line| line.split_whitespace().next())
        .filter(|name| *name != "help")
        .map(str::to_owned)
        .collect();
    assert!(
        named.len() > 10,
        "the help lists {} commands, and reading none of them is not the same as there \
         being none: {listing}",
        named.len()
    );
    named
}

#[test]
fn every_subcommand_has_its_own_recorded_help() {
    for name in subcommands() {
        let output = rust_mutants(&[name.as_str(), "--help"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "the help offers {name} and the program does not take it: {}",
            njutest_devkit::process::strict_utf8(&output.stderr)
        );
        let golden = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("tests/testdata/help-{name}.golden"));
        njutest_devkit::golden::golden(&golden, &output.stdout)
            .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn no_arguments_prints_the_usage_to_stderr_and_exits_2() {
    let output = rust_mutants(&[]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(njutest_devkit::process::strict_utf8(&output.stderr).contains("Usage:"));
}

#[test]
fn an_unknown_subcommand_is_a_usage_error() {
    let output = rust_mutants(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(2));
    let stderr = njutest_devkit::process::strict_utf8(&output.stderr);
    assert!(
        stderr.contains("frobnicate"),
        "names the offending argument: {stderr}"
    );
}

/// Every subcommand, so a page and a golden both stay complete when one is added.
const SUBCOMMANDS: [&str; 16] = [
    "run",
    "list",
    "catalog",
    "explain",
    "why-skipped",
    "instrument",
    "replay",
    "equivalence",
    "rules",
    "init",
    "doctor",
    "diagnostics",
    "merge",
    "trace",
    "report",
    "cache",
];

#[test]
fn every_subcommand_has_the_recorded_help_text() {
    let mut recorded = String::new();
    for name in SUBCOMMANDS {
        let output = rust_mutants(&[name, "--help"]);
        assert_eq!(output.status.code(), Some(0), "{name} --help");
        recorded.push_str("$ rust-mutants ");
        recorded.push_str(name);
        recorded.push_str(" --help\n");
        recorded.push_str(&njutest_devkit::process::strict_utf8(&output.stdout));
        recorded.push('\n');
    }
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/subcommands.golden");
    njutest_devkit::golden::golden(&golden, recorded.as_bytes())
        .expect("the subcommand help is the recorded one");
}

#[test]
fn the_command_line_page_and_the_help_texts_name_the_same_flags() {
    let at = njutest_devkit::paths::workspace_root().join("docs/engine/command-line.md");
    let page = std::fs::read_to_string(&at)
        .unwrap_or_else(|error| panic!("the command line page at {}: {error}", at.display()));
    let mut helped: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for name in SUBCOMMANDS.into_iter().chain(std::iter::once("")) {
        let output = if name.is_empty() {
            rust_mutants(&["--help"])
        } else {
            rust_mutants(&[name, "--help"])
        };
        helped.extend(flags(&njutest_devkit::process::strict_utf8(&output.stdout)));
    }
    let missing: Vec<&String> = helped
        .iter()
        .filter(|flag| !page.contains(flag.as_str()))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/engine/command-line.md does not name {missing:?}"
    );

    let written = flags(&page);
    let unknown: Vec<&String> = written
        .iter()
        .filter(|flag| !helped.contains(*flag))
        .collect();
    assert!(
        unknown.is_empty(),
        "docs/engine/command-line.md names flags no command has: {unknown:?}"
    );
}

#[test]
fn the_flags_the_page_lists_beside_a_command_are_that_command_s_own() {
    let at = njutest_devkit::paths::workspace_root().join("docs/engine/command-line.md");
    let page = std::fs::read_to_string(&at)
        .unwrap_or_else(|error| panic!("the command line page at {}: {error}", at.display()));
    let rows = beside_a_command(&page);
    assert!(
        rows.len() > 5,
        "the table that lists a command and its flags is read: {rows:?}"
    );
    let mut wrong: Vec<String> = Vec::new();
    for (command, listed) in rows {
        let helped = flags(&njutest_devkit::process::strict_utf8(
            &rust_mutants(&[command.as_str(), "--help"]).stdout,
        ));
        wrong.extend(
            listed
                .into_iter()
                .filter(|flag| !helped.contains(flag))
                .map(|flag| format!("{command} {flag}")),
        );
    }
    assert!(
        wrong.is_empty(),
        "docs/engine/command-line.md lists these beside a command that does not take them: \
         {wrong:?}. A flag that exists on some other command is not this one's, and a \
         reader who types what the row says is answered with a usage error"
    );
}

/// Every row of the page's one table of commands and their flags, as the command and what it lists.
fn beside_a_command(page: &str) -> Vec<(String, std::collections::BTreeSet<String>)> {
    let mut rows = Vec::new();
    let mut reading = false;
    for line in page.lines() {
        if line.starts_with("| Command ") {
            reading = true;
            continue;
        }
        if reading && !line.starts_with('|') {
            reading = false;
            continue;
        }
        if !reading || line.starts_with("| ---") {
            continue;
        }
        let mut cells = line.split('|').skip(1);
        let (Some(named), Some(listed)) = (cells.next(), cells.next()) else {
            continue;
        };
        let Some(command) = named
            .trim()
            .trim_matches('`')
            .split_whitespace()
            .next()
            .map(ToOwned::to_owned)
        else {
            continue;
        };
        rows.push((command, flags(listed)));
    }
    rows
}

/// Every long flag a text spells, as `--name`.
fn flags(text: &str) -> std::collections::BTreeSet<String> {
    let mut found = std::collections::BTreeSet::new();
    let mut rest = text;
    while let Some(at) = rest.find("--") {
        let tail = rest.split_at(at).1.get(2..).unwrap_or("");
        let end = tail
            .find(|one: char| !one.is_ascii_alphanumeric() && one != '-')
            .unwrap_or(tail.len());
        let name = tail.get(..end).unwrap_or("");
        if name.len() > 1 && name.starts_with(|one: char| one.is_ascii_lowercase()) {
            found.insert(format!("--{name}"));
        }
        rest = tail.get(end..).unwrap_or("");
    }
    found
}

#[test]
fn rules_lists_every_rule_with_its_tier_and_version_so_a_team_can_pin_operators() {
    let output = rust_mutants(&["rules"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let text = njutest_devkit::process::strict_utf8(&output.stdout).into_owned();
    for rule in rust_mutants::rule::CANONICAL_TABLE {
        assert!(
            text.contains(rule.name),
            "{} is a rule this release has and does not list",
            rule.name
        );
    }
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/rules.golden");
    njutest_devkit::golden::golden(&golden, &output.stdout)
        .expect("the rules are the recorded set");
}

#[test]
fn rules_answers_as_a_document_when_it_is_asked_to() {
    let output = rust_mutants(&["rules", "--json"]);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_slice(&output.stdout).expect("the answer is JSON");
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/rules.golden.json");
    njutest_devkit::golden::golden(&golden, &output.stdout)
        .expect("the document form of the operator table is the recorded one");
    let rules = document["rules"].as_array().expect("the rules");
    assert_eq!(rules.len(), rust_mutants::rule::CANONICAL_RULE_COUNT);
    for rule in rules {
        assert!(rule["name"].as_str().is_some_and(|it| !it.is_empty()));
        assert!(rule["family"].as_str().is_some_and(|it| !it.is_empty()));
        assert!(rule["tier"].as_str().is_some_and(|it| !it.is_empty()));
        assert!(rule["version"].as_u64().is_some());
    }
}

#[test]
fn rules_narrowed_to_a_tier_is_what_that_tier_selects() {
    let output = rust_mutants(&["rules", "--tier", "balanced", "--json"]);
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_slice(&output.stdout).expect("the answer is JSON");
    let named: Vec<String> = document["rules"]
        .as_array()
        .expect("the rules")
        .iter()
        .filter_map(|rule| rule["name"].as_str().map(ToOwned::to_owned))
        .collect();
    let selected: Vec<String> = rust_mutants::rule::Registry::canonical()
        .select_tier(rust_mutants::rule::Tier::Balanced)
        .into_iter()
        .map(|rule| rule.name.to_owned())
        .collect();
    assert_eq!(named, selected);
    assert!(named.len() < rust_mutants::rule::CANONICAL_RULE_COUNT);
}

/// An optional-valued flag joined with a space takes the next word, which is somebody's argument.
#[test]
fn the_trace_flag_never_eats_the_argument_after_it() {
    use clap::Parser as _;
    let parsed = rust_mutants_cli::cli::Cli::try_parse_from([
        "rust-mutants",
        "explain",
        "--trace",
        "deadbeef1234",
    ])
    .expect("a command line naming a mutant and asking for a trace");
    let rust_mutants_cli::cli::Command::Explain { mutant, .. } = parsed.command else {
        panic!("explain parses as explain");
    };
    assert_eq!(
        mutant, "deadbeef1234",
        "the word after --trace is the mutant the caller named; reading it as the trace \
         directory made the command refuse for want of an argument it had been given"
    );
}
