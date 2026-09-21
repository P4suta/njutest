// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gates, applied to this repository. This is the ratchet inside `cargo test`: a seam without a ledger line, a dependency in the wrong direction, a fixture without its lock file, or a version that drifted fails the suite, not only `cargo xtask`.

use xtask::gates;

/// A test setup or gate refusal that should reach the test harness without a
/// second, panic-shaped failure path.
#[derive(Debug, thiserror::Error)]
enum TestFailure {
    /// A repository gate refused the tree.
    #[error(transparent)]
    Gate(#[from] gates::GateFailure),
    /// Test setup could not create or read its fixture.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// A path returned by the production source walker escaped its root.
    #[error("{} is outside {}", path.display(), root.display())]
    OutsideRoot {
        path: std::path::PathBuf,
        root: std::path::PathBuf,
        #[source]
        source: std::path::StripPrefixError,
    },
    /// A committed source path was not exact UTF-8.
    #[error("{} is not UTF-8", path.display())]
    NonUtf8Path { path: std::path::PathBuf },
    /// A deliberately malformed fixture unexpectedly passed its gate.
    #[error("{0}")]
    UnexpectedSuccess(&'static str),
    /// The gate declaration could not be found structurally in its source.
    #[error("xtask/src/lib.rs no longer declares the gates")]
    MissingGateDeclaration,
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
fn the_seam_ledger_agrees_with_the_tree() -> Result<(), TestFailure> {
    let report = gates::devgates(&gates::workspace_root())?;
    require(report.starts_with("devgates: "), report)
}

#[test]
fn every_internal_dependency_points_in_the_allowed_direction() -> Result<(), TestFailure> {
    let report = gates::deps(&gates::workspace_root())?;
    require(report.starts_with("deps: "), report)
}

#[test]
fn every_fixture_follows_the_conventions() -> Result<(), TestFailure> {
    let report = gates::fixtures(&gates::workspace_root())?;
    require(report.starts_with("fixtures: "), report)
}

#[test]
fn the_release_versions_agree() -> Result<(), TestFailure> {
    let report = gates::release_check(&gates::workspace_root())?;
    require(report.starts_with("release-check: "), report)
}

#[test]
fn every_milestone_reference_resolves_to_the_roadmap() -> Result<(), TestFailure> {
    let report = gates::milestones(&gates::workspace_root())?;
    require(report.starts_with("milestones: "), report)
}

#[test]
fn every_crate_surface_has_a_compiler_checked_meaning() -> Result<(), TestFailure> {
    let report = gates::surfaces(&gates::workspace_root())?;
    require(report.starts_with("surfaces: "), report)
}

#[test]
fn production_sources_exclude_test_support() -> Result<(), TestFailure> {
    let root = gates::workspace_root();
    let files: Vec<String> = gates::production_sources(&root)?
        .iter()
        .map(|path| {
            let relative = path
                .strip_prefix(&root)
                .map_err(|source| TestFailure::OutsideRoot {
                    path: path.clone(),
                    root: root.clone(),
                    source,
                })?;
            let relative = relative
                .to_str()
                .ok_or_else(|| TestFailure::NonUtf8Path { path: path.clone() })?;
            Ok(relative.replace('\\', "/"))
        })
        .collect::<Result<_, TestFailure>>()?;
    require(
        files.iter().any(|f| f == "crates/rust-mutants/src/lib.rs"),
        format!("missing production library from {files:?}"),
    )?;
    require(
        files.iter().any(|f| f == "xtask/src/devgates.rs"),
        format!("missing production gate from {files:?}"),
    )?;
    require(
        files
            .iter()
            .all(|f| !f.starts_with("crates/njutest-devkit/")),
        format!("test support entered production sources: {files:?}"),
    )?;
    require(
        files.iter().all(|f| !f.contains("/tests/")),
        format!("tests entered production sources: {files:?}"),
    )
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
        "use proc_macro::TokenStream;\n#[proc_macro_derive(AllVariants)]\npub fn all_variants(input: TokenStream) -> TokenStream { input }\n#[proc_macro_attribute]\npub fn integration(_args: TokenStream, input: TokenStream) -> TokenStream { input }\n#[proc_macro_attribute]\npub fn unit(_args: TokenStream, input: TokenStream) -> TokenStream { input }\n",
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
    let report = gates::lints(root.path())?;
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
            gates::lints(root.path()),
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
        gates::lints(root.path()),
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
        gates::lints(root.path()),
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
        gates::lints(root.path()),
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
        gates::lints(root.path()),
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
fn every_gate_that_needs_no_argument_is_one_all_runs() -> Result<(), TestFailure> {
    let root = gates::workspace_root();
    let source = std::fs::read_to_string(root.join("xtask/src/lib.rs"))?;
    let declaration = source
        .find("enum Gate {")
        .and_then(|at| source.get(at..))
        .ok_or(TestFailure::MissingGateDeclaration)?;
    let declaration = match declaration.find("\n}\n") {
        Some(end) => declaration
            .get(..end)
            .ok_or(TestFailure::MissingGateDeclaration)?,
        None => declaration,
    };

    let bare: Vec<String> = declaration
        .lines()
        .filter_map(|line| line.trim().strip_suffix(','))
        .filter(|name| {
            name.chars().next().is_some_and(char::is_uppercase)
                && name.chars().all(char::is_alphanumeric)
        })
        .filter(|name| *name != "All")
        .map(kebab)
        .collect();
    require(
        bare.len() >= 5,
        format!("the gates that need no argument are the ones a person runs as a set: {bare:?}"),
    )?;

    let report = gates::all(&root)?;
    let unrun: Vec<&String> = bare
        .iter()
        .filter(|name| !report.contains(&format!("{name}:")))
        .collect();
    require(
        unrun.is_empty(),
        format!(
            "a gate `all` does not run is a gate `mise run check` does not run and continuous \
             integration does not run: it holds nothing, and the only sign is that it is \
             still in the help. {unrun:?} is declared and `all` never calls it:\n{report}"
        ),
    )
}

/// The name a gate answers to on the command line, from the name of its variant.
fn kebab(variant: &str) -> String {
    let mut said = String::new();
    for (at, character) in variant.char_indices() {
        if character.is_uppercase() && at > 0 {
            said.push('-');
        }
        said.extend(character.to_lowercase());
    }
    said
}
