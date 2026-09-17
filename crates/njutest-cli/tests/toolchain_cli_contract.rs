// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The command-line contract of the `njutest` binary: what `--version` and `--help` print, and the exit code of a usage error, which is `3` — the code of invalid input — and not clap's default.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and asserts with panics"
)]

use std::ffi::OsString;
use std::path::Path;
use std::process::Output;

use njutest_cli::cli::Environment;
use rust_mutants::runner::Cancel;

fn njutest(args: &[&str]) -> Output {
    let here = std::env::current_dir().unwrap_or_else(|_error| Path::new(".").to_path_buf());
    asked(&environment(&here, &[]), args)
}

/// The same, in a directory of its own, for a command that writes.
fn njutest_in(dir: &Path, args: &[&str]) -> Output {
    asked(&environment(dir, &[]), args)
}

/// One command, driven in this process against an environment a test composed.
fn asked(environment: &Environment, args: &[&str]) -> Output {
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = njutest_cli::run_from(
        std::iter::once("njutest")
            .chain(args.iter().copied())
            .map(OsString::from),
        environment,
        &mut out,
        &mut err,
    );
    njutest_devkit::process::answered(code, out, err)
}

/// The environment a run of this suite composes: the four variables a toolchain needs, and nothing else.
fn environment(directory: &Path, named: &[(&str, &str)]) -> Environment {
    let mut vars: Vec<(OsString, OsString)> =
        njutest_devkit::paths::environment_for_a_toolchain_run(&[]);
    for (name, value) in named {
        vars.retain(|(held, _)| {
            !njutest_devkit::paths::same_name(held, OsString::from(name).as_os_str())
        });
        vars.push((OsString::from(name), OsString::from(value)));
    }
    Environment {
        cache_directory: directory.join("njutest-cache"),
        working_directory: directory.to_path_buf(),
        temp_directory: directory.join("njutest-temp"),
        vars,
        cancel: Cancel::new(),
    }
}

fn golden_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/testdata/{name}"))
}

