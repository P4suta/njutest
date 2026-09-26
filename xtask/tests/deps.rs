// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The dependency direction rule.

use xtask::deps::{Edge, EdgeKind, check, prohibited_direct_dependencies};

#[derive(Debug, thiserror::Error)]
enum TestError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Gate(#[from] xtask::gates::GateError),
    #[error("the dependency gate did not report {expected}; report was: {report}")]
    Missing { expected: String, report: String },
}

fn edge(from: &str, to: &str, kind: EdgeKind) -> Edge {
    Edge {
        from: from.to_owned(),
        to: to.to_owned(),
        kind,
    }
}

#[test]
fn the_runner_may_depend_on_the_engine_but_not_the_reverse() {
    let allowed = [
        edge("njutest", "rust-mutants", EdgeKind::Normal),
        edge("rust-mutants-cli", "rust-mutants", EdgeKind::Normal),
        edge("njutest", "njutest-macros", EdgeKind::Normal),
        edge("rust-mutants", "njutest-macros", EdgeKind::Normal),
        edge("rust-mutants-cli", "njutest-macros", EdgeKind::Normal),
        edge("xtask", "njutest-macros", EdgeKind::Normal),
        edge("compiler-surfaces", "rust-mutants", EdgeKind::Normal),
    ];
    assert!(check(&allowed).is_empty());

    let refused = [
        edge("rust-mutants", "njutest", EdgeKind::Normal),
        edge("rust-mutants-cli", "njutest", EdgeKind::Normal),
        edge("njutest-macros", "njutest", EdgeKind::Normal),
        edge("xtask", "rust-mutants", EdgeKind::Normal),
    ];
    assert_eq!(check(&refused), refused);
}

#[test]
fn the_devkit_is_a_dev_dependency_of_anybody_and_a_dependency_of_nobody() {
    assert!(check(&[edge("rust-mutants", "njutest-devkit", EdgeKind::Dev)]).is_empty());
    assert!(check(&[edge("njutest", "njutest-devkit", EdgeKind::Dev)]).is_empty());
    let refused = [edge("rust-mutants", "njutest-devkit", EdgeKind::Normal)];
    assert_eq!(check(&refused), refused);
}

#[test]
fn a_crate_may_dev_depend_on_itself_to_enable_its_own_testkit_feature() {
    assert!(check(&[edge("njutest", "njutest", EdgeKind::Dev)]).is_empty());
}

#[test]
fn a_refused_edge_is_reported_in_words() {
    assert_eq!(
        edge("rust-mutants", "njutest", EdgeKind::Normal).to_string(),
        "rust-mutants depends on njutest"
    );
    assert_eq!(
        edge("a", "b", EdgeKind::Dev).to_string(),
        "a dev-depends on b"
    );
}

#[test]
fn errors_remain_a_closed_typed_vocabulary() {
    assert_eq!(
        prohibited_direct_dependencies([
            ("runner", "serde"),
            ("runner", "anyhow"),
            ("runner", "miette"),
        ]),
        [
            "runner depends on anyhow, which erases error variants behind downcasts",
            "runner depends on miette, which erases error variants behind downcasts",
        ]
    );
    assert!(
        prohibited_direct_dependencies([
            ("runner", "thiserror"),
            ("runner", "snafu"),
            ("runner", "error-stack"),
        ])
        .is_empty()
    );
}

#[test]
fn source_invariants_cannot_be_evaded_by_proc_macro_expansion() {
    assert_eq!(
        prohibited_direct_dependencies([
            ("runner", "serde"),
            ("runner", "async-trait"),
            ("engine", "async-recursion"),
            ("wire", "typetag"),
        ]),
        [
            "engine depends on async-recursion, which generates owned trait objects after source inspection",
            "runner depends on async-trait, which generates owned trait objects after source inspection",
            "wire depends on typetag, which generates owned trait objects after source inspection",
        ]
    );
}

fn write_package(root: &std::path::Path, path: &str, name: &str) -> Result<(), TestError> {
    let package = root.join(path);
    std::fs::create_dir_all(package.join("src"))?;
    std::fs::write(
        package.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n"),
    )?;
    std::fs::write(package.join("src/lib.rs"), "")?;
    Ok(())
}

