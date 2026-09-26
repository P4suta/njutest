// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the command line answers about a real tree: what it lists, what it catalogs, and what it exits with.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::ffi::OsString;

use njutest_devkit::fixture::Fixture;
use rust_mutants::runner::Cancel;
use rust_mutants_cli::{Environment, Streams};
use std::process::Output;

/// Runs the binary against a fixture, with the environment a real run has.
fn stdout(output: &Output) -> String {
    njutest_devkit::process::strict_utf8(&output.stdout).into_owned()
}

fn against(fixture: &Fixture, args: &[&str]) -> Output {
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        std::iter::once("rust-mutants")
            .chain(args.iter().copied().take(1))
            .chain(["--root", root.as_str()])
            .chain(["--tier", "all"])
            .chain(["--offline", "--locked"])
            .chain(args.iter().copied().skip(1))
            .map(OsString::from),
        &environment(fixture),
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    njutest_devkit::process::answered(code, out, err)
}
#[test]
fn list_names_every_candidate_without_building_anything() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["list"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = stdout(&output);
    let rows: Vec<&str> = text.lines().filter(|line| line.contains(" => ")).collect();
    assert_eq!(rows.len(), 13, "{text}");
    assert!(
        rows.iter().all(|line| line.contains("src/lib.rs:")),
        "{text}"
    );
    assert!(
        text.contains("13 candidates, which is what the rules propose"),
        "a list says how many it listed and what listing them is not: {text}"
    );
    assert!(text.contains("gt-to-ge@1"), "{text}");
    assert!(text.contains("utf8:\">\" => utf8:\">=\""), "{text}");
    assert!(
        !text.contains("LosslessBytes"),
        "the human boundary exposed a renderer's Debug representation: {text}"
    );
}
#[test]
fn why_skipped_tallies_the_reasons_with_a_sentence_each() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["why-skipped"]);
    assert_eq!(output.status.code(), Some(0));
    let text = stdout(&output);
    assert!(text.contains("test-code"), "{text}");
    assert!(text.contains("test-only-file"), "{text}");
    assert!(
        text.contains("measures itself"),
        "the reason is explained: {text}"
    );
}
#[test]
fn catalog_says_what_compiles_and_what_the_compiler_refused() {
    let fixture = Fixture::copy("fixture-rejectable");
    let output = against(&fixture, &["catalog", "--no-verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("refused by the compiler:"), "{text}");
    assert!(text.contains("cannot subtract"), "{text}");
    assert!(text.contains("accepted, 4 refused"), "{text}");
}
#[test]
fn catalog_as_json_is_one_document_a_program_can_read() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["catalog", "--json"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&stdout(&output)).expect("one JSON document");
    assert_eq!(document["document_type"], "rust-mutants/catalog");
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["tool_version"], rust_mutants::VERSION);
    assert_eq!(
        document["workspace"]["catalog_digest"]
            .as_str()
            .map(str::len),
        Some(64)
    );
    let skips = document["skips"].as_array().expect("skips");
    let observed: Vec<(&str, u64)> = skips
        .iter()
        .map(|skip| {
            (
                skip["reason"].as_str().expect("a named skip reason"),
                skip["count"].as_u64().expect("an exact skip count"),
            )
        })
        .collect();
    assert_eq!(
        observed,
        [
            ("test-code", 22),
            ("test-only-file", 6),
            ("let-condition", 1),
        ],
        "the all-tier fixture's complete skip ledger drifted"
    );
}
#[test]
fn explain_says_everything_known_about_one_mutant() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = stdout(&against(&fixture, &["list"]));
    let short = listed
        .lines()
        .find(|line| line.contains("return-default"))
        .and_then(|line| line.split_whitespace().next())
        .expect("a return-default mutant")
        .to_owned();

    let output = against(&fixture, &["explain", &short, "--fresh", "--no-verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains(&format!("SHORT     {short}")), "{text}");
    assert!(
        text.contains("RULE      return-default@1 (return-replacement)"),
        "{text}"
    );
    assert!(
        text.contains("OUTCOME   no stored run answers for it"),
        "a tree prepared for the purpose says what the mutation is, not what became of it: \
         {text}"
    );
    assert!(
        text.contains("REPRODUCE rust-mutants run --mutant"),
        "{text}"
    );
}
#[test]
fn run_exits_by_what_the_tests_said() {
    let fixture = Fixture::copy("fixture-simple");
    let listed = stdout(&against(&fixture, &["list"]));
    let short_of = |rule: &str| -> String {
        listed
            .lines()
            .find(|line| line.contains(rule))
            .and_then(|line| line.split_whitespace().next())
            .unwrap_or_else(|| panic!("a {rule} mutant"))
            .to_owned()
    };

    let killed = against(&fixture, &["run", "--mutant", &short_of("return-default")]);
    assert_eq!(
        killed.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&killed.stderr)
    );
    let text = stdout(&killed);
    assert!(text.contains("killed"), "{text}");
    assert!(text.contains("fixture-simple/lib/fixture_simple"), "{text}");

    let survived = against(&fixture, &["run", "--mutant", &short_of("gt-to-ge")]);
    assert_eq!(survived.status.code(), Some(1), "{}", stdout(&survived));
    assert!(stdout(&survived).contains("survived"));

    let unknown = against(&fixture, &["run", "--mutant", "ffffffff"]);
    assert_eq!(unknown.status.code(), Some(2));
    let said = njutest_devkit::process::strict_utf8(&unknown.stderr);
    assert!(said.contains("RM5003"), "{said}");
}
#[test]
fn instrument_prints_one_file_as_the_engine_rewrites_it() {
    let fixture = Fixture::copy("fixture-simple");
    let output = against(&fixture, &["instrument", "--file", "src/lib.rs"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = stdout(&output);
    let module =
        rust_mutants::instrument::module_name("src/lib.rs", "").expect("valid source tokens");
    assert!(
        text.contains(&format!("{module}::active(")) && text.contains(&format!("mod {module} {{")),
        "{text}"
    );
    assert!(
        text.contains(&format!(
            "{allow}\nmod {module} {{",
            allow = rust_mutants::instrument::GENERATED_MODULE_ALLOW_ATTRIBUTE
        )),
        "only the private generated module owns the exact lint exception: {text}"
    );
    assert!(
        !text.contains("#[allow(warnings") && !text.contains("#[allow(unused) pub fn max"),
        "instrumentation must not suppress a diagnostic in user code: {text}"
    );

    let source = std::fs::read_to_string(fixture.root().join("src/lib.rs")).expect("read");
    assert!(
        !source.contains("__rm"),
        "the source workspace is read-only"
    );

    let missing = against(&fixture, &["instrument", "--file", "src/nope.rs"]);
    assert_eq!(missing.status.code(), Some(2));
    let refusal = njutest_devkit::process::strict_utf8(&missing.stderr).into_owned();
    assert!(
        refusal.contains("RM0004") && refusal.contains("src/nope.rs"),
        "a path the workspace does not hold is a value the flag cannot take, which is \
         what a person mistyping a name needs to read; it used to be reported as a tree \
         that could not be written, which sends them looking at the disk: {refusal}"
    );
    assert!(
        refusal.contains("--file"),
        "and the flag is named: {refusal}"
    );
}
#[test]
fn the_catalog_document_validates_against_its_schema() {
    let schema_path =
        njutest_devkit::paths::workspace_root().join("schema/rust-mutants-catalog-v1.json");
    let schema: serde_json::Value = njutest_devkit::strictjson::decode_str(
        &std::fs::read_to_string(&schema_path).expect("the schema"),
    )
    .expect("the schema is JSON");
    let validator = jsonschema::validator_for(&schema).expect("the schema compiles");

    for fixture_name in ["fixture-simple", "fixture-rejectable"] {
        let fixture = Fixture::copy(fixture_name);
        let output = against(&fixture, &["catalog", "--json", "--no-verify"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{}",
            njutest_devkit::process::strict_utf8(&output.stderr)
        );
        let document: serde_json::Value =
            njutest_devkit::strictjson::decode_str(&stdout(&output)).expect("one JSON document");
        let problems: Vec<String> = validator
            .iter_errors(&document)
            .map(|error| format!("{} at {}", error, error.instance_path()))
            .collect();
        assert!(problems.is_empty(), "{fixture_name}: {problems:?}");

        let mutants = document["mutants"].as_array().expect("mutants");
        assert!(!mutants.is_empty(), "{fixture_name}");
        assert!(
            mutants
                .iter()
                .all(|mutant| mutant["line"].as_u64().unwrap_or(0) > 0),
            "every mutant is placed in the file a person would open"
        );
        assert_eq!(document["selection"]["tier"], "all");
        assert_eq!(
            document["workspace"]["root_name"], fixture_name,
            "the workspace names itself"
        );
    }
}

#[test]
fn equivalence_says_what_the_compiler_renders_identically_and_never_says_equivalent() {
    let fixture = Fixture::copy("fixture-equivalent");
    let output = against(&fixture, &["equivalence"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = stdout(&output);
    let rows: Vec<&str> = text.lines().filter(|line| line.contains('\t')).collect();
    assert!(!rows.is_empty(), "{text}");
    for row in &rows {
        let columns: Vec<&str> = row.split('\t').collect();
        if columns[0] == "EQUIVALENCE" {
            continue;
        }
        assert_eq!(columns.len(), 4, "{row}");
        assert!(
            if njutest_devkit::reproducible::builds_the_same_twice() {
                columns[1] == "identical" || columns[1] == "differs"
            } else {
                columns[1] == "not-established"
            },
            "an answer is about what the compiler rendered, not about what the tests could \
             notice — and on a machine that renders one unchanged tree two ways there is no \
             answer to give: {row}"
        );
    }
    assert!(
        text.contains("identical is not equivalent"),
        "the summary says what identical does not mean: {text}"
    );
    assert!(
        !text.contains("\tequivalent\t") && !text.contains(" equivalent "),
        "a mutation of a function nothing links comes out identical for the opposite of a \
         reassuring reason, so this command never says equivalent: {text}"
    );
}

#[test]
fn the_equivalence_tally_counts_the_rows_it_printed() {
    let fixture = Fixture::copy("fixture-equivalent");
    let output = against(&fixture, &["equivalence"]);
    let text = stdout(&output);
    let rows: Vec<Vec<&str>> = text
        .lines()
        .filter(|line| line.contains('\t') && !line.starts_with("EQUIVALENCE"))
        .map(|line| line.split('\t').collect())
        .collect();
    let identical = rows
        .iter()
        .filter(|columns| columns.get(1) == Some(&"identical"))
        .count();
    if njutest_devkit::reproducible::builds_the_same_twice() {
        assert!(
            identical > 0,
            "this fixture is built at an optimisation level where the compiler renders \
             `x + 0` and `x - 0` the same, which is the whole reason it exists: {text}"
        );
    } else {
        assert!(
            rows.iter()
                .all(|columns| columns.get(1) == Some(&"not-established")),
            "a machine that renders one unchanged tree two ways establishes nothing here, \
             and a row saying the compiler rendered a mutation would be reporting the \
             machine's own difference as the mutation's: {text}"
        );
    }
    assert!(
        text.contains(&format!(
            "asked={} of {}\tidentical={identical}",
            rows.len(),
            rows.len()
        )),
        "the tally is the count of the rows above it, and a tally that counted none \
         would read exactly like a tree the compiler renders every mutation of: {text}"
    );
}

#[test]
fn equivalence_asks_about_at_most_the_limit_it_was_given() {
    let fixture = Fixture::copy("fixture-equivalent");
    let output = against(&fixture, &["equivalence", "--limit", "2"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        njutest_devkit::process::strict_utf8(&output.stderr)
    );
    let text = stdout(&output);
    assert!(text.contains("asked=2"), "{text}");
    let rows = text
        .lines()
        .filter(|line| line.contains('\t') && !line.starts_with("EQUIVALENCE"))
        .count();
    assert_eq!(rows, 2, "{text}");
}

fn environment(fixture: &Fixture) -> Environment {
    Environment {
        vars: njutest_devkit::paths::environment_for_a_run(),
        temp_directory: fixture.temp().to_path_buf(),
        program: std::path::PathBuf::from("this test never runs it"),
        cache_directory: fixture.cache().to_path_buf(),
        working_directory: fixture.root().to_path_buf(),
        no_color: true,
        stdout_is_terminal: false,
        paints: false,
        ci: rust_mutants_cli::CiHost::None,
    }
}

#[test]
fn cargo_rust_mutants_drops_the_word_cargo_gave_it_and_reads_the_rest() {
    let asked = rust_mutants_cli::cli::parse(
        ["cargo-rust-mutants", "rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("cargo calls its subcommands with their own name in argv[1]");
    let direct = rust_mutants_cli::cli::parse(
        ["rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("and a person calls it without");

    assert_eq!(
        format!("{asked:?}"),
        format!("{direct:?}"),
        "`cargo rust-mutants list` and `rust-mutants list` are one command asked for two \
         ways"
    );
}

#[test]
fn a_binary_that_is_not_a_cargo_subcommand_keeps_every_argument_it_was_given() {
    let refused = rust_mutants_cli::cli::parse(
        ["rust-mutants", "rust-mutants", "list"]
            .into_iter()
            .map(OsString::from),
    );
    assert!(
        refused.is_err(),
        "dropping a repeated word is cargo's convention and not this program's: called \
         directly, `rust-mutants rust-mutants list` is a mistake and is said to be one"
    );
}

#[test]
fn the_word_cargo_repeats_is_dropped_once_and_only_where_it_is_the_subcommand() {
    let twice = rust_mutants_cli::cli::parse(
        ["cargo-rust-mutants", "rust-mutants", "rust-mutants", "list"]
            .into_iter()
            .map(OsString::from),
    );
    assert!(
        twice.is_err(),
        "one repeat is cargo's; a second is a mistake, and dropping both would run a \
         command nobody asked for"
    );

    let elsewhere = rust_mutants_cli::cli::parse(
        ["cargo-rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("cargo may be called without the repeat, and the rest is still the command");
    let direct = rust_mutants_cli::cli::parse(
        ["rust-mutants", "list", "--offline"]
            .into_iter()
            .map(OsString::from),
    )
    .expect("which is this command");
    assert_eq!(format!("{elsewhere:?}"), format!("{direct:?}"));
}

/// A directory holding `cargo` and `rustc` scripts that run the real ones only from `trusted`, and refuse every other directory as mise refuses a configuration nobody trusted there.
#[cfg(unix)]
#[expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]
fn refusing_shims(fixture: &Fixture) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt as _;
    let trusted = std::fs::canonicalize(fixture.root()).expect("the fixture's root");
    let sysroot = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .current_dir(&trusted)
        .output()
        .expect("rustc names its toolchain");
    let sysroot =
        std::path::PathBuf::from(njutest_devkit::process::strict_utf8(&sysroot.stdout).trim());
    let shims = fixture.temp().join("shims");
    std::fs::create_dir_all(&shims).expect("the shims' directory");
    for name in ["cargo", "rustc"] {
        let path = shims.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\nif [ \"$(pwd -P)\" != '{}' ]; then echo \"mise ERROR Config files in $(pwd -P)/mise.toml are not trusted.\" >&2; exit 1; fi\nexec '{}' \"$@\"\n",
                trusted.display(),
                sysroot.join("bin").join(name).display()
            ),
        )
        .expect("a shim");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755))
            .expect("a shim that runs");
    }
    shims
}

#[cfg(unix)]
#[test]
fn a_test_that_runs_a_bare_cargo_gets_the_runs_toolchain_rather_than_a_shim_that_refuses_the_copy()
{
    let fixture = Fixture::copy("fixture-bare-cargo");
    let shims = refusing_shims(&fixture);
    let mut given = environment(&fixture);
    let searched = given
        .vars
        .iter()
        .find(|(name, _)| name == "PATH")
        .map(|(_, value)| value.clone());
    let path = std::env::join_paths(
        std::iter::once(shims).chain(searched.iter().flat_map(std::env::split_paths)),
    )
    .expect("a search path");
    given.vars.retain(|(name, _)| name != "PATH");
    given.vars.push(("PATH".into(), path));
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        [
            "rust-mutants",
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
            "--root",
            root.as_str(),
        ]
        .map(OsString::from),
        &given,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert_eq!(
        output.status.code(),
        Some(0),
        "a shim that chooses the toolchain by the directory it runs in refuses the copy a run \
         measures, so a test that runs `cargo` by its bare name fails there for a reason none of \
         its code holds; the run gives every test its own toolchain first instead: {output:?}"
    );
}

/// A test that reads a setting only the home the run was given holds, which a confined execution cannot see.
const READS_THE_GIVEN_HOME: &str = "// SPDX-FileCopyrightText: 2026 njutest contributors\n// SPDX-License-Identifier: MIT OR Apache-2.0\n\n//! Reads a setting only the given home holds.\n\n#[test]\nfn the_setting_the_home_already_holds_is_the_one_recalled() {\n    assert_eq!(fixture_home::recall().expect(\"the home holds a setting\"), \"already there\");\n}\n";

/// The environment every run gets, with `home` as the home directory and the toolchain's own homes still where they are.
fn given_home(fixture: &Fixture, home: &std::path::Path) -> Environment {
    let mut given = environment(fixture);
    let real = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .map(std::path::PathBuf::from);
    for (name, beside) in [("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")] {
        let pinned = std::env::var_os(name)
            .or_else(|| real.as_ref().map(|home| home.join(beside).into_os_string()));
        given.vars.retain(|(held, _)| held != name);
        if let Some(pinned) = pinned {
            given.vars.push((name.into(), pinned));
        }
    }
    for name in ["HOME", "USERPROFILE"] {
        given.vars.retain(|(held, _)| held != name);
        given.vars.push((name.into(), home.as_os_str().to_owned()));
    }
    given
}

#[test]
fn a_write_a_test_makes_under_its_home_lands_in_its_execution() {
    let fixture = Fixture::copy("fixture-home");
    std::fs::write(fixture.root().join("tests/reads.rs"), READS_THE_GIVEN_HOME)
        .expect("a test that reads the given home");
    let home = fixture.temp().join("given-home");
    let setting = home.join(".fixture-home").join("setting");
    std::fs::create_dir_all(setting.parent().expect("the setting's directory"))
        .expect("the given home");
    std::fs::write(&setting, "already there").expect("a setting the given home holds");
    let given = given_home(&fixture, &home);
    let root = njutest_devkit::paths::utf8(fixture.root()).to_owned();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let code = rust_mutants_cli::run_from(
        [
            "rust-mutants",
            "run",
            "--offline",
            "--locked",
            "--tier",
            "all",
            "--no-coverage",
            "--ui",
            "quiet",
            "--root",
            root.as_str(),
        ]
        .map(OsString::from),
        &given,
        &Cancel::new(),
        Streams {
            out: &mut out,
            err: &mut err,
        },
    );
    let output = njutest_devkit::process::answered(code, out, err);
    assert!(
        output.status.code().is_some_and(|code| code < 2),
        "the run reaches a verdict: {output:?}"
    );
    assert_eq!(
        std::fs::read_to_string(&setting).expect("the given home's setting"),
        "already there",
        "a test that writes under its home writes the home of its own execution, so the home the \
         run was given keeps what it held, whatever a mutation did to the path: {output:?}"
    );
    let document: serde_json::Value =
        njutest_devkit::strictjson::decode_str(&njutest_devkit::fixture::stored_report(
            &rust_mutants_cli::app::stored::Store::read(fixture.root()).root(),
        ))
        .expect("the report is a document");
    let limited: Vec<String> = document["targets"]
        .as_array()
        .expect("the targets")
        .iter()
        .filter(|target| {
            target["limitations"]
                .as_array()
                .is_some_and(|said| said.iter().any(|one| one == "unconfined-target"))
        })
        .map(|target| target["id"].to_string())
        .collect();
    assert_eq!(
        limited,
        vec!["\"fixture-home/test/reads\"".to_owned()],
        "a target that passes only with the given home is measured with it, and the run says \
         which, every time: {}",
        document["targets"]
    );
}
