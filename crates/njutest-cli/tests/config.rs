// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `.njutest.toml`: optional, strict, and defaulted.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::time::Duration;

use njutest_cli::config::{
    Acceptance, Config, ConfigErrorKind, Contract, DEFAULT_CACHE_MAX_BYTES, DEFAULT_CACHE_TTL,
    DEFAULT_REPORTS_KEEP, DEFAULT_TIMEOUT, FILE_NAME, parse_duration, skeleton,
};

fn load(text: &str) -> Result<Config, njutest_cli::config::ConfigError> {
    Config::parse(text, std::path::Path::new(".njutest.toml"))
}

fn expect_error(text: &str) -> njutest_cli::config::ConfigError {
    load(text).expect_err("this configuration is refused")
}

fn expect_ok(text: &str) -> Config {
    load(text).expect("this configuration is read")
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
    assert_eq!(config.reports.keep, DEFAULT_REPORTS_KEEP);
    assert!(!config.fuzz.run);
    assert_eq!(config.fuzz.max_total_time, Duration::from_secs(60));
    assert!(config.fuzz.targets.is_empty());
    assert_eq!(FILE_NAME, ".njutest.toml");
}

#[test]
fn the_defaults_ask_for_nothing_a_run_has_to_be_told() {
    let config = Config::default();
    let empty: [(&str, bool); 9] = [
        ("packages", config.project.packages.is_empty()),
        ("exclude", config.project.exclude.is_empty()),
        ("features", config.execution.features.is_empty()),
        (
            "test_binary_args",
            config.execution.test_binary_args.is_empty(),
        ),
        ("environment", config.execution.environment.is_empty()),
        ("skip_targets", config.execution.skip_targets.is_empty()),
        ("miri_flags", config.soundness.miri_flags.is_empty()),
        ("sanitizers", config.soundness.sanitizers.is_empty()),
        ("acceptance", config.acceptance.is_empty()),
    ];
    for (name, is_empty) in empty {
        assert!(is_empty, "{name} is not empty by default");
    }
    assert!(!config.execution.all_features);
    assert!(!config.execution.no_default_features);
    assert_eq!(config.verification.unwind, None);
    assert_eq!(config.verification.timeout, None);
    assert!(config.resources.is_empty());
    assert!(config.generation.is_none());
}

#[test]
fn how_long_a_measurement_may_take_is_not_how_long_a_build_may_take() {
    let config = load("version = 1\n\n[execution]\ntimeout = \"2s\"\n").expect("a configuration");
    assert_eq!(config.execution.timeout, Duration::from_secs(2));
    assert_eq!(
        config.execution.build_timeout, None,
        "a project that said how long it waits for one mutation has not said how long \
         its own compiler may take, and bounding the build by the same number is how a \
         tight bound turns every run into RM1014 on a machine that was busy"
    );

    let bounded =
        load("version = 1\n\n[execution]\nbuild_timeout = \"5m\"\n").expect("a configuration");
    assert_eq!(
        bounded.execution.build_timeout,
        Some(Duration::from_secs(300)),
        "and a project that does want one says so in its own key"
    );
    assert_eq!(
        load("version = 1\n\n[execution]\nbuild_timeout = \"\"\n")
            .expect("a configuration")
            .execution
            .build_timeout,
        None,
        "where empty is the same as saying nothing, which is what the skeleton shows"
    );
}

#[test]
fn an_empty_file_and_the_written_skeleton_both_mean_the_defaults() {
    let written = skeleton();
    assert_eq!(load("").expect("empty is legal"), Config::default());
    assert_eq!(
        load(&written).expect("the skeleton loads"),
        Config::default(),
        "the untouched skeleton is exactly the defaults"
    );
    for section in [
        "[project]",
        "[execution]",
        "[cache]",
        "[reports]",
        "[verification]",
        "[soundness]",
        "[resources.",
        "[generation]",
        "[[acceptance]]",
    ] {
        assert!(written.contains(section), "{section} is missing");
    }
    assert!(written.starts_with("# "), "the skeleton explains itself");
    for exact_default in [
        "# timeout = \"10m\"",
        "# max_bytes = 5368709120",
        "# ttl = \"720h\"",
        "# keep = 20",
        "# max_total_time = \"60s\"",
    ] {
        assert!(
            written.contains(exact_default),
            "the rendered default is part of the init contract: {exact_default}"
        );
    }
}

#[test]
fn an_unknown_key_is_an_error_that_names_it() {
    let error = expect_error("version = 1\nunknown = true\n");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("unknown"), "{error}");
    assert!(error.to_string().contains("NJ1002"), "{error}");

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
fn the_contract_is_one_of_three_names() {
    assert_eq!(
        load("contract = \"deep-v1\"\n").expect("deep").contract,
        Contract::DeepV1
    );
    assert_eq!(
        load("contract = \"verified-v1\"\n\n[verification]\nunwind = 8\ntimeout = \"30s\"\n")
            .expect("verified")
            .contract,
        Contract::VerifiedV1
    );
    let error = expect_error("contract = \"strict-v9\"\n");
    assert_eq!(error.kind(), ConfigErrorKind::Unparsable);
    assert!(error.to_string().contains("strict-v9"), "{error}");
}

