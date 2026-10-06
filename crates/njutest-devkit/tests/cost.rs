// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The cost publisher's records, read back as the complete protocol a reader sees.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking and reads as a table"
)]

use std::path::PathBuf;

/// The private marker a parent test sets to let a child-side helper case run.
const CHILD: &str = "NJUTEST_TEST_COST_CHILD";

/// Every work field the reader requires of a record, by name.
const REQUIRED: [&str; 24] = [
    "builds",
    "build_ms",
    "units",
    "build_requests",
    "build_hits",
    "build_misses",
    "build_keys",
    "unbound",
    "launch_failures",
    "observed_cargo_starts",
    "cargo_probes",
    "cargo_probe_ms",
    "cargo_metadata",
    "cargo_metadata_ms",
    "rustc_probes",
    "rustc_probe_ms",
    "rustc_builds",
    "rustc_build_ms",
    "executions",
    "probes",
    "host_waits",
    "unobserved_cargo",
    "platform",
    "platform_requests",
];

fn one_record(directory: &std::path::Path) -> serde_json::Value {
    let mut records: Vec<PathBuf> = std::fs::read_dir(directory)
        .expect("the cost directory")
        .map(|entry| entry.expect("one record").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    records.sort();
    assert_eq!(records.len(), 1, "exactly one record: {records:?}");
    njutest_devkit::strictjson::decode_slice(
        &std::fs::read(records.first().expect("the one")).expect("the record"),
    )
    .expect("a complete record")
}

fn the_work(record: &serde_json::Value) -> &serde_json::Value {
    record.get("work").expect("the work block")
}

fn count(work: &serde_json::Value, name: &str) -> u64 {
    work.get(name)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| panic!("the count {name} is present and measured"))
}

fn run_child(test: &str, directory: &std::path::Path) {
    let status = std::process::Command::new(std::env::current_exe().expect("this binary"))
        .args(["--exact", test, "--nocapture"])
        .env(CHILD, "1")
        .env("NJUTEST_TEST_COST_DIR", directory)
        .status()
        .expect("the child test runs");
    assert!(status.success(), "the child test {test} passed: {status}");
}

/// Gives the suite the child's measured work under this test's own nextest identity, exactly once.
fn owned_by_suite(record: &serde_json::Value) {
    if std::env::var_os("NJUTEST_TEST_COST_DIR").is_none() {
        return;
    }
    let root = record
        .get("root")
        .and_then(serde_json::Value::as_str)
        .expect("the child's measured root");
    njutest_devkit::cost::record(
        std::path::Path::new(root),
        record.get("work").expect("the child's measured work"),
        record.get("sealed").expect("the child's sealed work"),
    )
    .expect("the suite record of the child's work");
}

#[test]
fn a_real_direct_build_publishes_every_field_the_reader_requires() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_real_build", guard.path());
    let record = one_record(guard.path());
    owned_by_suite(&record);
    let work = the_work(&record);
    let absent: Vec<&str> = REQUIRED
        .iter()
        .filter(|name| work.get(**name).is_none_or(serde_json::Value::is_null))
        .copied()
        .collect();
    assert!(
        absent.is_empty(),
        "a published work payload carries every required field: absent {absent:?}: {work}"
    );
    let origin = record.get("origin").expect("the actual suite origin");
    assert_eq!(
        origin.get("kind").and_then(serde_json::Value::as_str),
        Some("suite")
    );
    assert_eq!(origin.get("binary"), record.get("binary"));
    assert_eq!(origin.get("test"), record.get("test"));
    let machine = record.get("machine").expect("the executing machine");
    assert_eq!(
        machine.get("os").and_then(serde_json::Value::as_str),
        Some(std::env::consts::OS)
    );
    assert_eq!(
        machine.get("arch").and_then(serde_json::Value::as_str),
        Some(std::env::consts::ARCH)
    );
    assert!(
        machine
            .get("cpus")
            .and_then(serde_json::Value::as_u64)
            .is_some_and(|cpus| cpus > 0)
    );
    assert_eq!(count(work, "observed_cargo_starts"), 1);
    assert_eq!(count(work, "builds"), 1);
    assert_eq!(count(work, "build_requests"), 1);
    assert_eq!(count(work, "launch_failures"), 0);
    assert!(
        count(work, "units") >= 1,
        "an actual build derives its artifacts from the Cargo JSON stream it observed: {work}"
    );
}

