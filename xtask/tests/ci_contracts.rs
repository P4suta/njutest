// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Toolchain identities and recoverable test evidence are CI contracts.

#![expect(
    clippy::expect_used,
    reason = "repository contracts fail with their missing input"
)]

use std::path::Path;

fn read(path: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(path))
        .expect("the repository contract input")
}

fn floating(source: &str) -> bool {
    let words = regex::Regex::new(r#"(?:cargo\s+\+["']?[a-zA-Z0-9]|^\s*(?:RUSTUP_TOOLCHAIN|toolchain):\s*["']?[a-zA-Z0-9]|toolchain install\s+["']?[^$%"'\s]+|--toolchain\s+["']?[^$%"'\s]+|actions-rs/toolchain@)"#)
        .expect("the toolchain command grammar");
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .any(|line| words.is_match(line))
}

#[test]
fn every_installed_toolchain_is_read_from_one_dated_pin_file() {
    let pins: toml::Value = toml::from_str(&read("rust-toolchain.toml")).expect("toolchain TOML");
    let stable = pins
        .get("toolchain")
        .and_then(|table| table.get("channel"))
        .and_then(toml::Value::as_str)
        .expect("the stable pin");
    assert!(
        regex::Regex::new(r"^\d+\.\d+\.\d+$")
            .expect("version grammar")
            .is_match(stable)
    );
    let nightly = pins
        .get("njutest")
        .and_then(|table| table.get("nightly"))
        .and_then(toml::Value::as_str)
        .expect("the dated nightly pin beside the stable pin");
    assert!(
        regex::Regex::new(r"^nightly-\d{4}-\d{2}-\d{2}$")
            .expect("date grammar")
            .is_match(nightly),
        "an undated nightly changes under -D warnings: {nightly}"
    );
    let mise = read("mise.toml");
    let tasks: toml::Value = toml::from_str(&mise).expect("mise TOML");
    for pin in [
        tasks.get("tools").and_then(|tools| tools.get("rust")),
        tasks.get("env").and_then(|env| env.get("NJUTEST_NIGHTLY")),
    ] {
        assert!(
            pin.and_then(toml::Value::as_str).is_some_and(
                |value| value.contains("read_file(path=config_root ~ '/rust-toolchain.toml')")
            ),
            "mise must read both shared pins"
        );
    }
    assert!(
        !floating(&mise),
        "local tasks may select only the shared pins"
    );
    let setup = read(".github/actions/setup-rust/action.yml");
    assert!(
        !floating(&setup),
        "the shared setup action must read the pins"
    );
    assert!(
        setup.contains("rust-toolchain.toml") && setup.contains("scripts/toolchain.py"),
        "setup must export the nightly from the shared pin file"
    );
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/workflows");
    for entry in std::fs::read_dir(dir).expect("workflows") {
        let path = entry.expect("workflow").path();
        let source = std::fs::read_to_string(&path).expect("workflow text");
        assert!(
            !floating(&source),
            "{} installs or selects a toolchain without the shared pin",
            path.display()
        );
        if source.contains("dtolnay/rust-toolchain@") {
            assert!(
                source.contains("scripts/toolchain.py")
                    && source.contains("toolchain: ${{ steps.toolchains.outputs.nightly }}"),
                "{} must select the pin explicitly",
                path.display()
            );
        }
    }
}

#[test]
fn the_toolchain_rule_refuses_floating_and_independently_pinned_commands() {
    for source in [
        "cargo +nightly miri setup",
        "cargo +1.98.0 test",
        "cargo +custom-toolchain test",
        "rustup toolchain install nightly-2026-09-29",
        "rustup target add --toolchain 1.98.0",
        "RUSTUP_TOOLCHAIN: nightly",
        "toolchain: nightly-2026-08-21",
    ] {
        assert!(floating(source), "a command must read the pin: {source}");
    }
    assert!(!floating("rustup toolchain install \"${NJUTEST_NIGHTLY}\""));
}

#[test]
fn long_test_jobs_report_each_test_and_upload_evidence_before_the_job_deadline() {
    let source = read(".github/workflows/ci.yml");
    let jobs = regex::Regex::new(r"(?m)^  [a-z][a-z-]*:$").expect("job grammar");
    let deadlines =
        regex::Regex::new(r"(?m)^        timeout-minutes: (\d+)$").expect("step timeout grammar");
    for (name, budget) in [("test", 150), ("coverage", 100)] {
        let start = source
            .find(&format!("\n  {name}:\n"))
            .expect("the long test job");
        let body = source.get(start + name.len() + 5..).expect("job body");
        let end = jobs.find(body).map_or(body.len(), |next| next.start());
        let job = body.get(..end).expect("job extent");
        assert!(
            job.contains(&format!("timeout-minutes: {budget}\n")),
            "{name} must keep its job deadline"
        );
        let steps: Vec<&str> = job.split("\n      - ").collect();
        let (index, testing) = steps
            .iter()
            .enumerate()
            .find(|(_index, step)| {
                step.contains("mise run test:ci") || step.contains("cargo nextest run")
            })
            .expect("the complete suite step");
        let upload = steps
            .get(index + 1)
            .expect("upload immediately after testing");
        assert!(
            upload.contains("if: always()")
                && upload.contains("actions/upload-artifact@")
                && upload.contains("target/nextest/ci/junit.xml")
                && upload.contains("target/suite-cost/ci"),
            "{name} must upload JUnit and cost records even when cancelled"
        );
        assert!(
            testing.contains("NJUTEST_TEST_COST_DIR:")
                && testing.contains("NJUTEST_FIXTURE_BUILD_CACHE:"),
            "{name} records all engine work"
        );
        if name == "coverage" {
            for argument in ["--status-level pass", "--test-threads 3", "--profile ci"] {
                assert!(testing.contains(argument), "coverage requires {argument}");
            }
        }
        assert!(
            !testing.contains("--partition") && !testing.contains(" -E "),
            "the whole suite runs once"
        );
        let step_budget = deadlines
            .captures(testing)
            .and_then(|capture| capture.get(1))
            .map(|value| value.as_str().parse::<u32>().expect("step minutes"))
            .expect("a step timeout below the job timeout");
        assert!(
            step_budget < budget && step_budget > 0,
            "{name} leaves time to publish evidence"
        );
    }
    let mise = read("mise.toml");
    let task: toml::Value = toml::from_str(&mise).expect("mise TOML");
    let command = task
        .get("tasks")
        .and_then(|tasks| tasks.get("test:ci"))
        .and_then(|task| task.get("run"))
        .and_then(toml::Value::as_str)
        .expect("CI test task");
    assert!(
        command.contains("--status-level pass")
            && command.contains("--profile ci")
            && command.contains("--test-threads 3"),
        "CI reports successful tests with their durations: {command}"
    );
}

#[test]
fn the_ci_profile_stores_every_success_and_failure_in_junit() {
    let config: toml::Value = toml::from_str(&read(".config/nextest.toml")).expect("nextest TOML");
    let junit = config
        .get("profile")
        .and_then(|profile| profile.get("ci"))
        .and_then(|profile| profile.get("junit"))
        .expect("CI JUnit configuration");
    assert_eq!(
        junit.get("path").and_then(toml::Value::as_str),
        Some("junit.xml")
    );
    for field in ["store-success-output", "store-failure-output"] {
        assert_eq!(
            junit.get(field).and_then(toml::Value::as_bool),
            Some(true),
            "{field}"
        );
    }
}

#[test]
fn the_cost_measurement_uses_ci_concurrency_and_records_machine_load() {
    let script = read("scripts/run-suite-cost.py");
    assert!(script.contains("environment[\"CARGO_BUILD_JOBS\"] = \"3\""));
    assert!(
        script.contains("\"--test-threads\", \"3\"")
            && script.contains("\"--status-level\", \"pass\"")
    );
    for field in [
        "load.jsonl",
        "machine.json",
        "toolchain_running",
        "maximum_observed_toolchain_concurrency",
        "os.getloadavg()",
    ] {
        assert!(script.contains(field), "measurements require {field}");
    }
}

fn native_sleep(source: &str) -> bool {
    use syn::visit::Visit as _;
    struct Sleeps {
        found: bool,
    }
    impl<'ast> syn::visit::Visit<'ast> for Sleeps {
        fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
            if let syn::Expr::Path(path) = call.func.as_ref()
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "sleep")
            {
                self.found = true;
            }
            syn::visit::visit_expr_call(self, call);
        }
    }
    let syntax = xtask::lexed::file(source).expect("the supervision fixture parses");
    let mut sleeps = Sleeps { found: false };
    for item in syntax.items {
        if let syn::Item::Fn(function) = item {
            let virtual_wasi = function.attrs.iter().any(|attribute| {
                attribute.path().is_ident("cfg") && matches!(attribute.parse_args::<syn::Meta>(),
                    Ok(syn::Meta::NameValue(meta)) if meta.path.is_ident("target_os") && matches!(&meta.value,
                        syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(value), .. }) if value.value() == "wasi"))
            });
            if !virtual_wasi {
                sleeps.visit_item_fn(&function);
            }
        }
    }
    sleeps.found
}

#[test]
fn supervision_fixtures_advance_events_instead_of_waiting_on_wall_time() {
    assert!(native_sleep(
        "fn wait() { std::thread::sleep(std::time::Duration::from_secs(60)); }"
    ));
    assert!(!native_sleep(
        "#[cfg(target_os = \"wasi\")] fn wait() { std::thread::sleep(std::time::Duration::from_secs(60)); }"
    ));
    for fixture in [
        "fixture-faulted/tests/calls.rs",
        "fixture-hang/tests/pace.rs",
    ] {
        let source = read(&format!("fixtures/{fixture}"));
        assert!(
            !native_sleep(&source),
            "{fixture} must use the injected supervision clock"
        );
        assert!(
            source.contains("NJUTEST_TEST_CLOCK"),
            "{fixture} explicitly receives its clock"
        );
    }
}
