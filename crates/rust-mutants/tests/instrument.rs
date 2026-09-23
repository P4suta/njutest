// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Instrumentation: every compilable mutant of a file lives in the file at once, dormant behind a guard, and the file keeps its line numbering.

#![expect(
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use rust_mutants::catalog::{Builder, Catalog};
use rust_mutants::instrument::{
    ACTIVE_ENV, CATALOG_ENV, COMPILED_CATALOG_ENV, InstrumentErrorKind, Instrumenting, MODULE_STEM,
    RUNTIME_MARKER, STALE_CATALOG_EXIT, STEP_PROTOCOL_EXIT, STEP_STATE_ENV, STEP_STATE_SCHEMA,
    instrument_file, module_name, plan_file,
};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::splice::count_lines;
use rust_mutants::syntax::{Selection, discover_file};

static REGISTRY: Registry = Registry::canonical();

/// Discovers, catalogs, and instruments one source, returning the text with the file's own runtime module named `__rm`.
fn instrument(source: &str) -> String {
    let (text, _) = instrument_with_catalog(source);
    text.replace(
        &module_name("src/lib.rs", source).expect("valid source tokens"),
        MODULE_STEM,
    )
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
        probed: &BTreeMap::default(),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    (file.text, catalog)
}

/// Every mutant the syntax offers a comparison for, as a run has it once the compiler has vouched for the operands.
fn offered(discovery: &rust_mutants::syntax::FileDiscovery, catalog: &Catalog) -> BTreeSet<u32> {
    discovery
        .candidates
        .iter()
        .filter(|found| found.comparable.is_some())
        .map(|found| {
            found
                .candidate
                .id()
                .expect("a discovered candidate has an identity")
        })
        .filter_map(|id| catalog.by_id(id.as_str()))
        .map(|mutant| mutant.index)
        .collect()
}

fn golden_path(name: &str) -> PathBuf {
    njutest_devkit::paths::workspace_root()
        .join("crates/rust-mutants/tests/testdata/instrument")
        .join(name)
}

/// Instruments `<name>.input` and compares the result with `<name>.golden`.
fn golden_case(name: &str) {
    let input = std::fs::read(golden_path(&format!("{name}.input"))).expect("input");
    let source = String::from_utf8(input).expect("utf-8");
    let text = instrument(&source);
    syn::parse_file(&text).expect("the instrumented file parses");
    let (body, runtime) = split_runtime(&text);
    assert!(runtime.contains(RUNTIME_MARKER));
    assert_eq!(
        count_lines(body.as_bytes()),
        count_lines(source.as_bytes()),
        "{name}: the body kept its line count"
    );
    njutest_devkit::golden::golden(&golden_path(&format!("{name}.golden")), text.as_bytes())
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
    assert_eq!(COMPILED_CATALOG_ENV, "RUST_MUTANTS_COMPILED_CATALOG");
    assert_eq!(MODULE_STEM, "__rm");
    assert_eq!(STALE_CATALOG_EXIT, 97);
    assert_eq!(STEP_PROTOCOL_EXIT, 94);
    assert_eq!(STEP_STATE_ENV, "RUST_MUTANTS_STEP_STATE");
    assert_eq!(STEP_STATE_SCHEMA, "rust-mutants-step-state-v1");
    assert_eq!(RUNTIME_MARKER, "rust-mutants-runtime-v1");
}