#[test]
fn actual_toolchain_probes_keep_their_role_and_one_execution() {
    for (role, count_field, execution_role) in [
        ("cargo-banner", "cargo_probes", "cargo-probe"),
        ("rustc-cfg", "rustc_probes", "rustc-probe"),
        ("rustc-build", "rustc_builds", "rustc-build"),
    ] {
        let guard = tempfile::tempdir().expect("a probe cost directory");
        let status = std::process::Command::new(std::env::current_exe().expect("this binary"))
            .args(["--exact", "cost::cost_child_actual_probe", "--nocapture"])
            .env(CHILD, "1")
            .env("NJUTEST_TEST_COST_DIR", guard.path())
            .env("NJUTEST_TEST_PROBE_ROLE", role)
            .status()
            .expect("the actual probe child");
        assert!(status.success(), "the actual {role} succeeded: {status}");
        let record = one_record(guard.path());
        owned_by_suite(&record);
        let work = the_work(&record);
        assert_eq!(count(work, count_field), 1, "the actual {role}");
        let executions = work
            .get("executions")
            .and_then(serde_json::Value::as_object)
            .expect("the complete actual execution inventory");
        assert_eq!(executions.len(), 1, "one real process, recorded once");
        let executed = executions.values().next().expect("the actual execution");
        assert_eq!(
            executed.get("role").and_then(serde_json::Value::as_str),
            Some(execution_role)
        );
        assert_eq!(
            executed
                .get("processes")
                .and_then(serde_json::Value::as_u64),
            Some(1)
        );
        let probes = work
            .get("probes")
            .and_then(serde_json::Value::as_object)
            .expect("the keyed actual probe");
        assert_eq!(probes.len(), 1);
        assert_eq!(
            probes
                .values()
                .next()
                .and_then(|probe| probe.get("role"))
                .and_then(serde_json::Value::as_str),
            Some(role)
        );
    }
}

#[test]
fn cost_child_actual_probe() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let directory = std::env::var_os("NJUTEST_TEST_COST_DIR").expect("a cost directory");
    let directory = std::path::Path::new(&directory);
    let cargo = njutest_devkit::paths::cargo_binary();
    let rustc = cargo.with_file_name(if cfg!(windows) { "rustc.exe" } else { "rustc" });
    let role = std::env::var("NJUTEST_TEST_PROBE_ROLE").expect("the actual probe role");
    let (mut command, role) = match role.as_str() {
        "cargo-banner" => {
            let mut command = std::process::Command::new(cargo);
            command.arg("--version");
            (command, njutest_devkit::cost::ProbeRole::CargoBanner)
        }
        "rustc-cfg" => {
            let mut command = std::process::Command::new(rustc);
            command.args(["--print", "cfg"]);
            (command, njutest_devkit::cost::ProbeRole::RustcCfg)
        }
        "rustc-build" => {
            let source = directory.join("probe.rs");
            std::fs::write(&source, "pub fn actual() -> u8 { 1 }\n")
                .expect("the actual compiler input");
            let mut command = std::process::Command::new(rustc);
            command
                .args(["--crate-type=lib", "--emit=metadata", "--out-dir"])
                .arg(directory)
                .arg(source);
            (command, njutest_devkit::cost::ProbeRole::RustcBuild)
        }
        other => panic!("unknown actual probe role {other}"),
    };
    command.current_dir(directory);
    let output = njutest_devkit::cost::probe(command, role, "an actual observed toolchain probe")
        .expect("the actual probe executes");
    assert!(
        output.status.success(),
        "the actual tool succeeds: {:?}",
        output.stderr
    );
}