#[test]
fn verified_v1_requires_both_nonzero_bounds_and_reifies_them() {
    let config =
        expect_ok("contract = \"verified-v1\"\n\n[verification]\nunwind = 12\ntimeout = \"45s\"\n");
    let verified = config
        .verified()
        .expect("valid verifier settings")
        .expect("verified contract");
    assert_eq!(verified.unwind().get(), 12);
    assert_eq!(verified.timeout(), Duration::from_secs(45));
    assert_eq!(verified.timeout_ms().get(), 45_000);

    for refused in [
        "contract = \"verified-v1\"\n",
        "contract = \"verified-v1\"\n[verification]\ntimeout = \"1s\"\n",
        "contract = \"verified-v1\"\n[verification]\nunwind = 1\n",
        "contract = \"verified-v1\"\n[verification]\nunwind = 0\ntimeout = \"1s\"\n",
        "contract = \"verified-v1\"\n[verification]\nunwind = 1\ntimeout = \"0s\"\n",
        "contract = \"verified-v1\"\n[verification]\nunwind = 1\ntimeout = \"1ns\"\n",
    ] {
        let error = expect_error(refused);
        assert_eq!(error.kind(), ConfigErrorKind::Invalid, "{refused}: {error}");
    }
}

#[test]
fn verifier_only_keys_are_not_silently_ignored_by_other_contracts() {
    for contract in ["standard-v1", "deep-v1"] {
        let text =
            format!("contract = {contract:?}\n\n[verification]\nunwind = 8\ntimeout = \"30s\"\n");
        let error = expect_error(&text);
        assert_eq!(error.kind(), ConfigErrorKind::Invalid, "{error}");
        assert!(error.to_string().contains("verified-v1"), "{error}");
    }
    assert_eq!(
        Config::default().verified().expect("default contract"),
        None
    );
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

    let wrong_type = expect_error("[execution]\ntimeout = 90\n");
    assert_eq!(wrong_type.kind(), ConfigErrorKind::Unparsable);
    assert!(wrong_type.to_string().contains("timeout"), "{wrong_type}");
}

#[test]
fn parse_errors_are_one_line_without_losing_their_boundaries() {
    let error = expect_error("[execution]\ntimeout = [\n");
    let rendered = error.to_string();
    assert!(!rendered.contains('\n'), "{rendered:?}");
    assert!(
        rendered.contains("; "),
        "folded source lines retain an unambiguous separator: {rendered}"
    );
}

#[test]
fn the_harness_flags_njutest_owns_are_refused_and_the_rest_pass() {
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
        let error = load(&text).expect_err("njutest owns this flag");
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
skip_targets = ["fixture-app/test/cli"]

[cache]
max_bytes = 1024
ttl = "24h"

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
    assert_eq!(config.execution.skip_targets, ["fixture-app/test/cli"]);
    assert_eq!(config.cache.max_bytes, 1024);
    assert_eq!(config.cache.ttl, Duration::from_hours(24));
    assert_eq!(config.reports.keep, 5);
    assert_eq!(config.soundness.sanitizers, ["thread"]);

    assert_documented_extras(&config);
}

#[test]
fn omitted_resource_fields_have_the_documented_defaults() {
    let config = load("[resources.db]\ncommand = [\"provider\"]\n").expect("resource");
    let resource = config.resources.get("db").expect("db");
    assert_eq!(resource.timeout, Duration::from_secs(30));
    assert!(!resource.shared);
    assert!(!resource.exclusive);
    assert!(resource.environment.is_empty());
}

#[test]
fn an_acceptance_expires_at_the_instant_it_names() {
    let boundary = jiff::Timestamp::from_second(1_800_000_000).expect("in range");
    let before = jiff::Timestamp::from_second(1_799_999_999).expect("in range");
    let after = jiff::Timestamp::from_second(1_800_000_001).expect("in range");
    let expiring = Acceptance {
        id: "0123456789abcdef".to_owned(),
        path: None,
        item: None,
        rule: None,
        original: None,
        line: None,
        reason: "reviewed".to_owned(),
        expires: Some(boundary),
        owner: None,
        ticket: None,
    };
    assert!(expiring.holds(before));
    assert!(!expiring.holds(boundary));
    assert!(!expiring.holds(after));

    let forever = Acceptance {
        expires: None,
        ..expiring
    };
    assert!(forever.holds(after));
}

