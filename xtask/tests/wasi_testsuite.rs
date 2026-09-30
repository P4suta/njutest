// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! `wasi-testsuite`: the pin it reads, the checkout it verifies, and the harness it holds to having run.

#![expect(
    clippy::expect_used,
    clippy::panic,
    reason = "a test reports a setup failure by panicking"
)]

use xtask::wasitestsuite::{
    EXPECTATIONS, HARNESS_PACKAGE, HARNESS_TARGET, HARNESS_TEST, SUITE_VARIABLE, SuiteError, pin,
    ran, verify,
};

/// The text of `relative` in this repository.
fn repository(relative: &str) -> String {
    let path = njutest_devkit::paths::workspace_root().join(relative);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

#[test]
fn the_pin_names_the_suite_and_counts_every_test_the_file_names() {
    let text = repository(EXPECTATIONS);
    let pinned = pin(&text).expect("the repository's expectations are a pin");
    assert_eq!(
        pinned.repository,
        "https://github.com/WebAssembly/wasi-testsuite"
    );
    let named = text.matches("\n[[test]]\n").count();
    assert_eq!(
        pinned.passing.checked_add(pinned.refused),
        Some(named),
        "every test the file names is counted once, as passing or as refused"
    );
}

#[test]
fn a_pin_that_names_no_full_commit_or_a_result_the_harness_does_not_read_is_refused() {
    let repository = "repository = \"https://github.com/WebAssembly/wasi-testsuite\"\n";
    for (case, text) in [
        ("no commit", repository.to_owned()),
        (
            "a branch for a commit",
            format!("{repository}commit = \"main\"\n"),
        ),
        (
            "a result the harness does not read",
            format!(
                "{repository}commit = \"609c446139956ff30239f87cb18af1dc6128bed2\"\n[[test]]\nname = \"c/a\"\nresult = \"skip\"\nrefused = []\n"
            ),
        ),
        (
            "no test",
            format!("{repository}commit = \"609c446139956ff30239f87cb18af1dc6128bed2\"\n"),
        ),
    ] {
        assert!(
            matches!(pin(&text), Err(SuiteError::Expectations { .. })),
            "{case} is refused"
        );
    }
}

#[test]
fn only_libtest_saying_the_harness_test_passed_is_a_run() {
    let passed = format!("running 1 test\ntest {HARNESS_TEST} ... ok\n\ntest result: ok. 1 passed");
    assert!(ran(&passed));
    for (case, printed) in [
        (
            "a filter that matched nothing",
            "running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 1 ignored".to_owned(),
        ),
        (
            "the test ignored",
            format!("test {HARNESS_TEST} ... ignored, needs the pinned suite"),
        ),
        ("another test", format!("test {HARNESS_TEST}_too ... ok")),
    ] {
        assert!(!ran(&printed), "{case} ran nothing of the suite");
    }
}

#[test]
fn the_command_names_the_harness_as_the_harness_names_itself() {
    let manifest = repository(&format!("crates/{HARNESS_PACKAGE}/Cargo.toml"));
    assert!(
        manifest.contains(&format!(
            "[[test]]\nname = \"{HARNESS_TARGET}\"\npath = \"tests/{HARNESS_TARGET}.rs\"\n"
        )),
        "{HARNESS_PACKAGE} declares the test binary {HARNESS_TARGET} the command runs"
    );
    let (module, test) = HARNESS_TEST
        .split_once("::")
        .expect("the harness's test is named with its module");
    let binary = repository(&format!(
        "crates/{HARNESS_PACKAGE}/tests/{HARNESS_TARGET}.rs"
    ));
    assert!(
        binary.contains(&format!("#[path = \"{module}.rs\"]\nmod {module};\n")),
        "the harness is the module {module} of the test binary {HARNESS_TARGET}"
    );
    let harness = repository(&format!("crates/{HARNESS_PACKAGE}/tests/{module}.rs"));
    assert!(
        harness.contains(&format!("const SUITE: &str = \"{SUITE_VARIABLE}\";")),
        "the harness reads the checkout from the variable the command names it in"
    );
    assert!(
        harness.contains(&format!(
            "#[test]\n#[ignore = \"needs the pinned WebAssembly/wasi-testsuite, which `cargo xtask wasi-testsuite` fetches, verifies and names\"]\nfn {test}() {{"
        )),
        "the test the command runs by name is the harness's one ignored test"
    );
    assert!(
        harness.contains(&format!("const EXPECTATIONS: &str = \"{EXPECTATIONS}\";")),
        "the harness and the command read one expectations file"
    );
}

/// What `xtask` prints when it is asked `asked`, run as a program named `xtask` on every platform.
fn printed(asked: &[&str]) -> Vec<u8> {
    let environment = xtask::environment::Environment::of(Vec::new());
    let directory = njutest_devkit::paths::workspace_root();
    let process = xtask::Process {
        cargo: std::ffi::OsStr::new("cargo"),
        environment: &environment,
        directory: &directory,
        executable: std::path::Path::new("xtask"),
    };
    let (mut output, mut errors) = (Vec::new(), Vec::new());
    let arguments = std::iter::once("xtask")
        .chain(asked.iter().copied())
        .map(std::ffi::OsString::from);
    let ended = xtask::run_from(
        arguments,
        &process,
        &mut xtask::Streams {
            input: &mut std::io::empty(),
            output: &mut output,
            errors: &mut errors,
        },
    );
    assert_eq!(
        (ended, errors.as_slice()),
        (std::process::ExitCode::SUCCESS, &[][..]),
        "{asked:?} is answered on standard output"
    );
    output
}

#[test]
fn the_command_list_and_the_command_say_what_it_holds_as_their_goldens_do() {
    let testdata = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata");
    for (asked, golden) in [
        (&["--help"][..], "help.golden"),
        (
            &["wasi-testsuite", "--help"][..],
            "help-wasi-testsuite.golden",
        ),
    ] {
        njutest_devkit::golden::golden(&testdata.join(golden), &printed(asked))
            .unwrap_or_else(|error| panic!("{error}"));
    }
}

/// The commit `HEAD` names in the repository at `root`.
fn head(root: &std::path::Path) -> String {
    let output = xtask::repository::git(root)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git rev-parse HEAD");
    String::from_utf8(output.stdout)
        .expect("a commit id is text")
        .trim()
        .to_owned()
}

#[test]
fn a_checkout_is_its_commit_only_while_nothing_of_it_has_changed() {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let root = scratch.path();
    std::fs::create_dir_all(root.join("tests")).expect("a directory");
    std::fs::write(root.join("tests/kept.txt"), "kept").expect("a file");
    std::fs::write(root.join(".gitignore"), "*.log\n").expect("an ignore file");
    njutest_devkit::repo::commit_tree(root);
    let commit = head(root);
    assert!(
        verify(root, &commit).is_ok(),
        "a fresh checkout is its commit"
    );
    let other = "0123456789abcdef0123456789abcdef01234567";
    assert!(
        matches!(verify(root, other), Err(SuiteError::Checkout { .. })),
        "a checkout of another commit is refused"
    );
    for (case, relative, contents) in [
        ("a file changed", "tests/kept.txt", "changed"),
        ("a file added", "tests/added.txt", "added"),
        ("a file its ignore rules hide", "tests/hidden.log", "hidden"),
    ] {
        let path = root.join(relative);
        let before = std::fs::read_to_string(&path);
        std::fs::write(&path, contents).expect("a file");
        assert!(
            matches!(verify(root, &commit), Err(SuiteError::Checkout { .. })),
            "{case} is refused"
        );
        match before {
            Ok(before) => std::fs::write(&path, before).expect("the file restored"),
            Err(_absent) => std::fs::remove_file(&path).expect("the file removed"),
        }
        assert!(
            verify(root, &commit).is_ok(),
            "{case}, undone, is its commit again"
        );
    }
}