#[test]
fn a_failed_direct_attempt_closes_without_asking_the_cache() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_direct_failed", guard.path());
    let record = one_record(guard.path());
    owned_by_suite(&record);
    let work = the_work(&record);
    let held = work
        .get("unbound")
        .and_then(serde_json::Value::as_object)
        .and_then(|unbound| unbound.values().next())
        .expect("the attempt's counts");
    let said = |name: &str| held.get(name).and_then(serde_json::Value::as_u64);
    assert_eq!(
        said("misses"),
        Some(0),
        "direct work never asks the cache, so a failed start misses nothing: {work}"
    );
    assert_eq!(said("requests"), Some(1));
    assert_eq!(said("failed_launches"), Some(1));
    assert_eq!(said("processes"), Some(0));
    assert_eq!(
        count(work, "build_misses"),
        0,
        "the suite-level miss count closes too"
    );
}

#[test]
fn a_direct_build_that_cannot_start_keeps_its_attempted_work() {
    let guard = tempfile::tempdir().expect("a cost directory");
    run_child("cost::cost_child_direct_failed", guard.path());
    let record = one_record(guard.path());
    owned_by_suite(&record);
    let work = the_work(&record);
    let absent: Vec<&str> = REQUIRED
        .iter()
        .filter(|name| work.get(**name).is_none_or(serde_json::Value::is_null))
        .copied()
        .collect();
    assert!(
        absent.is_empty(),
        "a failed attempt publishes every required field too: absent {absent:?}: {work}"
    );
    let identity = work
        .get("unbound")
        .and_then(serde_json::Value::as_object)
        .and_then(|unbound| unbound.keys().next())
        .cloned()
        .expect("the attempt's identity");
    assert_eq!(
        identity, "direct: a direct build that cannot start",
        "the failed launch keeps exactly its request's identity"
    );
    let held = work
        .get("unbound")
        .and_then(|unbound| unbound.get(&identity))
        .expect("the attempt's counts");
    let said = |name: &str| held.get(name).and_then(serde_json::Value::as_u64);
    assert_eq!(said("requests"), Some(1), "the attempted request is kept");
    assert_eq!(
        said("misses"),
        Some(0),
        "direct work never asks the cache, so a failed start misses nothing"
    );
    assert_eq!(said("failed_launches"), Some(1));
    assert_eq!(said("processes"), Some(0));
    assert_eq!(count(work, "builds"), 0);
    assert_eq!(count(work, "build_misses"), 0);
    assert_eq!(count(work, "launch_failures"), 1);
    assert_eq!(count(work, "observed_cargo_starts"), 0);
    assert_eq!(
        count(work, "units"),
        0,
        "a never-started attempt proves zero artifacts"
    );
}

#[test]
fn cost_child_real_build() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let directory = std::env::var_os("NJUTEST_TEST_COST_DIR").expect("a cost directory");
    let root = std::path::Path::new(&directory).join("project");
    std::fs::create_dir_all(root.join("src")).expect("the project directory");
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"cost-direct-build\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\n\
         [lib]\npath = \"src/lib.rs\"\n\n[workspace]\n",
    )
    .expect("the manifest");
    std::fs::write(root.join("src/lib.rs"), "pub fn one() -> u8 {\n    1\n}\n")
        .expect("the library");
    std::fs::write(
        root.join("Cargo.lock"),
        "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"cost-direct-build\"\nversion = \"0.0.0\"\n",
    )
    .expect("the lock file");
    let mut command = std::process::Command::new(njutest_devkit::paths::cargo_binary());
    command
        .args(["build", "--offline", "--locked"])
        .current_dir(&root);
    let built =
        njutest_devkit::cost::cargo(command, "a real direct build of a tiny offline project")
            .expect("the build runs");
    assert!(
        built.status.success(),
        "the real build succeeds:\n{:?}\n{:?}",
        built.stdout,
        built.stderr
    );
}

#[test]
fn cost_child_direct_failed() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let directory = std::env::var_os("NJUTEST_TEST_COST_DIR").expect("a cost directory");
    let mut command =
        std::process::Command::new(std::path::Path::new(&directory).join("no-such-cargo"));
    command
        .arg("--version")
        .current_dir(std::path::Path::new(&directory));
    let refused = njutest_devkit::cost::cargo(command, "a direct build that cannot start");
    assert!(refused.is_err(), "the launch itself fails");
}
