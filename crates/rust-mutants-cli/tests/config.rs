// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.rust-mutants.toml`: optional, strict, and defaulted. What the file cannot say is as much a contract as what it can.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::create_dir,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::Path;
use std::time::Duration;

use rust_mutants::session::Timeout;

use rust_mutants_cli::config::{
    Config, ConfigErrorKind, DEFAULT_REPORTS_DIRECTORY, DEFAULT_REPORTS_KEEP, DEFAULT_TIMEOUT,
    FILE_NAME, skeleton,
};

fn parse(text: &str) -> Result<Config, rust_mutants_cli::config::ConfigError> {
    Config::parse(text, Path::new(FILE_NAME))
}

fn kind(text: &str) -> ConfigErrorKind {
    parse(text).map(|_ok| ()).expect_err("refused").kind()
}

#[test]
fn a_missing_file_is_the_defaults_and_a_present_one_is_read() {
    let root = tempfile::tempdir().expect("temp");
    assert_eq!(
        Config::load(root.path()).expect("no file is the defaults"),
        Config::default()
    );
    std::fs::write(
        root.path().join(FILE_NAME),
        "version = 1\n[mutation]\ntier = \"all\"\n",
    )
    .expect("write");
    let config = Config::load(root.path()).expect("read");
    assert_eq!(config.mutation.tier, rust_mutants::rule::Tier::All);
}

#[test]
fn the_defaults_are_the_numbers_the_contract_states() {
    let config = Config::default();
    assert_eq!(config.version, 1);
    assert_eq!(config.mutation.tier, rust_mutants::rule::Tier::Balanced);
    assert_eq!(config.mutation.timeout, DEFAULT_TIMEOUT);
    assert_eq!(
        config.mutation.timeout,
        Timeout::Auto,
        "a budget nobody chose is a multiple of what the target's own baseline took"
    );
    assert_eq!(config.mutation.build_timeout, None);
    assert!(config.mutation.verify, "a run verifies unless told not to");
    assert!(config.mutation.expect.is_empty());
    assert!(config.project.packages.is_empty());
    assert_eq!(config.reports.keep, DEFAULT_REPORTS_KEEP);
    assert_eq!(
        config.reports.directory,
        Path::new(DEFAULT_REPORTS_DIRECTORY)
    );
    assert!(!config.execution.offline);
    assert!(!config.execution.locked);
}

#[test]
fn an_empty_file_and_the_written_skeleton_both_mean_the_defaults() {
    assert_eq!(parse("").expect("empty"), Config::default());
    assert_eq!(
        parse(&skeleton()).expect("the skeleton parses"),
        Config::default(),
        "every line the skeleton leaves uncommented must already be the default"
    );
    assert!(skeleton().starts_with("# rust-mutants configuration"));
}

