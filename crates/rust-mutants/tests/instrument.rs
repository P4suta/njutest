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

use std::collections::BTreeSet;
use std::path::PathBuf;

use rust_mutants::catalog::{Builder, Catalog};
use rust_mutants::instrument::{
    ACTIVE_ENV, CATALOG_ENV, InstrumentErrorKind, Instrumenting, MODULE_STEM, RUNTIME_MARKER,
    STALE_CATALOG_EXIT, instrument_file, module_name, plan_file,
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
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &offered(&discovery, &catalog),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    (file.text, catalog)
}

/// Every mutant the syntax offers a comparison for, as a run has it once the compiler has vouched for the operands.
///
/// A test cannot ask the compiler, so it asks for all of them: what the
/// goldens are about is the shape of a guard that compares, and a set the
/// compiler pruned would only make the recorded shape depend on which
/// operands this case happened to spell.
fn offered(discovery: &rust_mutants::syntax::FileDiscovery, catalog: &Catalog) -> BTreeSet<u32> {
    discovery
        .candidates
        .iter()
        .filter(|found| found.comparable.is_some())
        .filter_map(|found| found.candidate.id().ok())
        .filter_map(|id| catalog.by_id(id.as_str()))
        .map(|mutant| mutant.index)
        .collect()
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
        "pub fn f(a: i32, b: i32, c: i32) -> bool {\n    if a > b {\n        return true;\n    }\n    a + c > b\n}\n",
    );
    assert!(
        text.contains("if (__rm::active(0) && (!(a > b)) || __rm::active(1) && (a >= b) || !(__rm::active(0)) && !(__rm::active(1)) && __rm::differing(1, (a > b), || (a >= b))) {"),
        "{text}"
    );
    assert!(
        text.contains("return (if __rm::active(2) { false } else { true });"),
        "{text}"
    );
    assert!(
        text.contains("(if __rm::active(3) { true } else if __rm::active(5) { a + c >= b } else {"),
        "{text}"
    );
}

#[test]
fn a_guard_the_compiler_vouched_for_answers_what_it_replaces_and_says_where_the_two_part() {
    let source = "pub fn f(a: i32, b: i32) -> bool {\n    if a > b {\n        return true;\n    }\n    false\n}\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let compared = offered(&discovery, &catalog);
    assert!(
        !compared.is_empty(),
        "widening `>` inside an inert condition compares"
    );

    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &compared,
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    assert_eq!(
        file.compared,
        compared.iter().copied().collect::<Vec<u32>>(),
        "the tree reports what it does rather than what it was offered"
    );

    let unoffered = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    assert!(
        unoffered.compared.is_empty(),
        "and a tree offered nothing compares nothing"
    );
    assert!(
        !unoffered.text.contains("__rm::differing"),
        "{}",
        unoffered.text
    );
}

#[test]
fn a_site_whose_form_cannot_compare_reports_no_comparison_however_it_is_offered() {
    let source = "pub fn f(a: i32, b: i32) -> i32 {\n    let larger = if a > b { a } else { b };\n    larger + 1\n}\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let every: BTreeSet<u32> = placements.iter().map(|placement| placement.index).collect();
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &every,
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    let value_sites: Vec<u32> = file
        .guards
        .iter()
        .filter(|guard| guard.form != rust_mutants::syntax::Form::C)
        .map(|guard| guard.index)
        .collect();
    assert!(
        !value_sites.is_empty(),
        "the let binding is a value position"
    );
    assert!(
        value_sites
            .iter()
            .all(|index| !file.compared.contains(index)),
        "a chain has nowhere to put the comparison, and says so: {:?} of {value_sites:?}",
        file.compared
    );
}

/// Instruments `source` with `markers` and hands back what the file became.
fn instrumented_with_markers(
    source: &str,
    markers: &[rust_mutants::syntax::branch::Marker],
) -> rust_mutants::instrument::FileOutput {
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers,
        comparable: &BTreeSet::default(),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument")
}

/// The marker a body starting at `opening` would carry.
fn marker_at(source: &str, opening: &str) -> rust_mutants::syntax::branch::Marker {
    let at = u32::try_from(source.find(opening).expect("the gated body")).expect("a small file");
    rust_mutants::syntax::branch::Marker {
        at: at + 1,
        index: 0,
        super_depth: 0,
    }
}