#[test]
fn a_boolean_position_takes_the_selector_form_and_a_value_position_the_expression_form() {
    let text = instrument(
        "pub fn f(a: i32, b: i32, c: i32) -> bool {\n    if a > b {\n        return true;\n    }\n    a + c > b\n}\n",
    );
    assert!(
        text.contains("if __rm::value!(__rm::active(0) && __rm::value!(!(a > b)) || __rm::active(1) && __rm::value!(true) || __rm::active(2) && __rm::value!(false) || __rm::active(3) && __rm::value!(a >= b) || !__rm::active(0) && !__rm::active(1) && !__rm::active(2) && !__rm::active(3) && __rm::differing(3, __rm::value!(a > b), || __rm::value!(a >= b))) {"),
        "{text}"
    );
    assert!(
        text.contains("return __rm::value!(if __rm::active(4) { false } else { true });"),
        "{text}"
    );
    assert!(
        text.contains(
            "__rm::value!(if __rm::active(5) { true } else if __rm::active(7) { a + c >= b } else {"
        ),
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
        probed: &BTreeMap::default(),
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
        probed: &BTreeMap::default(),
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
        probed: &BTreeMap::default(),
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
        probed: &BTreeMap::default(),
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
    assert!(
        !text.contains("macro_rules! value"),
        "a statement-only file emits no unused expression macro: {text}"
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
        line.contains("else { __rm::value!(if __rm::active("),
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
    assert!(body.starts_with("pub fn f(a: i32) -> i32 {"), "{body}");
    assert!(runtime.contains(RUNTIME_MARKER), "{runtime}");
    assert!(
        runtime.contains(&format!("{:?}", catalog.digest())),
        "{runtime}"
    );
    for mutant in catalog.mutants() {
        assert!(
            runtime.contains(&format!("({:?}, {})", mutant.id.as_str(), mutant.index)),
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
fn the_step_runtime_uses_one_locked_process_state_and_fails_closed_at_every_boundary() {
    let text = instrument("pub fn f(a: i32) -> i32 { a + 1 }\n");
    let (_, runtime) = split_runtime(&text);

    for required in [
        "enum Budget",
        "enum StepNoticeError",
        "StepPhase {",
        "enum StepStateError",
        "Budget::Invalid(_) => protocol_failure()",
        "pub(crate) fn checkpoint()",
        "file.lock().map_err",
        "file.unlock().map_err",
        "RUST_MUTANTS_STEP_STATE",
        ".create_new(true)",
        "Write::write_all",
        "file.sync_data().map_err",
        "fs::rename(partial, path).map_err",
        "process::exit(94)",
    ] {
        assert!(
            runtime.contains(required),
            "missing {required:?}: {runtime}"
        );
    }
    assert!(
        !runtime.contains("unwrap_or(STEPS_UNBOUNDED)"),
        "a malformed bound must not become unbounded: {runtime}"
    );
    for prohibited in [
        "Result<(), ()>",
        "Ordering::Relaxed",
        "wrapping_sub",
        "poisoned.into_inner()",
        "let _ = SEEN.try_with",
    ] {
        assert!(
            !runtime.contains(prohibited),
            "generated code must not regain {prohibited:?}: {runtime}"
        );
    }
}

#[test]
fn a_file_without_a_mutant_still_carries_the_process_wide_checkpoint_runtime() {
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
        probed: &BTreeMap::default(),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    assert!(file.text.starts_with(source), "{}", file.text);
    assert!(file.text.contains(RUNTIME_MARKER), "{}", file.text);
    assert!(
        !file.text.contains("macro_rules! value"),
        "a candidate-free file emits no unused expression macro: {}",
        file.text
    );
    assert!(file.guards.is_empty());
    assert!(file.instrumented);
    assert!(!file.module.is_empty());
}

#[test]
fn a_checkpoint_inside_a_mutant_edit_stays_in_the_original_branch() {
    let source = include_str!("../../../fixtures/fixture-modern/src/lib.rs");
    let text = instrument(source);
    syn::parse_file(&text).expect("the checkpointed mutant file parses");
    assert!(
        text.contains("filter(|one| { __rm::checkpoint(); within(**one, bound) })"),
        "the expression closure remains bounded in the original branch: {text}"
    );
    assert!(
        text.contains("map(|one| { __rm::checkpoint();"),
        "every expression closure enclosed by the mutation stays bounded: {text}"
    );
}

#[test]
fn only_the_private_generated_module_carries_the_exact_lint_exception() {
    let text = instrument(
        "pub fn f(a: i32, b: i32) -> i32 {\n    a + b\n}\n\npub fn g(a: i32) -> i32 {\n    fn inner(x: i32) -> i32 { x * 2 }\n    inner(a) - 1\n}\n",
    );
    assert_eq!(
        text.matches("#[allow(").count(),
        1,
        "user functions never inherit a generated-code exception: {text}"
    );
    let parsed = syn::parse_file(&text).expect("instrumented source parses");
    let module = parsed
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Mod(module) if module.ident == "__rm" => Some(module),
            _ => None,
        })
        .expect("the generated support module");
    assert!(
        matches!(module.vis, syn::Visibility::Inherited),
        "the lint exception is confined to a private module: {:?}",
        module.vis
    );
    let attributes: Vec<&syn::Attribute> = module
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("allow"))
        .collect();
    assert_eq!(attributes.len(), 1, "{text}");
    let names = attributes
        .first()
        .expect("the one generated allow attribute")
        .parse_args_with(syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated)
        .expect("the generated allow is a literal lint list")
        .iter()
        .map(|path| {
            path.segments
                .iter()
                .map(|segment| segment.ident.to_string())
                .collect::<Vec<_>>()
                .join("::")
        })
        .collect::<Vec<_>>();
    assert_eq!(names, ["dead_code", "unused_qualifications"]);
    assert!(!text.contains("allow(warnings"), "{text}");
}

#[test]
fn a_file_that_already_spells_the_module_name_gets_the_next_one() {
    let taken = module_name("src/lib.rs", "").expect("valid source tokens");
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
    let one = module_name("src/lib.rs", "").expect("valid source tokens");
    let other = module_name("src/items.rs", "").expect("valid source tokens");

    assert!(one.starts_with(&format!("{MODULE_STEM}_")), "{one}");
    assert_ne!(
        one, other,
        "`include!` at item position pastes one file's items into another's module, and two \
         modules of the same name in one scope are the same item defined twice: every mutant \
         of both files would then come back refused by an error that names neither"
    );
    assert_eq!(
        one,
        module_name("src/lib.rs", "").expect("valid source tokens"),
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
    let (body, runtime) = split_runtime(&text);
    assert!(runtime.contains(RUNTIME_MARKER));
    assert_eq!(count_lines(body.as_bytes()), count_lines(source.as_bytes()));
    assert!(
        body.contains("(!(a < b && b > 0))"),
        "an alternative is folded onto one line: {body}"
    );
    assert!(
        body.contains(")\n        && __rm::value!("),
        "the original branch is where the line break stayed: {body}"
    );
}

#[test]
fn a_placement_naming_an_unknown_mutant_is_refused() {
    let source = "pub fn f(a: i32) -> i32 { a + 1 }\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let catalog = Builder::new().build().expect("empty catalog");
    let error = plan_file(&catalog, "src/lib.rs", &discovery.candidates)
        .expect_err("a placement cannot name a mutant outside its catalog");
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
        probed: &BTreeMap::default(),
        catalog_digest: catalog.digest(),
    })
    .expect_err("the source must be the one the candidates came from");
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
        let (body, runtime) = split_runtime(&text);
        assert!(runtime.contains(RUNTIME_MARKER));
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
        probed: &BTreeMap::default(),
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
fn generated_guards_do_not_change_the_user_functions_lint_policy() {
    let text = instrument("#![deny(warnings)]\npub fn positive(n: u32) -> bool { n > 0 }\n");
    let (body, runtime) = split_runtime(&text);
    assert!(!body.contains("#[allow"), "{body}");
    assert!(!runtime.contains("allow(warnings"), "{runtime}");
    assert!(
        runtime.contains(rust_mutants::instrument::GENERATED_MODULE_ALLOW_ATTRIBUTE),
        "{runtime}"
    );
}

#[test]
fn the_generated_allowance_names_the_lints_a_forbid_of_which_is_a_conflict() {
    let allow = rust_mutants::instrument::GENERATED_MODULE_ALLOW_ATTRIBUTE;
    let named: Vec<&str> = allow
        .trim_start_matches("#[allow(")
        .trim_end_matches(")]")
        .split(", ")
        .collect();
    assert_eq!(
        named,
        rust_mutants::instrument::GENERATED_MODULE_ALLOWED_LINTS,
        "the attribute goes into somebody else's tree and the conflicting-lints list decides \
         whether their `forbid` is refused before it does, so a lint in one and not the other \
         is a build this engine breaks and says nothing about: {allow}"
    );
}

/// A file of `functions` generated functions, each with a comparison, a branch, and a tail.
fn generated(functions: usize) -> String {
    use std::fmt::Write as _;
    let mut text = String::from("//! A generated module.\n\n");
    for index in 0..functions {
        let written = writeln!(
            text,
            "pub fn f{index}(a: i32, b: i32) -> i32 {{\n    \
             let mut total = 0;\n    \
             if a > b {{\n        total += a - b;\n    }} else {{\n        total += b - a;\n    }}\n    \
             total + a * b\n}}\n"
        );
        written.expect("writing into a String cannot fail");
    }
    text
}

/// A match of `arms` arms, some guarded, ending in a bare wildcard.
fn generated_match(arms: usize, guarded: bool) -> String {
    use std::fmt::Write as _;
    let mut text =
        String::from("//! A generated match.\n\npub fn pick(n: i32) -> i32 {\n    match n {\n");
    for index in 0..arms {
        let written = if guarded && index % 2 == 1 {
            let distant = index.checked_add(1000).expect("the generated arm fits");
            writeln!(
                text,
                "        {index}\n        | {distant} if n > {index} => {index},"
            )
        } else {
            writeln!(text, "        {index} => {index},")
        };
        written.expect("writing into a String cannot fail");
    }
    text.push_str("        _ => -1,\n    }\n}\n");
    text
}

proptest::proptest! {
    /// However many arms a match holds and whichever way its lines end, writing a guard onto one keeps the line count and leaves a file that parses.
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
        let (body, runtime) = split_runtime(&text);
        proptest::prop_assert!(runtime.contains(RUNTIME_MARKER));
        proptest::prop_assert_eq!(
            count_lines(body.as_bytes()),
            count_lines(source.as_bytes()),
            "writing a guard onto an arm moved a line"
        );
    }

    /// However many functions a file holds and whichever way its lines end, instrumenting keeps the line count and leaves a file that parses.
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
        let (body, runtime) = split_runtime(&text);
        proptest::prop_assert!(runtime.contains(RUNTIME_MARKER));
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
    let parsed = syn::parse_file(&text);
    assert!(
        parsed.is_ok(),
        "an arm whose body is a block needs no comma after it, and one whose body is a \
         parenthesised expression does: {parsed:?}\n{text}"
    );
    assert!(
        text.contains("Ok(two) => __rm::value!(if "),
        "so the macro groups the block site without a coercing call or lint-producing +         parentheses: {text}"
    );
}

#[test]
fn every_source_of_this_repository_still_parses_once_it_is_instrumented() {
    let root = njutest_devkit::paths::workspace_root();
    let mut checked = 0_u32;
    let mut stack = vec![root.join("crates"), root.join("xtask")];
    while let Some(directory) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries {
            let entry = entry.expect("fixture directory entry");
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
                .expect("every walked source stays below the repository root")
                .to_str()
                .expect("repository source paths are exact UTF-8")
                .replace('\\', "/");
            let Some((text, _)) = instrumented(&relative, &source) else {
                continue;
            };
            checked = checked
                .checked_add(1)
                .expect("the repository has fewer than u32::MAX files");
            let parsed = syn::parse_file(&text);
            assert!(
                parsed.is_ok(),
                "{relative} does not parse once instrumented: {parsed:?}"
            );
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
    let discovery = match discover_file(path, source.as_bytes(), &selection) {
        Ok(discovery) => discovery,
        Err(_) => return None,
    };
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        if builder.add(found.candidate.clone()).is_err() {
            return None;
        }
    }
    let catalog = match builder.build() {
        Ok(catalog) => catalog,
        Err(_) => return None,
    };
    let placements = match plan_file(&catalog, path, &discovery.candidates) {
        Ok(placements) => placements,
        Err(_) => return None,
    };
    let file = match instrument_file(&Instrumenting {
        path,
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &BTreeMap::default(),
        catalog_digest: catalog.digest(),
    }) {
        Ok(file) => file,
        Err(_) => return None,
    };
    Some((file.text, catalog))
}

/// Every return replacement the syntax offers a probe for, as a run has it once the compiler has vouched for the type.
fn probeable(
    discovery: &rust_mutants::syntax::FileDiscovery,
    catalog: &Catalog,
) -> BTreeMap<u32, rust_mutants::probe::Question> {
    discovery
        .candidates
        .iter()
        .filter_map(|found| {
            found.probe.map(|question| {
                (
                    found
                        .candidate
                        .id()
                        .expect("a discovered candidate has an identity"),
                    question,
                )
            })
        })
        .filter_map(|(id, question)| Some((catalog.by_id(id.as_str())?.index, question)))
        .collect()
}

/// One source instrumented with every probe the syntax offers written in.
fn instrument_probing(source: &str) -> String {
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
        probed: &probeable(&discovery, &catalog),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    assert!(
        !file.compared.is_empty(),
        "the tree reports what it wrote, and this source has a probe in it: {}",
        file.text
    );
    file.text.replace(
        &module_name("src/lib.rs", source).expect("valid source tokens"),
        MODULE_STEM,
    )
}

#[test]
fn a_probed_return_asks_what_the_value_already_held_on_the_branch_that_keeps_it() {
    let source = "pub fn f(a: i32) -> i32 {\n    return a;\n}\n";
    let text = instrument_probing(source);
    assert!(
        text.contains("else { __rm::undefaulted(0, a) }"),
        "the call takes the original branch and answers with it: {text}"
    );
}

#[test]
fn a_probed_bool_return_asks_whether_it_was_already_true() {
    let source = "pub fn f(a: bool) -> bool {\n    return a;\n}\n";
    let text = instrument_probing(source);
    assert!(
        text.contains("__rm::untrue("),
        "a bool return's replacement writes `true`, so that is what it is compared against: {text}"
    );
}

#[test]
fn probing_a_return_keeps_the_line_the_catalog_reported() {
    let source = "\
pub fn f(a: i32) -> i32 {
    return a;
}

pub fn g(b: bool) -> bool {
    return b;
}
";
    let text = instrument_probing(source);
    let body = text.split("#[doc(hidden)]").next().unwrap_or_default();
    assert_eq!(
        body.lines().count(),
        source.lines().count(),
        "no guard may move a line: {body}"
    );
}