#[test]
fn every_section_is_read_the_way_the_contract_describes_it() {
    let config = parse(
        "\
version = 1

[project]
packages = [\"demo\"]
include = [\"src/**\"]
exclude = [\"src/generated/**\"]

[mutation]
tier = \"strong\"
operators = [\"add-to-sub\"]
timeout = \"90s\"
build_timeout = \"10m\"
verify = false

[[mutation.expect]]
id = \"b8e3f78d\"
outcome = \"survived\"
reason = \"the bound is equivalent under the invariant the type carries\"

[execution]
offline = true
locked = true
test_binary_args = [\"--test-threads=1\"]

[reports]
directory = \"out/mutation\"
keep = 3
",
    )
    .expect("a whole document");
    assert_eq!(config.project.packages, ["demo"]);
    assert_eq!(config.project.include, ["src/**"]);
    assert_eq!(config.project.exclude, ["src/generated/**"]);
    assert_eq!(config.mutation.tier, rust_mutants::rule::Tier::Strong);
    assert_eq!(config.mutation.operators, ["add-to-sub"]);
    assert_eq!(
        config.mutation.timeout,
        Timeout::Fixed(Duration::from_secs(90))
    );
    assert_eq!(
        config.mutation.build_timeout,
        Some(Duration::from_secs(600))
    );
    assert!(!config.mutation.verify);
    assert_eq!(config.mutation.expect.len(), 1);
    assert_eq!(config.mutation.expect[0].id.as_deref(), Some("b8e3f78d"));
    assert_eq!(
        config.mutation.expect[0].outcome(),
        Some(rust_mutants::outcome::Outcome::Survived)
    );
    assert!(config.execution.offline && config.execution.locked);
    assert_eq!(config.execution.test_binary_args, ["--test-threads=1"]);
    assert_eq!(config.reports.directory, Path::new("out/mutation"));
    assert_eq!(config.reports.keep, 3);
}

#[test]
fn an_unknown_key_is_an_error_that_names_it() {
    let error = parse("version = 1\nbanana = true\n").expect_err("refused");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("banana"), "{error}");
    assert_eq!(kind("[mutation]\nspeed = 3\n"), ConfigErrorKind::Unparsable);
    assert_eq!(
        kind("[[mutation.expect]]\nid = \"a\"\nreason = \"r\"\nwhen = 1\n"),
        ConfigErrorKind::Unparsable
    );
}

#[test]
fn only_version_one_is_understood() {
    assert_eq!(
        kind("version = 2\n"),
        ConfigErrorKind::UnsupportedVersion,
        "a later version is refused rather than read as this one"
    );
    assert_eq!(kind("version = 0\n"), ConfigErrorKind::UnsupportedVersion);
}

#[test]
fn a_duration_that_is_not_a_duration_names_the_key_it_came_from() {
    let error = parse("[mutation]\ntimeout = \"soon\"\n").expect_err("refused");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("timeout"), "{error}");
}

#[test]
fn an_expectation_without_a_reason_is_a_suppression_and_is_refused() {
    assert_eq!(
        kind("[[mutation.expect]]\nid = \"abcd\"\n"),
        ConfigErrorKind::Unparsable,
        "reason is required by the shape itself"
    );
    assert_eq!(
        kind("[[mutation.expect]]\nid = \"abcd\"\nreason = \"  \"\n"),
        ConfigErrorKind::Invalid,
        "a blank reason is no reason"
    );
    assert_eq!(
        kind("[[mutation.expect]]\nid = \"\"\nreason = \"why\"\n"),
        ConfigErrorKind::Invalid
    );
    assert_eq!(
        kind("[[mutation.expect]]\nid = \"abcd\"\nreason = \"why\"\noutcome = \"errored\"\n"),
        ConfigErrorKind::Invalid,
        "only an outcome a run can confirm may be expected"
    );
    assert_eq!(
        kind(
            "[[mutation.expect]]\nid = \"ab\"\nreason = \"why\"\n[[mutation.expect]]\nid = \"ab\"\nreason = \"other\"\n"
        ),
        ConfigErrorKind::Invalid,
        "two expectations for one mutant cannot both be the reason"
    );
}

#[test]
fn the_harness_flags_the_engine_owns_are_refused_and_the_rest_pass() {
    for allowed in [
        "--test-threads=2",
        "--include-ignored",
        "--nocapture",
        "--show-output",
    ] {
        parse(&format!(
            "[execution]\ntest_binary_args = [\"{allowed}\"]\n"
        ))
        .unwrap_or_else(|error| panic!("{allowed}: {error}"));
    }
    for reserved in [
        "--exact",
        "--list",
        "--format=json",
        "--logfile=x",
        "--skip=y",
        "--ignored",
        "-Zunstable-options",
        "some::test",
    ] {
        let error = parse(&format!(
            "[execution]\ntest_binary_args = [\"{reserved}\"]\n"
        ))
        .expect_err(reserved);
        assert_eq!(error.kind(), ConfigErrorKind::Invalid, "{reserved}");
    }
}

#[test]
fn a_pattern_and_an_operator_are_checked_when_the_file_is_read_not_when_the_run_is_half_done() {
    assert_eq!(
        kind("[project]\ninclude = [\"\"]\n"),
        ConfigErrorKind::Invalid
    );
    assert_eq!(
        kind("[project]\nexclude = [\"/absolute\"]\n"),
        ConfigErrorKind::Invalid
    );
    assert_eq!(
        kind("[mutation]\noperators = [\"no-such-rule\"]\n"),
        ConfigErrorKind::Invalid
    );
}

#[test]
fn a_report_directory_stays_inside_the_workspace() {
    assert_eq!(
        kind("[reports]\ndirectory = \"/tmp/elsewhere\"\n"),
        ConfigErrorKind::Invalid
    );
    assert_eq!(
        kind("[reports]\ndirectory = \"../elsewhere\"\n"),
        ConfigErrorKind::Invalid
    );
    assert_eq!(
        kind("[reports]\ndirectory = \"\"\n"),
        ConfigErrorKind::Invalid
    );
    let leaving: &[&str] = if cfg!(windows) {
        &[
            "/tmp/elsewhere",
            "\\\\server\\share\\elsewhere",
            "C:\\elsewhere",
            "C:elsewhere",
        ]
    } else {
        &["/tmp/elsewhere"]
    };
    for named in leaving {
        assert_eq!(
            kind(&format!("[reports]\ndirectory = '{named}'\n")),
            ConfigErrorKind::Invalid,
            "{named} names somewhere the workspace root does not reach on the machine this \
             runs on, and a run that took it would write its reports where nobody said. \
             Which spellings those are is the platform's answer: a volume and a share are \
             names of their own on a unix machine, and places of their own on Windows"
        );
    }
}

#[test]
fn every_failure_carries_the_code_its_reader_can_search_for() {
    let codes: Vec<&str> = ConfigErrorKind::ALL
        .iter()
        .map(|kind| kind.code().code)
        .collect();
    assert_eq!(codes, ["RM0002", "RM0003", "RM0004", "RM0005"]);
    let declared: Vec<&str> = rust_mutants::error::error_codes()
        .iter()
        .map(|code| code.code)
        .collect();
    for code in codes {
        assert!(declared.contains(&code), "{code} is not in the ledger");
    }
    let root = tempfile::tempdir().expect("temp");
    std::fs::create_dir(root.path().join(FILE_NAME)).expect("a directory where a file goes");
    let error = Config::load(root.path()).expect_err("a directory is not a document");
    assert_eq!(error.kind(), ConfigErrorKind::Unreadable);
    assert!(error.to_string().contains("RM0002"), "{error}");
}

#[test]
fn build_keys_default_to_nothing() {
    let config = Config::default();
    assert!(config.build.features.is_empty());
    assert!(!config.build.all_features);
    assert!(!config.build.no_default_features);
    assert!(config.build.target.is_empty());
    assert!(config.build.profile.is_empty());
    assert_eq!(config.build.jobs, 0);
    assert!(
        config.build.config().is_default(),
        "a build nobody configured is the build cargo would have done"
    );
}

#[test]
fn a_build_section_becomes_the_engine_s_build_configuration() {
    let config = parse(
        "\
version = 1

[build]
features = [\"one\", \"two\"]
no_default_features = true
target = \"wasm32-unknown-unknown\"
profile = \"release\"
jobs = 2
",
    )
    .expect("the section is read");
    assert_eq!(
        config.build.config(),
        rust_mutants::cargo::BuildConfig {
            features: vec!["one".to_owned(), "two".to_owned()],
            all_features: false,
            no_default_features: true,
            target: Some("wasm32-unknown-unknown".to_owned()),
            profile: Some("release".to_owned()),
            jobs: Some(2),
            debug: false,
        },
        "an empty name and a zero are what nobody said, not what somebody asked for"
    );
}

#[test]
fn a_configured_skip_names_a_path_a_reason_and_at_most_one_of_lines_or_item() {
    let good = parse(
        "version = 1\n[[mutation.skip]]\npath = \"src/scanner/**\"\nitem = \"Scanner::skip_ws\"\nreason = \"a hand-tuned loop; a mutant here is a timeout, not a finding\"\n",
    )
    .expect("a skip that names a path, an item and a reason");
    assert_eq!(good.mutation.skip.len(), 1);
    assert_eq!(
        good.mutation.skip[0].item.as_deref(),
        Some("Scanner::skip_ws")
    );

    assert_eq!(
        kind("version = 1\n[[mutation.skip]]\npath = \"src/lib.rs\"\nreason = \"\"\n"),
        ConfigErrorKind::Invalid,
        "a skip nobody explained is one nobody can review"
    );
    assert_eq!(
        kind(
            "version = 1\n[[mutation.skip]]\npath = \"src/lib.rs\"\nlines = \"40-58\"\nitem = \"f\"\nreason = \"why\"\n"
        ),
        ConfigErrorKind::Invalid,
        "lines and an item are two ways to say where, and a skip says it once"
    );
    assert_eq!(
        kind(
            "version = 1\n[[mutation.skip]]\npath = \"src/lib.rs\"\nlines = \"58-40\"\nreason = \"why\"\n"
        ),
        ConfigErrorKind::Invalid,
        "a range that ends before it starts describes nothing"
    );
}

#[test]
fn lines_requires_a_literal_path() {
    assert_eq!(
        kind(
            "version = 1\n[[mutation.skip]]\npath = \"src/**\"\nlines = \"40-58\"\nreason = \"why\"\n"
        ),
        ConfigErrorKind::Invalid,
        "line forty of every file a glob matches is not a place anybody meant"
    );
    let good = parse(
        "version = 1\n[[mutation.skip]]\npath = \"src/lib.rs\"\nlines = \"40-58\"\nreason = \"why\"\n",
    )
    .expect("a literal path with a range");
    assert_eq!(good.mutation.skip[0].lines.as_deref(), Some("40-58"));
}

#[test]
fn an_expectation_is_addressed_by_id_or_by_locator_and_never_both() {
    let by_id = parse("version = 1\n[[mutation.expect]]\nid = \"abc\"\nreason = \"why\"\n")
        .expect("an expectation by identity");
    assert_eq!(by_id.mutation.expect[0].id.as_deref(), Some("abc"));
    assert!(!by_id.mutation.expect[0].is_locator());

    let by_locator = parse(
        "version = 1\n[[mutation.expect]]\npath = \"src/lib.rs\"\nitem = \"clamp\"\nrule = \"le-to-lt\"\noriginal = \"<=\"\nline = 42\nreason = \"the bound is equivalent under the invariant the type carries\"\n",
    )
    .expect("an expectation by locator");
    let locator = by_locator.mutation.expect[0]
        .expectation()
        .locator
        .expect("a locator");
    assert_eq!(locator.item, "clamp");
    assert_eq!(locator.line, Some(42));

    assert_eq!(
        kind(
            "version = 1\n[[mutation.expect]]\nid = \"abc\"\npath = \"src/lib.rs\"\nitem = \"clamp\"\nrule = \"le-to-lt\"\noriginal = \"<=\"\nreason = \"why\"\n"
        ),
        ConfigErrorKind::Invalid,
        "an identity and a locator are two ways to name one mutant, and a claim names it once"
    );
    assert_eq!(
        kind(
            "version = 1\n[[mutation.expect]]\npath = \"src/lib.rs\"\nitem = \"clamp\"\nreason = \"why\"\n"
        ),
        ConfigErrorKind::Invalid,
        "a locator that names no rule and no original names a place, not a mutation"
    );
}

#[test]
fn a_count_says_how_many_mutations_one_reason_was_written_for() {
    let locator = "version = 1\n[[mutation.expect]]\npath = \"src/prove.rs\"\nitem = \"establish\"\nrule = \"delete-call-statement\"\noriginal = \"phase.end();\"\n";
    let counted = parse(&format!(
        "{locator}count = 3\nreason = \"Phase ends its phase in Drop, and the trace test holds it\"\n"
    ))
    .expect("an expectation written for three mutations");
    assert_eq!(
        counted.mutation.expect[0]
            .expectation()
            .locator
            .expect("a locator")
            .count,
        Some(3)
    );

    assert_eq!(
        kind(&format!("{locator}count = 0\nreason = \"why\"\n")),
        ConfigErrorKind::Invalid,
        "a claim written for no mutation is not a claim"
    );
    assert_eq!(
        kind("version = 1\n[[mutation.expect]]\nid = \"abc\"\ncount = 2\nreason = \"why\"\n"),
        ConfigErrorKind::Invalid,
        "an identity is one mutation, so counting what it names says two different things"
    );
}

#[test]
fn a_root_a_command_names_is_resolved_against_where_the_command_was_told_it_is() {
    use std::path::{Path, PathBuf};

    use rust_mutants_cli::cli;
    use rust_mutants_cli::settings::Settings;

    let here = tempfile::tempdir().expect("a directory");
    let inside = here.path().join("tree");
    std::fs::create_dir_all(&inside).expect("a tree below it");
    let environment = rust_mutants_cli::Environment {
        vars: Vec::new(),
        temp_directory: PathBuf::from("/tmp"),
        program: PathBuf::from("this test never runs it"),
        cache_directory: PathBuf::from("/tmp/cache"),
        working_directory: here.path().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
    };

    let named = |root: Option<&str>| {
        let mut parsed = cli::parse(
            ["rust-mutants", "list"]
                .into_iter()
                .map(std::ffi::OsString::from),
        )
        .expect("`list` with nothing else on it parses");
        if let cli::Command::List { scope, .. } = &mut parsed.command {
            scope.root = root.map(PathBuf::from);
            return Settings::resolve(scope, &environment)
                .map(|settings| settings.root)
                .expect("a scope with nothing wrong in it");
        }
        panic!("`list` parses as List")
    };

    assert_eq!(
        named(None),
        here.path(),
        "a command that names no root is about the directory it was told it is in"
    );
    assert_eq!(
        named(Some("tree")),
        inside,
        "and a relative root is relative to that one rather than to the process's own: a \
         caller that says where it is and then gets an answer about somewhere else has \
         been told about a tree it did not name"
    );
    assert_eq!(
        named(Some(&inside.to_string_lossy())),
        inside,
        "while an absolute root is the tree it names, wherever the caller is"
    );
    assert_eq!(
        named(Some(".")),
        Path::new(here.path()).join("."),
        "a dot is the directory the command was told it is in"
    );
}
