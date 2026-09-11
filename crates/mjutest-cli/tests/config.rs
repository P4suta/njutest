// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.mjutest.toml`: optional, strict, and defaulted.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::time::Duration;

use mjutest_cli::config::{
    Config, ConfigErrorKind, Contract, DEFAULT_BUILD_MAX_BYTES, DEFAULT_CACHE_MAX_BYTES,
    DEFAULT_CACHE_TTL, DEFAULT_REPORTS_KEEP, DEFAULT_TIMEOUT, FILE_NAME, parse_duration, skeleton,
};

fn load(text: &str) -> Result<Config, mjutest_cli::config::ConfigError> {
    Config::parse(text, std::path::Path::new(".mjutest.toml"))
}

fn expect_error(text: &str) -> mjutest_cli::config::ConfigError {
    load(text).expect_err("this configuration is refused")
}

#[test]
fn the_defaults_are_the_numbers_the_contract_states() {
    let config = Config::default();
    assert_eq!(config.version, 1);
    assert_eq!(config.contract, Contract::StandardV1);
    assert_eq!(config.execution.timeout, DEFAULT_TIMEOUT);
    assert_eq!(config.execution.jobs, 0);
    assert_eq!(config.cache.max_bytes, DEFAULT_CACHE_MAX_BYTES);
    assert_eq!(config.cache.ttl, DEFAULT_CACHE_TTL);
    assert_eq!(config.cache.build_max_bytes, DEFAULT_BUILD_MAX_BYTES);
    assert_eq!(config.cache.build_dir, None);
    assert_eq!(config.reports.keep, DEFAULT_REPORTS_KEEP);
    assert_eq!(FILE_NAME, ".mjutest.toml");
}

#[test]
fn the_defaults_ask_for_nothing_a_run_has_to_be_told() {
    let config = Config::default();
    let empty: [(&str, bool); 8] = [
        ("packages", config.project.packages.is_empty()),
        ("exclude", config.project.exclude.is_empty()),
        ("features", config.execution.features.is_empty()),
        (
            "test_binary_args",
            config.execution.test_binary_args.is_empty(),
        ),
        ("environment", config.execution.environment.is_empty()),
        ("miri_flags", config.soundness.miri_flags.is_empty()),
        ("sanitizers", config.soundness.sanitizers.is_empty()),
        ("acceptance", config.acceptance.is_empty()),
    ];
    for (name, is_empty) in empty {
        assert!(is_empty, "{name} is not empty by default");
    }
    assert!(!config.execution.all_features);
    assert!(!config.execution.no_default_features);
    assert!(config.resources.is_empty());
    assert!(config.generation.is_none());
}

#[test]
fn an_empty_file_and_the_written_skeleton_both_mean_the_defaults() {
    assert_eq!(load("").expect("empty is legal"), Config::default());
    assert_eq!(
        load(&skeleton()).expect("the skeleton loads"),
        Config::default(),
        "the untouched skeleton is exactly the defaults"
    );
    for section in [
        "[project]",
        "[execution]",
        "[cache]",
        "[reports]",
        "[soundness]",
        "[resources.",
        "[generation]",
        "[[acceptance]]",
    ] {
        assert!(skeleton().contains(section), "{section} is missing");
    }
    assert!(skeleton().starts_with("# "), "the skeleton explains itself");
}

#[test]
fn an_unknown_key_is_an_error_that_names_it() {
    let error = expect_error("version = 1\nunknown = true\n");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("unknown"), "{error}");
    assert!(error.to_string().contains("MJ1002"), "{error}");

    let nested = expect_error("[execution]\njobz = 2\n");
    assert!(nested.to_string().contains("jobz"), "{nested}");
}

#[test]
fn only_version_one_is_understood() {
    for bad in ["version = 0\n", "version = 2\n", "version = \"1\"\n"] {
        let error = expect_error(bad);
        assert!(
            matches!(
                error.kind(),
                ConfigErrorKind::UnsupportedVersion | ConfigErrorKind::Unparsable
            ),
            "{bad:?}: {error}"
        );
    }
    assert_eq!(
        load("version = 1\n").expect("one is the version").version,
        1
    );
}

#[test]
fn the_contract_is_one_of_two_names() {
    assert_eq!(
        load("contract = \"deep-v1\"\n").expect("deep").contract,
        Contract::DeepV1
    );
    let error = expect_error("contract = \"strict-v9\"\n");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("strict-v9"), "{error}");
}