#[test]
fn a_tree_holds_the_call_for_a_probe_only_where_the_compiler_vouched_for_one() {
    let source = "pub fn f(a: i32) -> i32 {\n    return a;\n}\n";
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
        probed: &BTreeMap::default(),
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    assert!(
        file.compared.is_empty(),
        "nothing was vouched for, so nothing is compared: {:?}",
        file.compared
    );
    let body = file
        .text
        .split("#[doc(hidden)]")
        .next()
        .expect("split always yields its prefix");
    assert!(
        !body.contains("undefaulted"),
        "and no guard of the tree calls it, however the runtime defines it: {body}"
    );
}

#[test]
fn a_form_that_cannot_hold_the_call_writes_no_probe_however_many_it_was_offered() {
    let source = "pub fn g() {}\npub fn f() {\n    g();\n}\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let statements: BTreeMap<u32, rust_mutants::probe::Question> = placements
        .iter()
        .filter(|placement| placement.hint.form == rust_mutants::syntax::Form::S)
        .map(|placement| (placement.index, rust_mutants::probe::Question::Default))
        .collect();
    assert!(
        !statements.is_empty(),
        "deleting a call is a statement site, which is the form this is about"
    );

    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &BTreeSet::default(),
        probed: &statements,
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    assert!(
        file.compared.is_empty(),
        "the call answers with the value it was given, so a statement is nowhere to put it, and \
         the tree reports what it wrote rather than what it was offered: {:?}",
        file.compared
    );
    let body = file.text.split("#[doc(hidden)]").next().unwrap_or_default();
    assert!(
        !body.contains("undefaulted"),
        "and no guard of the tree calls it: {body}"
    );
}