#[test]
fn a_body_a_guard_writes_twice_takes_no_marker_and_the_file_says_which_it_holds() {
    let outside = "pub fn f(a: i32, b: i32) -> i32 {\n    if a <= b { return 1; }\n    0\n}\n";
    let file = instrumented_with_markers(outside, &[marker_at(outside, "{ return 1; }")]);
    assert_eq!(
        file.marked,
        vec![0],
        "a body no guard site covers takes its marker: {}",
        file.text
    );
    assert!(
        file.text.contains(&format!("{}::body(0); ", file.module)),
        "{}",
        file.text
    );

    let inside = "pub fn f(v: &mut Vec<i32>, a: i32, b: i32) {\n    v.push(if a <= b { 1 } else { 2 });\n}\n";
    let file = instrumented_with_markers(inside, &[marker_at(inside, "{ 1 }")]);
    assert!(
        file.marked.is_empty(),
        "a guard writes its site twice and one splice cannot land in both, so the marker is \
         dropped and the file says so: {}",
        file.text
    );
    assert!(
        !file.text.contains(&format!("{}::body(0); ", file.module)),
        "{}",
        file.text
    );
}

#[test]
fn a_statement_takes_the_statement_form_and_a_deletion_renders_an_empty_branch() {
    let text = instrument(
        "pub fn f(v: &mut Vec<i32>, n: i32) {\n    v.push(n);\n    let mut t = n;\n    t += n;\n    drop(t);\n}\n",
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
    let text = instrument("pub fn f(a: i32, b: i32, c: i32) -> bool {\n    a + c < b\n}\n");
    let line = text
        .lines()
        .find(|line| line.contains("__rm::active"))
        .expect("a guard");
    assert!(
        line.contains("{ a + c <= b }"),
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
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        catalog_digest: catalog.digest(),
    })
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
    let error = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: other,
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        catalog_digest: catalog.digest(),
    })
    .unwrap_err();
    assert_eq!(error.kind(), InstrumentErrorKind::SourceMismatch);
}

#[test]
fn the_recorded_cases_are_rewritten_exactly_as_recorded() {
    for name in ["forms", "nested", "statements", "modules", "arms"] {
        golden_case(name);
    }
}

#[test]
fn the_crlf_variant_of_every_recorded_case_keeps_its_lines_and_reparses() {
    for name in ["forms", "nested", "statements", "modules", "arms"] {
        let input = std::fs::read(golden_path(&format!("{name}.input"))).expect("input");
        let source = rust_mutants::testkit::source::crlf(&String::from_utf8(input).expect("utf-8"));
        let text = instrument(&source);
        syn::parse_file(&text).expect("the instrumented file parses");
        let (body, _runtime) = split_runtime(&text);
        assert_eq!(
            count_lines(body.as_bytes()),
            count_lines(source.as_bytes()),
            "{name}: a file whose lines end the other way keeps its line count too"
        );
        assert!(
            body.contains("\r\n"),
            "{name}: the rewrite kept the endings the file had"
        );
    }
}

#[test]
fn a_crlf_file_is_rewritten_the_same_way_and_mints_its_own_identities() {
    for name in ["forms", "nested", "statements", "modules", "arms"] {
        let input = std::fs::read(golden_path(&format!("{name}.input"))).expect("input");
        let source = String::from_utf8(input).expect("utf-8");
        let with_crlf = rust_mutants::testkit::source::crlf(&source);
        let one = instrument(&source);
        let other = instrument(&with_crlf);
        let (body, runtime) = split_runtime(&one);
        let (crlf_body, crlf_runtime) = split_runtime(&other);
        assert_eq!(
            rust_mutants::testkit::source::lf(body),
            rust_mutants::testkit::source::lf(crlf_body),
            "{name}: the same program with the other line endings is rewritten the same way"
        );
        assert_ne!(
            runtime, crlf_runtime,
            "{name}: an identity hashes the exact bytes of the file, so a file whose lines end \
             the other way is a different file and mints different identities"
        );
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
    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        catalog_digest: catalog.digest(),
    })
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

/// A file of `functions` generated functions, each with a comparison, a branch, and a tail.
fn generated(functions: usize) -> String {
    use std::fmt::Write as _;
    let mut text = String::from("//! A generated module.\n\n");
    for index in 0..functions {
        let _written = writeln!(
            text,
            "pub fn f{index}(a: i32, b: i32) -> i32 {{\n    \
             let mut total = 0;\n    \
             if a > b {{\n        total += a - b;\n    }} else {{\n        total += b - a;\n    }}\n    \
             total + a * b\n}}\n"
        );
    }
    text
}

/// A match of `arms` arms, some guarded, ending in a bare wildcard.
fn generated_match(arms: usize, guarded: bool) -> String {
    use std::fmt::Write as _;
    let mut text =
        String::from("//! A generated match.\n\npub fn pick(n: i32) -> i32 {\n    match n {\n");
    for index in 0..arms {
        let _written = if guarded && index % 2 == 1 {
            writeln!(
                text,
                "        {index}\n        | {} if n > {index} => {index},",
                index.saturating_add(1000)
            )
        } else {
            writeln!(text, "        {index} => {index},")
        };
    }
    text.push_str("        _ => -1,\n    }\n}\n");
    text
}