#[test]
fn retired_build_cache_keys_are_refused_with_the_migration_in_their_names() {
    for (name, value) in [
        ("build_max_bytes", "2048"),
        ("build_dir", "\"/var/cache/njutest\""),
    ] {
        let text = format!("[cache]\n{name} = {value}\n");
        let error = expect_error(&text);
        assert_eq!(error.kind(), ConfigErrorKind::Unparsable, "{name}: {error}");
        assert!(
            error.to_string().contains(name),
            "the obsolete key is named so a reader can remove it: {error}"
        );
    }
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
fn a_configuration_is_a_named_way_to_build_the_project_and_is_named_once() {
    let read = expect_ok(
        "[[configuration]]\nname = \"all-features\"\nall_features = true\n\n\
         [[configuration]]\nname = \"release\"\nprofile = \"release\"\n",
    );
    assert_eq!(
        read.configuration.len(),
        2,
        "a project whose tests pass with the default features and break with all of \
         them has a hole nothing reports, because a run measures one build"
    );
    assert_eq!(read.configuration[0].name, "all-features");
    assert!(read.configuration[0].all_features);
    assert_eq!(read.configuration[1].profile.as_deref(), Some("release"));

    assert_eq!(
        Config::default().configuration,
        Vec::new(),
        "and a project that named none is measured exactly as it was before: the \
         second build is a thing somebody asks for"
    );

    let blank = expect_error("[[configuration]]\nname = \"\"\n");
    assert_eq!(blank.kind(), ConfigErrorKind::Invalid);

    let twice =
        expect_error("[[configuration]]\nname = \"same\"\n\n[[configuration]]\nname = \"same\"\n");
    assert_eq!(
        twice.kind(),
        ConfigErrorKind::Invalid,
        "two configurations under one name are two answers a report cannot tell apart"
    );
    assert!(twice.to_string().contains("same"), "{twice}");

    let reserved = expect_error("[[configuration]]\nname = \"default\"\n");
    assert_eq!(
        reserved.kind(),
        ConfigErrorKind::Invalid,
        "`default` is what the report calls the build `[execution]` describes, so a \
         second one under that name would overwrite the first"
    );
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
fn an_unreadable_configuration_is_not_mistaken_for_an_absent_one() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join(FILE_NAME);
    std::fs::create_dir_all(&path).expect("directory at the configuration path");
    let error = Config::load(dir.path()).expect_err("a directory is not an absent file");
    assert_eq!(error.kind(), ConfigErrorKind::Unreadable);
    assert!(error.to_string().contains(FILE_NAME), "{error}");
}

#[test]
fn canonical_configuration_is_the_complete_serialized_contract() {
    let config = Config::default();
    let canonical = config.canonical().expect("canonical configuration");
    assert_eq!(
        canonical,
        serde_json::to_string(&config).expect("the configuration is serializable")
    );
    assert!(canonical.starts_with("{\"version\":1,"), "{canonical}");
    assert!(canonical.ends_with("\"configuration\":[]}"), "{canonical}");
    assert_eq!(config.digest().expect("configuration digest").len(), 64);

    let mut two = Config::default();
    two.configuration.push(njutest_cli::config::Configuration {
        name: "release".to_owned(),
        profile: Some("release".to_owned()),
        ..njutest_cli::config::Configuration::default()
    });
    assert_ne!(
        two.digest().expect("second configuration digest"),
        config.digest().expect("first configuration digest"),
        "a run that measures a second build is a different run, and the identity has \
         to say so or the first run's answers would be read back for it"
    );
}

#[test]
fn a_directory_a_command_names_is_resolved_against_where_the_command_was_told_it_is() {
    use std::path::{Path, PathBuf};

    use njutest_cli::cli::Environment;
    use rust_mutants::runner::Cancel;

    let environment = Environment {
        vars: Vec::new(),
        working_directory: PathBuf::from("/somewhere/a/caller/named"),
        temp_directory: PathBuf::from("/tmp"),
        program: PathBuf::from("this test never runs it"),
        cache_directory: PathBuf::from("/tmp/cache"),
        cancel: Cancel::new(),
        terminal: njutest_cli::presentation::Terminal::default(),
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

#[test]
fn an_exclude_pattern_that_is_not_a_pattern_is_refused() {
    let good = load("[project]\nexclude = [\"vendor/**\", \"**/generated/**\"]\n")
        .expect("two patterns that compile");
    assert_eq!(good.project.exclude, ["vendor/**", "**/generated/**"]);

    for (refused, why) in [
        (
            "vendor/",
            "a trailing slash is the spelling a person reaches for first",
        ),
        ("/vendor/**", "a leading slash is the second"),
        ("", "and an empty pattern narrows nothing at all"),
    ] {
        let error = expect_error(&format!("[project]\nexclude = [{refused:?}]\n"));
        assert_eq!(error.kind(), ConfigErrorKind::Invalid, "{why}: {refused:?}");
        assert!(
            error.to_string().contains("exclude"),
            "a refusal names the key it is about: {error}"
        );
    }
}