#[test]
fn durations_are_read_the_way_go_writes_them() {
    let cases: [(&str, u64); 8] = [
        ("0s", 0),
        ("30s", 30),
        ("10m", 600),
        ("720h", 720 * 3600),
        ("1h30m", 5400),
        ("2h45m30s", 2 * 3600 + 45 * 60 + 30),
        ("1500ms", 1),
        ("90s", 90),
    ];
    for (text, seconds) in cases {
        let parsed = parse_duration(text).unwrap_or_else(|error| panic!("{text}: {error}"));
        assert_eq!(parsed.as_secs(), seconds, "{text}");
    }
    assert_eq!(
        parse_duration("1500ms").expect("ms").as_millis(),
        1500,
        "milliseconds are kept"
    );
    for bad in ["", "10", "m", "-5s", "10 m", "1d", "1.5h", "10s10"] {
        assert!(parse_duration(bad).is_err(), "{bad:?} is not a duration");
    }
}

#[test]
fn a_duration_key_carries_its_own_name_into_the_error() {
    let error = expect_error("[execution]\ntimeout = \"forever\"\n");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("timeout"), "{error}");
    let config = load("[execution]\ntimeout = \"90s\"\n").expect("a duration");
    assert_eq!(config.execution.timeout, Duration::from_secs(90));
}

#[test]
fn the_harness_flags_mjutest_owns_are_refused_and_the_rest_pass() {
    for allowed in [
        "--test-threads=1",
        "--include-ignored",
        "--nocapture",
        "--show-output",
    ] {
        let text = format!("[execution]\ntest_binary_args = [{allowed:?}]\n");
        let config = load(&text).unwrap_or_else(|error| panic!("{allowed}: {error}"));
        assert_eq!(config.execution.test_binary_args, [allowed.to_owned()]);
    }
    for refused in [
        "my_test",
        "--exact",
        "--list",
        "--ignored",
        "--skip=x",
        "--format=json",
        "--logfile=x",
        "--test",
        "--bench",
        "-q",
        "--color=never",
        "--report-time",
        "--shuffle",
        "--shuffle-seed=1",
        "-Zunstable-options",
    ] {
        let text = format!("[execution]\ntest_binary_args = [{refused:?}]\n");
        let error = load(&text).expect_err("mjutest owns this flag");
        assert_eq!(error.kind(), ConfigErrorKind::Invalid, "{refused}");
        assert!(error.to_string().contains(refused), "{refused}: {error}");
    }
}

#[test]
fn an_environment_entry_is_a_name_and_never_a_value() {
    let config = load("[execution]\nenvironment = [\"DATABASE_URL\", \"CI\"]\n").expect("names");
    assert_eq!(config.execution.environment, ["DATABASE_URL", "CI"]);

    let with_value = expect_error("[execution]\nenvironment = [\"TOKEN=secret\"]\n");
    assert_eq!(with_value.kind(), ConfigErrorKind::Invalid);
    assert!(with_value.to_string().contains("TOKEN"), "{with_value}");
    assert!(
        !with_value.to_string().contains("secret"),
        "a refusal never repeats the value: {with_value}"
    );

    let reserved = expect_error("[execution]\nenvironment = [\"RUST_TEST_THREADS\"]\n");
    assert_eq!(reserved.kind(), ConfigErrorKind::Invalid);
    assert!(
        reserved.to_string().contains("RUST_TEST_THREADS"),
        "{reserved}"
    );
}

#[test]
fn every_section_is_read_the_way_the_contract_describes_it() {
    let text = r#"
version = 1
contract = "deep-v1"

[project]
packages = ["core", "app"]
exclude = ["**/generated/**"]

[execution]
features = ["postgres"]
all_features = true
no_default_features = true
test_binary_args = ["--test-threads=4"]
environment = ["DATABASE_URL"]
timeout = "5m"
jobs = 3

[cache]
max_bytes = 1024
ttl = "24h"
build_max_bytes = 2048
build_dir = "/var/cache/mjutest"

[reports]
keep = 5

[soundness]
miri_flags = ["-Zmiri-strict-provenance"]
sanitizers = ["thread"]

[resources.postgres]
command = ["./tools/postgres-provider"]
timeout = "30s"
shared = true
environment = ["POSTGRES_IMAGE"]

[generation]
command = ["./tools/test-generator"]
allowed_paths = ["**/tests/**/*.rs"]
environment = ["GENERATOR_TOKEN"]

[[acceptance]]
id = "0123456789abcdef"
reason = "reviewed equivalent boundary"
expires = "2026-12-31T00:00:00Z"
owner = "quality-team"
ticket = "QA-123"
"#;
    let config = load(text).expect("the documented file");
    assert_eq!(config.contract, Contract::DeepV1);
    assert_eq!(config.project.packages, ["core", "app"]);
    assert_eq!(config.project.exclude, ["**/generated/**"]);
    assert_eq!(config.execution.features, ["postgres"]);
    assert!(config.execution.all_features && config.execution.no_default_features);
    assert_eq!(config.execution.timeout, Duration::from_secs(300));
    assert_eq!(config.execution.jobs, 3);
    assert_eq!(config.cache.max_bytes, 1024);
    assert_eq!(config.cache.ttl, Duration::from_hours(24));
    assert_eq!(
        config.cache.build_dir.as_deref(),
        Some(std::path::Path::new("/var/cache/mjutest"))
    );
    assert_eq!(config.reports.keep, 5);
    assert_eq!(config.soundness.sanitizers, ["thread"]);

    assert_documented_extras(&config);
}

