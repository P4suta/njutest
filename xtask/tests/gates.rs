// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates, applied to this repository.
//! This is the ratchet inside `cargo test`: a seam without a ledger line, a dependency in the wrong direction, a fixture without its lock file, or a version that drifted fails the suite, not only `cargo xtask`.

use xtask::gates;

/// A test setup or gate refusal that should reach the test harness without a second, panic-shaped failure path.
#[derive(Debug, thiserror::Error)]
enum TestFailure {
    /// A repository gate refused the tree.
    #[error(transparent)]
    Gate(#[from] gates::GateFailure),
    /// Test setup could not create or read its fixture.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A deliberately malformed fixture unexpectedly passed its gate.
    #[error("{0}")]
    UnexpectedSuccess(&'static str),
    /// A gate returned a report whose contract was not the one under test.
    #[error("{0}")]
    Contract(String),
}

fn require(condition: bool, message: impl Into<String>) -> Result<(), TestFailure> {
    if condition {
        Ok(())
    } else {
        Err(TestFailure::Contract(message.into()))
    }
}

fn refused<T>(
    result: Result<T, gates::GateFailure>,
    if_accepted: &'static str,
) -> Result<gates::GateFailure, TestFailure> {
    match result {
        Ok(_) => Err(TestFailure::UnexpectedSuccess(if_accepted)),
        Err(failure) => Ok(failure),
    }
}

#[test]
fn a_source_tree_that_cannot_be_walked_never_passes_as_empty() -> Result<(), TestFailure> {
    let root = tempfile::tempdir()?;
    let failure = refused(
        gates::all_sources(root.path()),
        "missing source roots established an empty repository",
    )?;
    require(
        failure.to_string().contains("walking the repository"),
        failure.to_string(),
    )
}

#[cfg(unix)]
#[test]
fn a_symbolic_link_never_hides_source_from_a_repository_gate() -> Result<(), TestFailure> {
    let root = tempfile::tempdir()?;
    for base in ["compiler-surfaces", "crates", "xtask", "fuzz"] {
        std::fs::create_dir_all(root.path().join(base))?;
    }
    let target = root.path().join("outside.txt");
    std::fs::write(&target, "fn hidden() {}\n")?;
    std::os::unix::fs::symlink(&target, root.path().join("crates/hidden.rs"))?;

    let failure = refused(
        gates::all_sources(root.path()),
        "a source set reached through a link passed as a closed tree",
    )?;
    require(
        failure.to_string().contains("symbolic link"),
        failure.to_string(),
    )
}

fn lint_tree(app_source: &str) -> Result<tempfile::TempDir, TestFailure> {
    let root = tempfile::tempdir()?;
    for directory in [
        "compiler-surfaces",
        "crates/app/src",
        "crates/njutest-macros/src",
        "xtask",
        "fuzz/src",
    ] {
        std::fs::create_dir_all(root.path().join(directory))?;
    }
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\", \"crates/njutest-macros\"]\nresolver = \"3\"\n",
    )?;
    std::fs::write(
        root.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
         \n[[package]]\nname = \"njutest-macros\"\nversion = \"0.0.0\"\n",
    )?;
    std::fs::write(
        root.path().join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n",
    )?;
    std::fs::write(root.path().join("crates/app/src/lib.rs"), app_source)?;
    std::fs::write(
        root.path().join("crates/njutest-macros/Cargo.toml"),
        "[package]\nname = \"njutest-macros\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[lib]\nproc-macro = true\n",
    )?;
    std::fs::write(
        root.path().join("crates/njutest-macros/src/lib.rs"),
        "use proc_macro::TokenStream;\n#[proc_macro_derive(AllVariants)]\npub fn all_variants(input: TokenStream) -> TokenStream { input }\n",
    )?;
    std::fs::write(root.path().join("xtask/empty.rs"), "")?;
    std::fs::write(root.path().join("xtask/wildcard_allowlist.txt"), "")?;
    std::fs::write(root.path().join("xtask/waiver_ceiling.txt"), "0\n")?;
    std::fs::write(
        root.path().join("xtask/proc_macro_inventory.txt"),
        "root njutest-macros 0.0.0 path\n",
    )?;
    std::fs::write(root.path().join("compiler-surfaces/empty.rs"), "")?;
    std::fs::write(
        root.path().join("fuzz/Cargo.toml"),
        "[package]\nname = \"fuzz\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[workspace]\n",
    )?;
    std::fs::write(root.path().join("fuzz/src/lib.rs"), "")?;
    std::fs::write(
        root.path().join("fuzz/Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"fuzz\"\nversion = \"0.0.0\"\n",
    )?;
    Ok(root)
}

#[test]
fn only_a_scanned_support_rs_file_may_be_included() -> Result<(), TestFailure> {
    let root = lint_tree("include!(\"support/ok.rs\");\n")?;
    std::fs::create_dir_all(root.path().join("crates/app/src/support"))?;
    std::fs::write(
        root.path().join("crates/app/src/support/ok.rs"),
        "pub fn supported() {}\n",
    )?;
    let report = gates::lints_scanned(root.path())?;
    require(report.starts_with("lints: "), report)
}

#[test]
fn opaque_or_unscanned_source_redirects_are_refused() -> Result<(), TestFailure> {
    for source in [
        "include!(\"hidden.inc\");\n",
        "std::include!(\"hidden.inc\");\n",
        "include!(concat!(env!(\"OUT_DIR\"), \"/hidden.rs\"));\n",
        "use core::include as load;\nload!(\"support/hidden.rs\");\n",
        "#[path = \"../../../../escape.rs\"] mod hidden;\n",
        "macro_rules! hidden { () => { include!(\"support/hidden.rs\"); } }\nhidden!();\n",
        "macro_rules! hidden { ($loader:path) => { $loader!(\"support/hidden.rs\") } }\nhidden!(include);\n",
    ] {
        let root = lint_tree(source)?;
        let failure = refused(
            gates::lints_scanned(root.path()),
            "a compiled source redirect escaped the scanned Rust universe",
        )?;
        require(
            failure.to_string().contains("opaque-macro-syntax"),
            failure.to_string(),
        )?;
    }
    Ok(())
}

#[test]
fn included_proc_macro_source_cannot_escape_the_exact_export_inventory() -> Result<(), TestFailure>
{
    let root = lint_tree("")?;
    let macro_root = root.path().join("crates/njutest-macros/src");
    std::fs::create_dir_all(macro_root.join("support"))?;
    let library = macro_root.join("lib.rs");
    let mut source = std::fs::read_to_string(&library)?;
    source.push_str("\ninclude!(\"support/hidden.rs\");\n");
    std::fs::write(&library, source)?;
    std::fs::write(
        macro_root.join("support/hidden.rs"),
        "#[proc_macro_attribute]\n\
         pub fn hidden(_args: proc_macro::TokenStream, input: proc_macro::TokenStream) \
         -> proc_macro::TokenStream { input }\n",
    )?;
    let failure = refused(
        gates::lints_scanned(root.path()),
        "an included procedural-macro export escaped the exact inventory",
    )?;
    require(
        failure.to_string().contains("njutest-macros exports"),
        failure.to_string(),
    )
}

#[test]
fn a_new_dependency_proc_macro_is_refused_before_its_expansion_is_trusted()
-> Result<(), TestFailure> {
    let root = lint_tree("")?;
    let generator = root.path().join("crates/foreign-generator");
    std::fs::create_dir_all(generator.join("src"))?;
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/app\", \"crates/foreign-generator\", \
         \"crates/njutest-macros\"]\nresolver = \"3\"\n",
    )?;
    std::fs::write(
        root.path().join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\
         [dependencies]\nforeign-generator = { path = \"../foreign-generator\" }\n",
    )?;
    std::fs::write(
        generator.join("Cargo.toml"),
        "[package]\nname = \"foreign-generator\"\nversion = \"9.9.9\"\nedition = \"2024\"\n\
         [lib]\nproc-macro = true\n",
    )?;
    std::fs::write(
        generator.join("src/lib.rs"),
        "#[proc_macro_attribute]\npub fn erase(_args: proc_macro::TokenStream, _input: \
         proc_macro::TokenStream) -> proc_macro::TokenStream { \
         \"Box<dyn Hidden>\".parse().unwrap() }\n",
    )?;
    std::fs::write(
        root.path().join("Cargo.lock"),
        "version = 4\n\n[[package]]\nname = \"app\"\nversion = \"0.0.0\"\n\
         dependencies = [\"foreign-generator\"]\n\n[[package]]\nname = \
         \"foreign-generator\"\nversion = \"9.9.9\"\n\n[[package]]\nname = \
         \"njutest-macros\"\nversion = \"0.0.0\"\n",
    )?;
    let failure = refused(
        gates::lints_scanned(root.path()),
        "a new dependency procedural macro passed without an inventory decision",
    )?;
    require(
        failure
            .to_string()
            .contains("dependency procedural-macro inventory drifted"),
        failure.to_string(),
    )
}

#[test]
fn a_non_rs_cargo_target_is_not_a_proved_source_tree() -> Result<(), TestFailure> {
    let root = lint_tree("")?;
    std::fs::write(
        root.path().join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[lib]\npath = \"src/hidden.inc\"\n",
    )?;
    std::fs::write(
        root.path().join("crates/app/src/hidden.inc"),
        "#![allow(unsafe_code)]\npub unsafe fn hidden() {}\n",
    )?;
    let failure = refused(
        gates::lints_scanned(root.path()),
        "a Cargo target outside the exact .rs set passed",
    )?;
    require(
        failure.to_string().contains("Cargo target app:app"),
        failure.to_string(),
    )
}

#[test]
fn recursive_local_path_dependencies_stay_inside_the_source_roots() -> Result<(), TestFailure> {
    let root = lint_tree("pub fn visible() { hidden::hidden(); }\n")?;
    std::fs::write(
        root.path().join("crates/app/Cargo.toml"),
        "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n[dependencies]\nhidden = { path = \"../../hidden\" }\n",
    )?;
    std::fs::create_dir_all(root.path().join("hidden/src"))?;
    std::fs::write(
        root.path().join("hidden/Cargo.toml"),
        "this external manifest must never be parsed = [",
    )?;
    std::fs::write(
        root.path().join("hidden/src/lib.rs"),
        "#![allow(unsafe_code)]\npub fn hidden() {}\n",
    )?;
    let failure = refused(
        gates::lints_scanned(root.path()),
        "an excluded local path dependency passed as external code",
    )?;
    require(
        failure
            .to_string()
            .contains("outside the four scanned source roots"),
        failure.to_string(),
    )?;
    require(
        !failure.to_string().contains("cargo metadata"),
        "Cargo interpreted the refused external manifest before the gate closed its source universe",
    )
}

#[test]
fn rust_outside_the_closed_roots_is_refused_except_for_fixture_inputs() -> Result<(), TestFailure> {
    let root = lint_tree("")?;
    std::fs::create_dir_all(root.path().join("fixtures/corpus/src"))?;
    std::fs::write(
        root.path().join("fixtures/corpus/src/lib.rs"),
        "pub fn external_input() {}\n",
    )?;
    let sources = gates::all_sources(root.path())?;
    require(
        sources
            .iter()
            .all(|path| !path.starts_with(root.path().join("fixtures"))),
        "fixture input entered the proved Rust source set",
    )?;

    std::fs::create_dir_all(root.path().join("scripts"))?;
    std::fs::write(
        root.path().join("scripts/hidden.rs"),
        "pub fn unproved_tool() {}\n",
    )?;
    let failure = refused(
        gates::all_sources(root.path()),
        "a Rust tool outside the closed source roots was silently unproved",
    )?;
    require(
        failure
            .to_string()
            .contains("only fixtures/ is an explicit unproved input corpus"),
        failure.to_string(),
    )
}

#[test]
fn a_fixture_root_that_cannot_be_listed_never_passes_as_empty() -> Result<(), TestFailure> {
    let root = tempfile::tempdir()?;
    let failure = refused(
        gates::fixtures(root.path()),
        "a missing fixture root passed as a repository with zero fixtures",
    )?;
    require(
        failure.to_string().contains("fixtures"),
        failure.to_string(),
    )
}

#[test]
fn a_public_function_only_a_test_names_is_what_the_reach_gate_reports() {
    let declaring = "pub fn believed_shipped() -> u8 { 0 }\npub fn called() -> u8 { 1 }\n";
    let ships = "pub fn believed_shipped() -> u8 { 0 }\npub fn called() -> u8 { 1 }\nfn use_it() { let _ = called(); }\n";
    let tested = "believed_shipped();\ncalled();\npub fn believed_shipped() -> u8 { 0 }\npub fn called() -> u8 { 1 }\n";
    assert_eq!(
        gates::only_a_test_reaches(declaring, ships, tested),
        vec!["believed_shipped".to_owned()],
        "a capability with a test is a capability somebody believed shipped (ADR 0023), so the \
         one nothing but a test names is the one to report and the one production calls is not"
    );
}

#[test]
fn a_defaulting_call_is_counted_and_a_test_module_is_not() {
    let source = "fn read(v: Option<u8>) -> u8 { v.unwrap_or(0) + v.map_or(1, |x| x) }\n\
                  fn all(v: Vec<Option<u8>>) -> Vec<u8> { v.into_iter().map(Option::unwrap_or_default).collect() }\n\
                  #[cfg(test)] mod tests { fn t(v: Option<u8>) -> u8 { v.unwrap_or_default() } }\n";
    assert_eq!(
        xtask::defaulted::defaulted_in(source).expect("the source parses"),
        3,
        "a value supplied where the input gave none is counted where the audit runs, and a test \
         building its own specimen is not the audit"
    );
}

#[test]
fn a_defaulting_call_inside_a_macro_is_counted_like_one_outside() {
    let source = "fn say(v: Option<&str>) -> String { format!(\"{}\", v.unwrap_or(\"?\")) }\n\
                  fn doc(v: Option<u8>) -> serde_json::Value { serde_json::json!({ \"n\": v.map_or(0, u8::from) }) }\n\
                  fn check(v: Option<u8>) { assert!(v.map(Option::Some).unwrap_or_default().is_some()); }\n\
                  fn named(unwrap_or: u8) -> String { format!(\"{unwrap_or}\") }\n";
    assert_eq!(
        xtask::defaulted::defaulted_in(source).expect("the source parses"),
        3,
        "syn leaves a macro's arguments as tokens, so a value supplied inside format!, json! or \
         assert! went uncounted while the same call outside one was held to the ceiling; a name \
         that is only a binding is not a call"
    );
}

#[test]
fn a_reader_is_held_to_exactly_its_ceiling() {
    let counted: std::collections::BTreeMap<String, usize> =
        std::iter::once(("xtask/src/wire.rs".to_owned(), 3)).collect();
    assert_eq!(
        xtask::defaulted::held(&counted, "3 xtask/src/wire.rs\n"),
        Ok(3)
    );
    let above = xtask::defaulted::held(&counted, "2 xtask/src/wire.rs\n").expect_err("above");
    assert!(
        above
            .iter()
            .any(|one| one.contains("against a ceiling of 2")),
        "{above:?}"
    );
    let below = xtask::defaulted::held(&counted, "5 xtask/src/wire.rs\n").expect_err("below");
    assert!(
        below
            .iter()
            .any(|one| one.contains("lower the ceiling to 3")),
        "a fall is kept by lowering the ceiling to it, or the next change can spend it: {below:?}"
    );
    let fallen: std::collections::BTreeMap<String, usize> =
        std::iter::once(("xtask/src/wire.rs".to_owned(), 0)).collect();
    let none = xtask::defaulted::held(&fallen, "3 xtask/src/wire.rs\n").expect_err("fallen");
    assert!(
        none.iter().any(|one| one.contains("remove its line")),
        "{none:?}"
    );
    let unnamed = xtask::defaulted::held(&counted, "").expect_err("unnamed");
    assert!(
        unnamed.iter().any(|one| one.contains("ceiling of 0")),
        "{unnamed:?}"
    );
    let gone = xtask::defaulted::held(&std::collections::BTreeMap::new(), "4 xtask/src/gone.rs\n")
        .expect_err("a stale line");
    assert!(
        gone.iter().any(|one| one.contains("remove the line")),
        "{gone:?}"
    );
}

/// A one-package workspace whose `.rust-mutants.toml` skips `skipped`, with a library and one integration test.
fn skipping(skipped: &str) -> Result<tempfile::TempDir, TestFailure> {
    let root = tempfile::tempdir()?;
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"planted\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[workspace]\n",
    )?;
    std::fs::create_dir_all(root.path().join("src"))?;
    std::fs::create_dir_all(root.path().join("tests"))?;
    std::fs::write(root.path().join("src/lib.rs"), "pub fn one() {}\n")?;
    std::fs::write(root.path().join("tests/one.rs"), "#[test]\nfn one() {}\n")?;
    std::fs::write(
        root.path().join(".rust-mutants.toml"),
        format!("version = 1\n[execution]\nskip_targets = [\"{skipped}\"]\n"),
    )?;
    Ok(root)
}

#[test]
fn a_skipped_target_no_member_declares_is_refused_with_what_its_package_declares()
-> Result<(), TestFailure> {
    let renamed = skipping("planted/test/two")?;
    let failure = refused(
        gates::skipped(renamed.path()),
        "a skip naming a target that was renamed passed the gate",
    )?;
    require(
        failure.0.contains("planted/test/two") && failure.0.contains("planted/test/one"),
        format!(
            "the refusal names what was skipped and what the package declares now: {}",
            failure.0
        ),
    )?;
    let named = skipping("planted/test/one")?;
    let passed = gates::skipped(named.path())?;
    require(passed.starts_with("skipped: 1 skipped target"), passed)
}