#[test]
fn real_manifests_cannot_hide_generators_by_rename_kind_target_or_fuzz() -> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    xtask::repository::init(root.path())?;
    for (path, name) in [
        ("vendor/async-trait", "async-trait"),
        ("vendor/async-recursion", "async-recursion"),
        ("vendor/typetag", "typetag"),
    ] {
        write_package(root.path(), path, name)?;
    }
    for name in ["normal-user", "build-user", "dev-user", "target-user"] {
        std::fs::create_dir_all(root.path().join("crates").join(name).join("src"))?;
        std::fs::write(root.path().join("crates").join(name).join("src/lib.rs"), "")?;
    }
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"vendor/*\"]\nresolver = \"3\"\n",
    )?;
    let manifests = [
        (
            "normal-user",
            "[dependencies]\nhidden = { package = \"async-trait\", path = \"../../vendor/async-trait\" }\n",
        ),
        (
            "build-user",
            "[build-dependencies]\nhidden = { package = \"async-recursion\", path = \"../../vendor/async-recursion\" }\n",
        ),
        (
            "dev-user",
            "[dev-dependencies]\nhidden = { package = \"typetag\", path = \"../../vendor/typetag\" }\n",
        ),
        (
            "target-user",
            "[target.'cfg(unix)'.dependencies]\nhidden = { package = \"async-trait\", path = \"../../vendor/async-trait\" }\n",
        ),
    ];
    for (name, dependency) in manifests {
        std::fs::write(
            root.path().join("crates").join(name).join("Cargo.toml"),
            format!(
                "[package]\nname = \"{name}\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
                 {dependency}"
            ),
        )?;
    }
    std::fs::create_dir_all(root.path().join("fuzz/src"))?;
    std::fs::write(root.path().join("fuzz/src/lib.rs"), "")?;
    std::fs::write(
        root.path().join("fuzz/Cargo.toml"),
        "[package]\nname = \"fuzz-user\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         [workspace]\n\
         [dependencies]\nhidden = { package = \"typetag\", path = \"../vendor/typetag\" }\n",
    )?;

    let failure = match xtask::gates::deps(root.path()) {
        Ok(report) => {
            return Err(TestError::Missing {
                expected: "a prohibited dependency".to_owned(),
                report,
            });
        }
        Err(failure) => failure.to_string(),
    };
    for expected in [
        "normal-user depends on async-trait",
        "build-user depends on async-recursion",
        "dev-user depends on typetag",
        "target-user depends on async-trait",
        "fuzz-user depends on typetag",
    ] {
        if !failure.contains(expected) {
            return Err(TestError::Missing {
                expected: expected.to_owned(),
                report: failure,
            });
        }
    }
    Ok(())
}

#[test]
fn a_feature_only_a_development_edge_turns_on_is_named_for_a_direct_dependency_alone() {
    let shipped = "serde_json v1.0.151|default,std\nparking_lot v0.12.5|\nnjutest v0.1.0 (/x)|\n";
    let developed = "serde_json v1.0.151|default,float_roundtrip,std (*)\nparking_lot v0.12.5|default\nnjutest v0.1.0 (/x)|testkit\njsonschema v0.56.0|default\n";
    let direct: std::collections::BTreeSet<String> = ["serde_json".to_owned()].into();
    assert_eq!(
        xtask::deps::features_only_tests_build_with((shipped, developed), &direct),
        vec![(
            "serde_json v1.0.151".to_owned(),
            vec!["float_roundtrip".to_owned()]
        )],
        "the shipped crates' own dependency built with a feature only tests ask for is named; a \
         transitive package, a member's own test seam and a development-only package are not \
         held"
    );
    assert_eq!(
        xtask::deps::features_only_tests_build_with((developed, developed), &direct),
        Vec::new(),
        "and a graph built alike with and without development edges draws nothing"
    );
}

#[test]
fn the_targets_the_feature_gate_reads_are_the_ones_a_release_builds() {
    let workflow = std::fs::read_to_string(
        njutest_devkit::paths::workspace_root().join(".github/workflows/release.yml"),
    )
    .unwrap_or_else(|error| panic!("the release workflow: {error}"));
    let built: std::collections::BTreeSet<&str> = workflow
        .lines()
        .filter_map(|line| line.trim().strip_prefix("target: "))
        .map(str::trim)
        .collect();
    let read: std::collections::BTreeSet<&str> = xtask::deps::SHIPPED_TARGETS.into_iter().collect();
    assert_eq!(
        read, built,
        "a target a release builds unifies features over its own graph, and a gate that does not read it passes what that release ships without"
    );
}