/// The sections a run reaches for only when it has to: resources, the generator, and the acceptances a reviewer recorded.
fn assert_documented_extras(config: &Config) {
    let postgres = config.resources.get("postgres").expect("the resource");
    assert_eq!(postgres.command, ["./tools/postgres-provider"]);
    assert_eq!(postgres.timeout, Duration::from_secs(30));
    assert!(postgres.shared && !postgres.exclusive);
    assert_eq!(postgres.environment, ["POSTGRES_IMAGE"]);

    let generation = config.generation.as_ref().expect("generation");
    assert_eq!(generation.allowed_paths, ["**/tests/**/*.rs"]);

    assert_eq!(config.acceptance.len(), 1);
    let acceptance = &config.acceptance[0];
    assert_eq!(acceptance.id, "0123456789abcdef");
    assert_eq!(acceptance.owner.as_deref(), Some("quality-team"));
    assert!(acceptance.expires.is_some());
}

#[test]
fn an_acceptance_without_a_reason_is_not_an_acceptance() {
    let error = expect_error("[[acceptance]]\nid = \"0123456789abcdef\"\n");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("reason"), "{error}");

    let bad_time = expect_error(
        "[[acceptance]]\nid = \"0123456789abcdef\"\nreason = \"why\"\nexpires = \"soon\"\n",
    );
    assert_eq!(bad_time.kind(), ConfigErrorKind::Unparsable);
    assert!(bad_time.to_string().contains("expires"), "{bad_time}");
}

#[test]
fn a_resource_cannot_be_shared_and_exclusive_at_once() {
    let error =
        expect_error("[resources.db]\ncommand = [\"x\"]\nshared = true\nexclusive = true\n");
    assert_eq!(error.kind(), ConfigErrorKind::Invalid);
    assert!(error.to_string().contains("db"), "{error}");

    let empty = expect_error("[resources.db]\ncommand = []\n");
    assert_eq!(empty.kind(), ConfigErrorKind::Invalid);
}

#[test]
fn a_missing_file_is_the_defaults_and_a_present_one_is_read() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert_eq!(
        Config::load(dir.path()).expect("a missing file is legal"),
        Config::default()
    );
    std::fs::write(dir.path().join(FILE_NAME), "[reports]\nkeep = 3\n").expect("write");
    assert_eq!(Config::load(dir.path()).expect("read").reports.keep, 3);

    std::fs::write(dir.path().join(FILE_NAME), "version = 2\n").expect("write");
    let error = Config::load(dir.path()).expect_err("refused");
    assert!(
        error.to_string().contains(FILE_NAME),
        "the error names the file: {error}"
    );
}

#[test]
fn a_directory_a_command_names_is_resolved_against_where_the_command_was_told_it_is() {
    use std::path::{Path, PathBuf};

    use mjutest_cli::cli::Environment;
    use rust_mutants::runner::Cancel;

    let environment = Environment {
        vars: Vec::new(),
        working_directory: PathBuf::from("/somewhere/a/caller/named"),
        temp_directory: PathBuf::from("/tmp"),
        cache_directory: PathBuf::from("/tmp/cache"),
        cancel: Cancel::new(),
    };

    assert_eq!(
        environment.rooted(None),
        Path::new("/somewhere/a/caller/named"),
        "a command that names no directory is about the one it was told it is in"
    );
    assert_eq!(
        environment.rooted(Some(Path::new("."))),
        Path::new("/somewhere/a/caller/named/."),
        "and a dot is that directory rather than the process's own: a caller that says \
         where it is and then gets an answer about somewhere else has been told about a \
         tree it did not name"
    );
    assert_eq!(
        environment.rooted(Some(Path::new("crates/core"))),
        Path::new("/somewhere/a/caller/named/crates/core"),
        "a relative name is relative to that one"
    );
    assert_eq!(
        environment.rooted(Some(Path::new("/elsewhere/entirely"))),
        Path::new("/elsewhere/entirely"),
        "and an absolute name is the tree it names, wherever the caller is"
    );
}
