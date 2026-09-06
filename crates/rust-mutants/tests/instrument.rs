// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Instrumentation: every compilable mutant of a file lives in the file at once, dormant behind a guard, and the file keeps its line numbering.

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::PathBuf;

use rust_mutants::catalog::{Builder, Catalog};
use rust_mutants::instrument::{
    ACTIVE_ENV, CATALOG_ENV, InstrumentErrorKind, MODULE_STEM, RUNTIME_MARKER, STALE_CATALOG_EXIT,
    instrument_file, module_name, plan_file,
};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::splice::count_lines;
use rust_mutants::syntax::{Selection, discover_file};

static REGISTRY: Registry = Registry::canonical();

/// Discovers, catalogs, and instruments one source, returning the text with the file's own runtime module named `__rm`.
///
/// Every file's module carries the digest of its path so that two files
/// pasted into one scope by `include!` do not define the same item twice.
/// What each case here is about is the shape of the guards rather than which
/// eight hex characters this path came to, so the name is put back to its stem.
fn instrument(source: &str) -> String {
    let (text, _) = instrument_with_catalog(source);
    text.replace(&module_name("src/lib.rs", source), MODULE_STEM)
}

fn instrument_with_catalog(source: &str) -> (String, Catalog) {
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let file = instrument_file(
        "src/lib.rs",
        source.as_bytes(),
        &placements,
        catalog.digest(),
    )
    .expect("instrument");
    (file.text, catalog)
}

fn golden_path(name: &str) -> PathBuf {
    mjutest_devkit::paths::workspace_root()
        .join("crates/rust-mutants/tests/testdata/instrument")
        .join(name)
}

/// Instruments `<name>.input` and compares the result with `<name>.golden`.
fn golden_case(name: &str) {
    let input = std::fs::read(golden_path(&format!("{name}.input"))).expect("input");
    let source = String::from_utf8(input).expect("utf-8");
    let text = instrument(&source);
    syn::parse_file(&text).expect("the instrumented file parses");
    let (body, _runtime) = split_runtime(&text);
    assert_eq!(
        count_lines(body.as_bytes()),
        count_lines(source.as_bytes()),
        "{name}: the body kept its line count"
    );
    mjutest_devkit::golden::golden(&golden_path(&format!("{name}.golden")), text.as_bytes())
        .expect("golden");
}

/// Splits an instrumented file into the rewritten body and the appended runtime module.
fn split_runtime(text: &str) -> (&str, &str) {
    let at = text.find(RUNTIME_MARKER).expect("the runtime is appended");
    let start = text[..at]
        .rfind("\n#[doc(hidden)]")
        .expect("the runtime's first line")
        + 1;
    (&text[..start], &text[start..])
}

#[test]
fn the_constants_are_frozen() {
    assert_eq!(ACTIVE_ENV, "RUST_MUTANTS_ACTIVE");
    assert_eq!(CATALOG_ENV, "RUST_MUTANTS_CATALOG");
    assert_eq!(MODULE_STEM, "__rm");
    assert_eq!(STALE_CATALOG_EXIT, 97);
    assert_eq!(RUNTIME_MARKER, "rust-mutants-runtime-v1");
}

#[test]
fn a_boolean_position_takes_the_selector_form_and_a_value_position_the_expression_form() {
    let text = instrument(
        "pub fn f(a: i32, b: i32) -> bool {\n    if a > b {\n        return true;\n    }\n    a + 1 > b\n}\n",
    );
    assert!(
        text.contains("if (__rm::active(0) && (!(a > b)) || __rm::active(1) && (a >= b) || !(__rm::active(0)) && !(__rm::active(1)) && (a > b)) {"),
        "{text}"
    );
    assert!(
        text.contains("return (if __rm::active(2) { false } else { true });"),
        "{text}"
    );
    assert!(
        text.contains("(if __rm::active(3) { true } else if __rm::active(5) { a + 1 >= b } else {"),
        "{text}"
    );
}

#[test]
fn a_statement_takes_the_statement_form_and_a_deletion_renders_an_empty_branch() {
    let text = instrument(
        "pub fn f(v: &mut Vec<i32>, n: i32) {\n    v.push(n);\n    let mut t = 0;\n    t += n;\n    drop(t);\n}\n",
    );
    assert!(
        text.contains("if __rm::active(0) { } else { v.push(n); }"),
        "a deleted call renders an empty branch: {text}"
    );
    assert!(
        text.contains(
            "if __rm::active(1) { } else if __rm::active(2) { t -= n; } else { t += n; }"
        ),
        "one chain holds every alternative of a site: {text}"
    );
}

#[test]
fn nested_sites_become_nested_guards_and_only_the_original_branch_carries_them() {
    let text = instrument("pub fn f(a: i32, b: i32) -> bool {\n    a + 1 < b\n}\n");
    let line = text
        .lines()
        .find(|line| line.contains("__rm::active"))
        .expect("a guard");
    assert!(
        line.contains("{ a + 1 <= b }"),
        "an alternative is the pristine site with one edit and no nested guard: {line}"
    );
    assert!(
        line.contains("else { (if __rm::active("),
        "the original branch is the only one carrying the nested guard: {line}"
    );
    let (_, tail) = line.split_once("else {").expect("an original branch");
    assert_eq!(
        tail.matches("__rm::active").count(),
        1,
        "exactly the nested guard: {tail}"
    );
}