#[test]
fn version_flag_prints_the_binary_name_and_its_version() {
    let output = njutest(&["--version"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("njutest {}\n", njutest_cli::VERSION)
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_flag_matches_the_recorded_help_text() {
    let output = njutest(&["--help"]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    njutest_devkit::golden::golden(&golden, &output.stdout).expect("help text is the recorded one");
}

#[test]
fn a_bare_invocation_prints_the_help_to_stdout_and_exits_0() {
    let output = njutest(&[]);
    assert_eq!(output.status.code(), Some(0));
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/help.golden");
    njutest_devkit::golden::golden(&golden, &output.stdout).expect("bare invocation is the help");
}

#[test]
fn an_unknown_subcommand_is_invalid_input_and_exits_3() {
    let output = njutest(&["frobnicate"]);
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.starts_with("njutest: "),
        "diagnostics carry the program prefix: {stderr}"
    );
    assert!(
        stderr.contains("frobnicate"),
        "names the offending argument: {stderr}"
    );
}

#[test]
fn an_unknown_flag_is_invalid_input_and_exits_3() {
    let output = njutest(&["--no-such-flag"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--no-such-flag"), "{stderr}");
}

/// Every command the top-level help lists, which is every command there is.
fn subcommands() -> Vec<String> {
    let help = String::from_utf8_lossy(&njutest(&["--help"]).stdout).into_owned();
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
        let name = name.as_str();
        let output = njutest(&[name, "--help"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        njutest_devkit::golden::golden(
            &golden_path(&format!("help-{name}.golden")),
            &output.stdout,
        )
        .unwrap_or_else(|error| panic!("{name}: {error}"));
    }
}

#[test]
fn every_command_the_help_lists_is_one_the_program_answers_to() {
    for name in subcommands() {
        let output = njutest(&[name.as_str(), "--help"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "the help offers {name} and the program does not take it: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn a_subcommand_given_a_flag_it_does_not_know_is_invalid_input() {
    let output = njutest(&["init", "--no-such-flag"]);
    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.starts_with("njutest: "), "{stderr}");
    assert!(stderr.contains("--no-such-flag"), "{stderr}");
}

#[test]
fn doctor_names_every_tool_a_run_needs_and_whether_it_is_there() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = asked(&environment(dir.path(), &[]), &["doctor"]);
    let stdout = String::from_utf8_lossy(&output.stdout);

    for tool in ["cargo", "rustc", "llvm-profdata", "llvm-cov", "git"] {
        assert!(stdout.contains(tool), "{tool} is not reported: {stdout}");
    }
    assert!(
        stdout.contains("required") && stdout.contains("optional"),
        "a reader must be able to tell what a missing line costs: {stdout}"
    );
    assert!(
        matches!(output.status.code(), Some(0 | 3)),
        "0 when a run could go ahead, 3 when it could not: {:?}",
        output.status.code()
    );
}

#[test]
fn doctor_reads_the_configuration_a_run_would_read() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    std::fs::write(
        dir.path().join(".njutest.toml"),
        "version = 1\n\n[project]\nexclude = [\"vendor/\"]\n",
    )
    .expect("a configuration");

    let output = asked(&environment(dir.path(), &[]), &["doctor"]);
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_ne!(
        output.status.code(),
        Some(0),
        "a doctor says whether a run can go ahead here, and a run in this directory \
         refuses the configuration before it does anything else. Saying a run can go \
         ahead is a claim the next command contradicts: {stdout}"
    );
    assert!(
        stdout.contains(".njutest.toml"),
        "and it names the file it could not read: {stdout}"
    );
}

/// A directory holding only what a run requires, so every optional tool is out of reach.
fn only_what_is_required(dir: &Path, said: &str) -> std::path::PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).expect("a directory");
    for tool in ["cargo", "rustc"] {
        let found = said
            .lines()
            .find(|line| line.contains(&format!(" {tool} ")))
            .and_then(|line| line.split_whitespace().next_back())
            .unwrap_or_else(|| panic!("the doctor says where {tool} is: {said}"));
        place_tool(found, &bin, tool);
    }
    bin
}

#[cfg(unix)]
fn place_tool(found: &str, bin: &Path, tool: &str) {
    std::os::unix::fs::symlink(found, bin.join(tool)).expect("a link");
}

#[cfg(not(unix))]
fn place_tool(found: &str, bin: &Path, tool: &str) {
    let name = format!("{tool}{}", std::env::consts::EXE_SUFFIX);
    let _copied = std::fs::copy(found, bin.join(name)).expect("a copy");
}

#[test]
fn doctor_says_a_run_can_go_ahead_when_only_the_optional_tools_are_missing() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let found = asked(&environment(dir.path(), &[]), &["doctor"]);
    let bin = only_what_is_required(dir.path(), &String::from_utf8_lossy(&found.stdout));

    let output = asked(
        &environment(dir.path(), &[("PATH", &bin.to_string_lossy())]),
        &["doctor"],
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        output.status.code(),
        Some(0),
        "an optional tool is one a run says a limitation about rather than one it needs,          so a machine with every required tool and no optional one can still verify:          {stdout}"
    );
    assert!(
        stdout.ends_with("\n\na standard-v1 run can go ahead on this machine\n"),
        "and the doctor says so in the last line, set off from the table by a blank one: \
         {stdout:?}"
    );
    assert!(
        stdout
            .lines()
            .any(|line| line.starts_with("optional") && line.contains("missing")),
        "with the ones that are not there named as missing: {stdout}"
    );
}

#[test]
fn doctor_without_a_toolchain_says_so_and_refuses_rather_than_guessing() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = asked(
        &environment(dir.path(), &[("PATH", "/nonexistent")]),
        &["doctor"],
    );
    assert_eq!(
        output.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("missing"), "{stdout}");
    assert!(
        stdout.contains("a run cannot go ahead: cargo is missing"),
        "and says which of them a run needs, because that is the line a reader acts on: \
         {stdout}"
    );
}

#[test]
fn init_writes_a_skeleton_that_loads_as_exactly_the_defaults() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let output = njutest_in(dir.path(), &["init"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let path = dir.path().join(njutest_cli::config::FILE_NAME);
    let written = std::fs::read_to_string(&path).expect("the skeleton");
    assert_eq!(written, njutest_cli::config::skeleton());
    assert_eq!(
        njutest_cli::config::Config::load(dir.path()).expect("it loads"),
        njutest_cli::config::Config::default(),
        "the untouched skeleton is the defaults, written down"
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(njutest_cli::config::FILE_NAME),
        "it says what it wrote"
    );
}

#[test]
fn init_refuses_to_write_over_a_configuration_somebody_edited() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(njutest_cli::config::FILE_NAME);
    std::fs::write(&path, "version = 1\n").expect("their configuration");

    let output = njutest_in(dir.path(), &["init"]);
    assert_eq!(output.status.code(), Some(3));
    assert_eq!(
        std::fs::read_to_string(&path).expect("still there"),
        "version = 1\n",
        "refused without touching it"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("NJ1005"), "{stderr}");
    assert!(
        stderr.contains("--force"),
        "and says how to mean it: {stderr}"
    );
}

#[test]
fn init_force_replaces_what_is_there() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let path = dir.path().join(njutest_cli::config::FILE_NAME);
    std::fs::write(&path, "version = 1\n").expect("their configuration");

    let output = njutest_in(dir.path(), &["init", "--force"]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        std::fs::read_to_string(&path).expect("the skeleton"),
        njutest_cli::config::skeleton()
    );
}

#[test]
fn cargo_njutest_drops_the_word_cargo_gave_it_and_reads_the_rest() {
    let asked = njutest_cli::cli::parse(
        ["cargo-njutest", "njutest", "verify", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("cargo calls its subcommands with their own name in argv[1]");
    let direct = njutest_cli::cli::parse(
        ["njutest", "verify", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("and a person calls it without");

    assert_eq!(
        format!("{asked:?}"),
        format!("{direct:?}"),
        "`cargo njutest verify` and `njutest verify` are one command asked for two ways"
    );
}

#[test]
fn a_binary_that_is_not_a_cargo_subcommand_keeps_every_argument_it_was_given() {
    let refused = njutest_cli::cli::parse(
        ["njutest", "njutest", "verify"]
            .into_iter()
            .map(OsString::from),
    );
    assert!(
        refused.is_err(),
        "dropping a repeated word is cargo's convention and not this program's: called \
         directly, `njutest njutest verify` is a mistake and is said to be one"
    );
}

#[test]
fn the_word_cargo_repeats_is_dropped_once_and_only_where_it_is_the_subcommand() {
    let twice = njutest_cli::cli::parse(
        ["cargo-njutest", "njutest", "njutest", "verify"]
            .into_iter()
            .map(OsString::from),
    );
    assert!(
        twice.is_err(),
        "one repeat is cargo's; a second is a mistake, and dropping both would run a \
         command nobody asked for"
    );

    let elsewhere = njutest_cli::cli::parse(
        ["cargo-njutest", "verify", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("cargo may be called without the repeat, and the rest is still the command");
    let direct = njutest_cli::cli::parse(
        ["njutest", "verify", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("which is this command");
    assert_eq!(format!("{elsewhere:?}"), format!("{direct:?}"));
}
