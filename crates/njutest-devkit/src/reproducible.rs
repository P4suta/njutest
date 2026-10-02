// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether this machine builds a change made and reverted back to the bytes it built before, and the bytes a build left.

use std::collections::BTreeMap;
use std::path::Path;

use sha2::{Digest as _, Sha256};

mod witness;

/// What one build produced, by target name, each executable digested.
type Built = BTreeMap<String, String>;

/// Whether a tree built, changed, and built back comes out as the bytes it came out as, which is not whether a second build of an unchanged tree runs the first one's bytes.
///
/// # Panics
/// When the fixture cannot be copied, which is a setup failure rather than an answer.
#[must_use]
pub fn builds_a_reverted_change_to_the_same_bytes() -> bool {
    match witness::answer() {
        Ok(answer) => answer,
        Err(source) => {
            eprintln!("reproducibility-unbound: {source}");
            actual_reverted_change()
        }
    }
}

fn actual_reverted_change() -> bool {
    let fixture = crate::fixture::Fixture::copy("fixture-equivalent");
    let target = fixture.temp().join("twice");
    let source = fixture.root().join("src/lib.rs");
    let Ok(original) = std::fs::read(&source) else {
        return false;
    };

    let first = built(fixture.root(), &target);
    let mut changed = original.clone();
    changed.extend_from_slice(b"\npub const A_THING_NOTHING_READS: u8 = 7;\n");
    if std::fs::write(&source, &changed).is_err() {
        return false;
    }
    let between = built(fixture.root(), &target);
    if between.is_empty() {
        return false;
    }
    if std::fs::write(&source, &original).is_err() {
        return false;
    }
    let again = built(fixture.root(), &target);

    !first.is_empty() && first == again
}

/// The SHA-256 of the file at `path`, in hex: what a test compares to know two runs ran the same program.
///
/// # Panics
/// When the file cannot be read, which is a setup failure rather than an answer.
#[must_use]
#[track_caller]
#[expect(
    clippy::expect_used,
    reason = "a test that cannot read the program it ran has nothing to compare, and saying so is its only honest answer"
)]
pub fn digest(path: &Path) -> String {
    hex::encode(Sha256::digest(
        std::fs::read(path).expect("the file a test digests is readable"),
    ))
}

/// One build of the tree at `root`, or nothing at all when it did not build.
fn built(root: &Path, target: &Path) -> Built {
    let mut command = std::process::Command::new(crate::paths::cargo_binary());
    configure_build_command(&mut command, root, target);
    let Ok(said) = crate::cost::cargo(command, "reproducible::built compiler witness") else {
        return Built::new();
    };
    if !said.status.success() {
        return Built::new();
    }
    let Ok(stdout) = String::from_utf8(said.stdout) else {
        return Built::new();
    };
    let mut found = Built::new();
    for line in stdout.lines() {
        let Ok(message) = crate::strictjson::decode_str::<serde_json::Value>(line) else {
            continue;
        };
        if message.get("reason").and_then(serde_json::Value::as_str) != Some("compiler-artifact") {
            continue;
        }
        let (Some(name), Some(executable)) = (
            message
                .get("target")
                .and_then(|target| target.get("name"))
                .and_then(serde_json::Value::as_str),
            message
                .get("executable")
                .and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let Ok(bytes) = std::fs::read(executable) else {
            return Built::new();
        };
        if found
            .insert(name.to_owned(), hex::encode(Sha256::digest(&bytes)))
            .is_some()
        {
            return Built::new();
        }
    }
    found
}

#[expect(
    unused_results,
    reason = "Command's infallible builder API returns self; this unit helper is the explicit boundary"
)]
fn configure_build_command(command: &mut std::process::Command, root: &Path, target: &Path) {
    command
        .env_clear()
        .envs(crate::paths::environment_for_a_toolchain_run(&[]))
        .env("CARGO_INCREMENTAL", "0")
        .args(["test", "--no-run", "--message-format=json", "--offline"])
        .arg("--locked")
        .arg("--target-dir")
        .arg(target)
        .current_dir(root);
}

#[cfg(test)]
mod tests {
    #[test]
    fn identical_reproducibility_questions_share_one_actual_independent_pair() {
        const CHILD: &str = "NJUTEST_REPRODUCIBILITY_CHILD";
        if std::env::var_os(CHILD).is_some() {
            assert!(super::builds_a_reverted_change_to_the_same_bytes());
            assert!(super::builds_a_reverted_change_to_the_same_bytes());
            return;
        }
        let directory = tempfile::tempdir().expect("the actual work owner");
        let cost = directory.path().join("cost");
        std::fs::create_dir_all(&cost).expect("the private actual-work inventory");
        let status = std::process::Command::new(std::env::current_exe().expect("this binary"))
            .args([
                "--exact",
                "reproducible::tests::identical_reproducibility_questions_share_one_actual_independent_pair",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env("NJUTEST_TEST_COST_DIR", &cost)
            .env("NJUTEST_FIXTURE_BUILD_CACHE", directory.path().join("cache"))
            .status()
            .expect("the actual paired compiler child");
        assert!(
            status.success(),
            "the unchanged compiler questions: {status}"
        );
        let mut builds = 0_u64;
        for entry in std::fs::read_dir(&cost).expect("the original actual cost records") {
            let path = entry.expect("one actual record").path();
            let record: serde_json::Value = crate::strictjson::decode_slice(
                &std::fs::read(&path).expect("the complete actual record"),
            )
            .expect("strict original work");
            let work = record.get("work").expect("actual work");
            let actual = work
                .get("builds")
                .and_then(serde_json::Value::as_u64)
                .expect("the complete actual build count");
            builds = builds.checked_add(actual).expect("the measured total fits");
            let root = record
                .get("root")
                .and_then(serde_json::Value::as_str)
                .expect("the original compiler source root");
            crate::cost::record(
                std::path::Path::new(root),
                work,
                record.get("sealed").expect("the original sealed work"),
            )
            .expect("charge only the actual current compiler work once");
        }
        assert_eq!(
            builds, 3,
            "two identical questions retain the original, changed and independent restored processes"
        );
    }
}