proptest::proptest! {
    /// However many arms a match holds and whichever way its lines end, writing a guard onto one keeps the line count and leaves a file that parses.
    ///
    /// Form M writes a guard where the source had none, which is the one
    /// splice that adds syntax rather than replacing it. A pattern that spans
    /// several lines, an `|` alternation, and an arm that already has a guard
    /// are the shapes it has to get right.
    #[test]
    fn arm_guards_keep_line_counts_and_reparse_for_every_generated_shape(
        arms in 1usize..8,
        guarded in proptest::bool::ANY,
        windows in proptest::bool::ANY
    ) {
        let generated = generated_match(arms, guarded);
        let source = if windows {
            rust_mutants::testkit::source::crlf(&generated)
        } else {
            generated
        };
        let text = instrument(&source);
        syn::parse_file(&text).expect("the instrumented file parses");
        let (body, _runtime) = split_runtime(&text);
        proptest::prop_assert_eq!(
            count_lines(body.as_bytes()),
            count_lines(source.as_bytes()),
            "writing a guard onto an arm moved a line"
        );
    }

    /// However many functions a file holds and whichever way its lines end, instrumenting keeps the line count and leaves a file that parses.
    ///
    /// A rewrite that moved a line makes every position in the report a
    /// position in a file nobody has, and one that does not parse fails the
    /// build for a reason that is not the mutation.
    #[test]
    fn instrumenting_a_generated_file_keeps_its_lines_and_reparses(
        functions in 1usize..12,
        windows in proptest::bool::ANY
    ) {
        let source = if windows {
            rust_mutants::testkit::source::crlf(&generated(functions))
        } else {
            generated(functions)
        };
        let text = instrument(&source);
        syn::parse_file(&text).expect("the instrumented file parses");
        let (body, _runtime) = split_runtime(&text);
        proptest::prop_assert_eq!(
            count_lines(body.as_bytes()),
            count_lines(source.as_bytes()),
            "instrumenting moved a line"
        );
        proptest::prop_assert_eq!(
            body.contains("\r\n"),
            windows,
            "the rewrite kept the endings the file had"
        );
    }
}

#[test]
fn a_match_arm_whose_body_is_a_block_still_parses_after_the_guard_goes_in() {
    let text = instrument(
        "pub fn f(parsed: Result<u8, u8>) -> u8 {\n\
         \x20   match parsed {\n\
         \x20       Ok(one) => one + 1,\n\
         \x20       Ok(two) => {\n\
         \x20           let value = two + 1;\n\
         \x20           value\n\
         \x20       }\n\
         \x20       Err(other) => other,\n\
         \x20   }\n\
         }\n",
    );
    syn::parse_file(&text).unwrap_or_else(|error| {
        panic!(
            "an arm whose body is a block needs no comma after it, and one whose body is a \
             parenthesised expression does: {error}\n{text}"
        )
    });
    assert!(
        text.contains("Ok(two) => {if "),
        "so a block site keeps its braces rather than gaining parentheses: {text}"
    );
}

#[test]
fn every_source_of_this_repository_still_parses_once_it_is_instrumented() {
    let root = mjutest_devkit::paths::workspace_root();
    let mut checked = 0_u32;
    let mut stack = vec![root.join("crates"), root.join("xtask")];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                if entry.file_name() != "target" {
                    stack.push(path);
                }
                continue;
            }
            if path.extension().is_none_or(|extension| extension != "rs") {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            if syn::parse_file(&source).is_err() {
                continue;
            }
            let relative = path
                .strip_prefix(&root)
                .unwrap_or(&path)
                .display()
                .to_string();
            let Some((text, _)) = instrumented(&relative, &source) else {
                continue;
            };
            checked = checked.saturating_add(1);
            if let Err(error) = syn::parse_file(&text) {
                panic!("{relative} does not parse once instrumented: {error}");
            }
        }
    }
    assert!(
        checked > 100,
        "this is the widest set of real shapes the suite has, and it read {checked} files"
    );
}

/// One file instrumented as a run would instrument it, or nothing when discovery refuses it.
fn instrumented(path: &str, source: &str) -> Option<(String, Catalog)> {
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file(path, source.as_bytes(), &selection).ok()?;
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).ok()?;
    }
    let catalog = builder.build().ok()?;
    let placements = plan_file(&catalog, path, &discovery.candidates).ok()?;
    let file = instrument_file(&Instrumenting {
        path,
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        catalog_digest: catalog.digest(),
    })
    .ok()?;
    Some((file.text, catalog))
}