#[test]
fn the_runtime_is_appended_after_the_last_line_and_names_the_catalog() {
    let source = "pub fn f(a: i32) -> i32 {\n    a + 1\n}\n";
    let (text, catalog) = instrument_with_catalog(source);
    let (body, runtime) = split_runtime(&text);
    assert!(
        body.starts_with("#[allow(warnings, unused, unused_qualifications, unfulfilled_lint_expectations, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)] pub fn f(a: i32) -> i32 {"),
        "{body}"
    );
    assert!(runtime.contains(RUNTIME_MARKER), "{runtime}");
    assert!(
        runtime.contains(&format!("{:?}", catalog.digest())),
        "{runtime}"
    );
    for mutant in catalog.mutants() {
        assert!(
            runtime.contains(&format!("({:?}, {})", mutant.id, mutant.index)),
            "{runtime}"
        );
    }
    assert!(
        runtime.contains(ACTIVE_ENV) && runtime.contains(CATALOG_ENV),
        "{runtime}"
    );
    assert!(
        runtime.contains("exit(97)"),
        "a stale catalog ends the process: {runtime}"
    );
    assert!(!runtime.contains("unsafe"), "{runtime}");
    assert!(text.ends_with("}\n"), "{text}");
}

#[test]
fn an_untouched_file_is_returned_byte_for_byte_with_no_runtime() {
    let source = "//! No candidates here.\n\npub struct S;\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    assert!(discovery.candidates.is_empty());
    let catalog = Builder::new().build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let file = instrument_file(
        "src/lib.rs",
        source.as_bytes(),
        &placements,
        catalog.digest(),
    )
    .expect("instrument");
    assert_eq!(file.text, source);
    assert!(file.guards.is_empty());
    assert!(!file.instrumented);
}

#[test]
fn the_innermost_function_carries_the_allow_and_carries_it_once() {
    let text = instrument(
        "pub fn f(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn g(a: i32) -> i32 {\n    fn inner(x: i32) -> i32 { x * 2 }\n    inner(a) - 1\n}\n",
    );
    assert_eq!(
        text.matches("#[allow(").count(),
        4,
        "three functions and the runtime: {text}"
    );
    assert!(
        text.contains("#[allow(warnings, unused, unused_qualifications, unfulfilled_lint_expectations, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)] pub fn f(a: i32, b: i32) -> i32 {"),
        "{text}"
    );
    assert!(
        text.contains("#[allow(warnings, unused, unused_qualifications, unfulfilled_lint_expectations, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)] fn inner(x: i32) -> i32 {"),
        "{text}"
    );
    assert!(
        text.contains("#[allow(warnings, unused, unused_qualifications, unfulfilled_lint_expectations, clippy::all, clippy::pedantic, clippy::restriction, clippy::nursery, clippy::cargo)] pub fn g(a: i32) -> i32 {"),
        "{text}"
    );
}

#[test]
fn a_file_that_already_spells_the_module_name_gets_the_next_one() {
    let taken = module_name("src/lib.rs", "");
    let source = format!(
        "mod {taken} {{\n    pub fn helper() -> i32 {{ 1 }}\n}}\n\npub fn f() -> i32 {{\n    {taken}::helper() + 1\n}}\n"
    );

    let (text, _) = instrument_with_catalog(&source);

    assert!(text.contains(&format!("{taken}1::active(")), "{text}");
    assert!(text.contains(&format!("mod {taken}1 {{")), "{text}");
    assert!(
        text.contains(&format!("{taken}::helper()")),
        "the file's own name is untouched: {text}"
    );
}

#[test]
fn each_file_names_its_runtime_module_after_its_own_path() {
    let one = module_name("src/lib.rs", "");
    let other = module_name("src/items.rs", "");

    assert!(one.starts_with(&format!("{MODULE_STEM}_")), "{one}");
    assert_ne!(
        one, other,
        "`include!` at item position pastes one file's items into another's module, and two \
         modules of the same name in one scope are the same item defined twice: every mutant \
         of both files would then come back refused by an error that names neither"
    );
    assert_eq!(
        one,
        module_name("src/lib.rs", ""),
        "and it is a function of the path"
    );
}

#[test]
fn an_inline_module_reaches_the_runtime_through_super() {
    let text = instrument(
        "pub mod outer {\n    pub mod inner {\n        pub fn f(a: i32) -> i32 { a + 1 }\n    }\n    pub fn g(a: i32) -> i32 { a - 1 }\n}\n",
    );
    assert!(text.contains("super::super::__rm::active("), "{text}");
    assert!(
        text.contains("super::__rm::active(") && !text.contains("{ super::super::__rm::active(2)"),
        "{text}"
    );
}