/// The site as the guard writes it when this mutant is the live one: its own bytes with the edit made.
fn edited(placement: &rust_mutants::instrument::Placement) -> String {
    let at = |offset: u32| usize::try_from(offset).expect("a u32 offset fits this test platform");
    let start = at(placement
        .edit
        .start
        .checked_sub(placement.hint.site.start)
        .expect("the edit starts inside the site"));
    let end = at(placement
        .edit
        .end
        .checked_sub(placement.hint.site.start)
        .expect("the edit ends inside the site"));
    let mut text = placement.hint.site_text.clone().into_bytes();
    text.splice(start..end, placement.replacement.iter().copied());
    String::from_utf8(text).expect("instrumenting exact UTF-8 source keeps exact UTF-8")
}

#[test]
fn a_probe_around_the_original_leaves_every_nested_branch_where_it_says_it_is() {
    let source = "pub fn f(a: i32, b: i32, c: bool) -> bool {\n    return a < b && c;\n}\n";
    let selection = Selection::tier(&REGISTRY, Tier::All);
    let discovery = discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover");
    let mut builder = Builder::new();
    for found in &discovery.candidates {
        builder.add(found.candidate.clone()).expect("add");
    }
    let catalog = builder.build().expect("catalog");
    let placements = plan_file(&catalog, "src/lib.rs", &discovery.candidates).expect("plan");
    let probes = probeable(&discovery, &catalog);
    assert!(
        !probes.is_empty(),
        "`return a < b` on a bool is a return replacement the syntax offers a probe for"
    );
    assert!(
        placements.len() > probes.len(),
        "and the comparison inside it is a site of its own, nested in the probed branch"
    );

    let file = instrument_file(&Instrumenting {
        path: "src/lib.rs",
        source: source.as_bytes(),
        placements: &placements,
        markers: &[],
        comparable: &offered(&discovery, &catalog),
        probed: &probes,
        catalog_digest: catalog.digest(),
    })
    .expect("instrument");
    let kept = file
        .text
        .find(" else { ")
        .expect("a value-position guard keeps the original in an else");
    assert!(
        file.branches.iter().any(|branch| {
            usize::try_from(branch.span.start).expect("a u32 offset fits this test platform") > kept
        }),
        "a branch inside the probed original is what this is about: {}",
        file.text
    );
    for branch in &file.branches {
        let at =
            |offset: u32| usize::try_from(offset).expect("a u32 offset fits this test platform");
        let held = file
            .text
            .get(at(branch.span.start)..at(branch.span.end))
            .expect("a branch names bytes of the text it is about");
        let placement = placements
            .iter()
            .find(|placement| placement.index == branch.index)
            .expect("a branch is about a placed mutant");
        assert_eq!(
            held,
            edited(placement),
            "the span a branch reports is where a compiler's error about that mutant lands, and \
             a probe written around the original must not move it: {branch:?} in {}",
            file.text
        );
    }
}