#[test]
fn line_endings_and_non_ascii_bytes_survive() {
    let source = "pub fn 加算(α: i32, β: i32) -> i32 {\r\n    α + β\r\n}\r\n";
    let text = instrument(source);
    assert!(text.contains("α + β"), "{text}");
    let (body, runtime) = split_runtime(&text);
    assert_eq!(
        body.matches("\r\n").count(),
        3,
        "the body keeps its CRLF: {body}"
    );
    assert!(
        runtime.contains("\r\n"),
        "the runtime matches the file: {runtime}"
    );
    assert!(
        !runtime.contains("\n\n"),
        "no bare LF in a CRLF file: {runtime:?}"
    );
}

#[test]
fn a_multi_line_site_keeps_its_lines_because_only_the_original_branch_holds_them() {
    let source = "pub fn f(a: i32, b: i32) -> bool {\n    if a < b\n        && b > 0\n    {\n        return true;\n    }\n    false\n}\n";
    let text = instrument(source);
    let (body, _) = split_runtime(&text);
    assert_eq!(count_lines(body.as_bytes()), count_lines(source.as_bytes()));
    assert!(
        body.contains("(!(a < b && b > 0))"),
        "an alternative is folded onto one line: {body}"
    );
    assert!(
        body.contains(")\n        && (__rm::active("),
        "the original branch is where the line break stayed: {body}"
    );
}

#[test]
fn a_placement_naming_an_unknown_mutant_is_refused() {
    let source = "pub fn f(a: i32) -> i32 { a + 1 }\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let catalog = Builder::new().build().expect("empty catalog");
    let error = plan_file(&catalog, "src/lib.rs", &discovery.candidates).unwrap_err();
    assert_eq!(error.kind(), InstrumentErrorKind::UnknownMutant);
    assert!(error.to_string().contains("RM3001"), "{error}");
}

#[test]
fn a_source_that_is_not_the_one_the_candidates_came_from_is_refused() {
    let source = "pub fn f(a: i32) -> i32 { a + 1 }\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let other = b"pub fn f(a: i32) -> i32 { a - 1 }\n";
    let error = instrument_file("src/lib.rs", other, &placements, catalog.digest()).unwrap_err();
    assert_eq!(error.kind(), InstrumentErrorKind::SourceMismatch);
}

#[test]
fn the_recorded_cases_are_rewritten_exactly_as_recorded() {
    for name in ["forms", "nested", "statements", "modules"] {
        golden_case(name);
    }
}

#[test]
fn every_alternative_reports_where_its_own_text_landed() {
    let source = "pub fn f(a: i32, b: i32) -> bool {\n    if a + 1 > b {\n        return true;\n    }\n    false\n}\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let file = instrument_file(
        "src/lib.rs",
        source.as_bytes(),
        &placements,
        catalog.digest(),
    )
    .expect("instrument");

    let mut indices: Vec<u32> = file.branches.iter().map(|branch| branch.index).collect();
    indices.sort_unstable();
    let mut expected: Vec<u32> = catalog
        .mutants()
        .iter()
        .map(|mutant| mutant.index)
        .collect();
    expected.sort_unstable();
    assert_eq!(indices, expected);

    let text_of = |index: u32| -> String {
        let branch = file
            .branches
            .iter()
            .find(|branch| branch.index == index)
            .expect("a branch");
        file.text[branch.span.start as usize..branch.span.end as usize].to_owned()
    };
    let by_rule = |rule: &str| -> u32 {
        catalog
            .mutants()
            .iter()
            .find(|mutant| mutant.candidate.rule.name == rule)
            .expect(rule)
            .index
    };
    assert_eq!(text_of(by_rule("add-to-sub")), "a - 1");
    assert_eq!(text_of(by_rule("gt-to-ge")), "a + 1 >= b");
    assert_eq!(text_of(by_rule("negate-condition")), "!(a + 1 > b)");
    assert_eq!(text_of(by_rule("true-to-false")), "false");

    let inner = file
        .branches
        .iter()
        .find(|branch| branch.index == by_rule("add-to-sub"))
        .expect("the nested branch");
    let outer = file
        .branches
        .iter()
        .find(|branch| branch.index == by_rule("gt-to-ge"))
        .expect("the enclosing site's alternative");
    assert!(
        !outer.span.contains(inner.span),
        "an alternative carries no nested guard"
    );
    let length = u32::try_from(file.text.len()).expect("a small file");
    assert!(file.branches.iter().all(|branch| branch.span.end <= length));
}

#[test]
fn the_allow_names_every_lint_a_guard_can_trip_rather_than_the_warning_group() {
    // `#[allow(warnings)]` covers only lints that are still at warn level.
    // A workspace that denies `unused` or clippy's pedantic set has taken
    // them out of that group, and a guard's parentheses would then fail the
    // build of every mutant at once.
    for lint in [
        "warnings",
        "unused",
        "unfulfilled_lint_expectations",
        "clippy::all",
        "clippy::pedantic",
        "clippy::restriction",
        "clippy::nursery",
    ] {
        assert!(
            rust_mutants::instrument::ALLOW_ATTRIBUTE.contains(lint),
            "{lint} is not named: {}",
            rust_mutants::instrument::ALLOW_ATTRIBUTE
        );
    }
}
