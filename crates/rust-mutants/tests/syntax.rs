// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Syntactic discovery: every candidate a file yields, with its guard site,
//! and every place deliberately passed over, with its reason.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    clippy::too_many_lines,
    clippy::type_complexity,
    clippy::string_slice,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{
    FileDiscovery, Form, Selection, Skip, SkipReason, SyntaxError, discover_file,
};

fn registry() -> &'static Registry {
    static REGISTRY: Registry = Registry::canonical();
    &REGISTRY
}

fn discover(source: &str) -> FileDiscovery {
    let selection = Selection::tier(registry(), Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover")
}

/// `rule@line:byte_col "original"=>"replacement" FORM["site text"]`.
fn render(discovery: &FileDiscovery) -> Vec<String> {
    discovery
        .candidates
        .iter()
        .map(|found| {
            format!(
                "{}@{}:{} {:?}=>{:?} {}[{:?}]",
                found.candidate.rule.name,
                found.position.line,
                found.position.byte_column,
                String::from_utf8_lossy(&found.candidate.original),
                String::from_utf8_lossy(&found.candidate.replacement),
                found.hint.form,
                found.hint.site_text
            )
        })
        .collect()
}

fn skips(discovery: &FileDiscovery) -> Vec<(&'static str, u32)> {
    discovery
        .skips
        .iter()
        .map(|skip| (skip.reason.name(), skip.count))
        .collect()
}

fn assert_coherent(source: &str, discovery: &FileDiscovery) {
    for found in &discovery.candidates {
        found
            .candidate
            .validate()
            .expect("every candidate validates");
        let span = found.candidate.span;
        assert_eq!(
            &source.as_bytes()[span.start as usize..span.end as usize],
            found.candidate.original.as_slice(),
            "original bytes come from the span"
        );
        let site = found.hint.site;
        assert!(
            site.start <= span.start && span.end <= site.end,
            "the edit {span} lies inside its site {site}"
        );
        assert_eq!(
            &source.as_bytes()[site.start as usize..site.end as usize],
            found.hint.site_text.as_bytes()
        );
    }
    let mut starts: Vec<(u32, usize)> = discovery
        .candidates
        .iter()
        .map(|f| {
            (
                f.candidate.span.start,
                registry()
                    .position(f.candidate.rule.name)
                    .expect("registered"),
            )
        })
        .collect();
    let sorted = {
        let mut s = starts.clone();
        s.sort_unstable();
        s
    };
    assert_eq!(starts, sorted, "candidates are in (start, rule) order");
    starts.clear();
}

// --- forms ----------------------------------------------------------------------

#[test]
fn comparisons_in_conditions_take_form_c_and_nested_arithmetic_form_e() {
    let src = "fn f(a: i32, b: i32) -> bool {\n    if a + 1 < b {\n        return true;\n    }\n    a > b\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "negate-condition@2:8 \"a + 1 < b\"=>\"!(a + 1 < b)\" C[\"a + 1 < b\"]",
            "add-to-sub@2:10 \"+\"=>\"-\" E[\"a + 1\"]",
            "lt-to-le@2:14 \"<\"=>\"<=\" C[\"a + 1 < b\"]",
            "true-to-false@3:16 \"true\"=>\"false\" E[\"true\"]",
            "return-true@5:5 \"a > b\"=>\"true\" E[\"a > b\"]",
            "gt-to-ge@5:7 \">\"=>\">=\" E[\"a > b\"]",
        ]
    );
    assert!(d.skips.is_empty());
    assert!(!d.no_std);
}

#[test]
fn loops_connectives_and_negations_are_bool_positions_and_let_chains_are_left_alone() {
    let src = "fn g(x: bool, y: Option<i32>) {\n    while true {\n        if x && !x {\n            break;\n        }\n        if let Some(n) = y && n > 0 {\n            break;\n        }\n    }\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "true-to-false@2:11 \"true\"=>\"false\" C[\"true\"]",
            "negate-loop-condition@2:11 \"true\"=>\"!(true)\" C[\"true\"]",
            "negate-condition@3:12 \"x && !x\"=>\"!(x && !x)\" C[\"x && !x\"]",
            "and-to-or@3:14 \"&&\"=>\"||\" C[\"x && !x\"]",
            "remove-not@3:17 \"!x\"=>\"x\" C[\"!x\"]",
            "gt-to-ge@6:33 \">\"=>\">=\" C[\"n > 0\"]",
        ]
    );
}

#[test]
fn ranges_change_the_type_so_their_site_is_the_statement_or_the_initializer() {
    let src = "fn h(v: &[u8], n: usize) -> Vec<u8> {\n    for i in 0..n {\n        let s = &v[i..=n];\n        drop(s);\n    }\n    let w = v[0..n].to_vec();\n    v[..n].to_vec()\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "range-to-inclusive@2:15 \"..\"=>\"..=\" S[\"for i in 0..n {\\n        let s = &v[i..=n];\\n        drop(s);\\n    }\"]",
            "inclusive-to-range@3:21 \"..=\"=>\"..\" E[\"&v[i..=n]\"]",
            "delete-call-statement@4:9 \"drop(s);\"=>\"\" S[\"drop(s);\"]",
            "range-to-inclusive@6:16 \"..\"=>\"..=\" E[\"v[0..n].to_vec()\"]",
            "return-default@7:5 \"v[..n].to_vec()\"=>\"Default::default()\" E[\"v[..n].to_vec()\"]",
            "range-to-inclusive@7:7 \"..\"=>\"..=\" E[\"v[..n].to_vec()\"]",
        ]
    );
}

#[test]
fn return_replacements_follow_the_signature_and_never_spell_the_default_again() {
    let src = "fn r1(x: i32) -> Result<i32, String> {\n    if x < 0 {\n        return Err(String::new());\n    }\n    Ok(x)\n}\nfn r2(x: i32) -> Option<i32> {\n    Some(x)\n}\nfn r3() -> Option<i32> {\n    None\n}\nfn r4(x: i32) -> i32 {\n    let f = |y: i32| y + 1;\n    f(x)\n}\nfn r5() -> Result<(), String> {\n    Ok(())\n}\nfn r6() {\n    return;\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "negate-condition@2:8 \"x < 0\"=>\"!(x < 0)\" C[\"x < 0\"]",
            "lt-to-le@2:10 \"<\"=>\"<=\" C[\"x < 0\"]",
            "return-ok-default@3:16 \"Err(String::new())\"=>\"Ok(Default::default())\" E[\"Err(String::new())\"]",
            "return-ok-default@5:5 \"Ok(x)\"=>\"Ok(Default::default())\" E[\"Ok(x)\"]",
            "return-default@8:5 \"Some(x)\"=>\"Default::default()\" E[\"Some(x)\"]",
            "return-some-default@8:5 \"Some(x)\"=>\"Some(Default::default())\" E[\"Some(x)\"]",
            "return-some-default@11:5 \"None\"=>\"Some(Default::default())\" E[\"None\"]",
            "add-to-sub@14:24 \"+\"=>\"-\" E[\"y + 1\"]",
            "return-default@15:5 \"f(x)\"=>\"Default::default()\" E[\"f(x)\"]",
        ]
    );
}

#[test]
fn error_propagation_and_statement_deletion_take_the_statement_site() {
    let src = "fn e(v: &mut Vec<i32>) -> Result<(), String> {\n    let n = parse()?;\n    parse()?;\n    v.push(n);\n    v[0] += 2;\n    v[0] = v[0] * 2;\n    Ok(())\n}\nfn parse() -> Result<i32, String> { Ok(1) }\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "question-to-unwrap@2:20 \"?\"=>\".unwrap()\" E[\"parse()?\"]",
            "delete-call-statement@3:5 \"parse()?;\"=>\"\" S[\"parse()?;\"]",
            "question-to-unwrap@3:12 \"?\"=>\".unwrap()\" E[\"parse()?\"]",
            "ignore-question-statement@3:12 \"?\"=>\"\" S[\"parse()?;\"]",
            "delete-call-statement@4:5 \"v.push(n);\"=>\"\" S[\"v.push(n);\"]",
            "delete-compound-assignment@5:5 \"v[0] += 2;\"=>\"\" S[\"v[0] += 2;\"]",
            "add-assign-to-sub-assign@5:10 \"+=\"=>\"-=\" S[\"v[0] += 2;\"]",
            "delete-assignment@6:5 \"v[0] = v[0] * 2;\"=>\"\" S[\"v[0] = v[0] * 2;\"]",
            "mul-to-div@6:17 \"*\"=>\"/\" E[\"v[0] * 2\"]",
            "return-ok-default@9:37 \"Ok(1)\"=>\"Ok(Default::default())\" E[\"Ok(1)\"]",
        ]
    );
}

#[test]
fn bitwise_operators_and_match_guards() {
    let src = "fn b(a: u8, b: u8) -> u8 {\n    match a & b {\n        x if x > 1 => x | b,\n        _ => (a ^ b) << 1 >> 1,\n    }\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "return-default@2:5 \"match a & b {\\n        x if x > 1 => x | b,\\n        _ => (a ^ b) << 1 >> 1,\\n    }\"=>\"Default::default()\" E[\"match a & b {\\n        x if x > 1 => x | b,\\n        _ => (a ^ b) << 1 >> 1,\\n    }\"]",
            "band-to-bor@2:13 \"&\"=>\"|\" E[\"a & b\"]",
            "gt-to-ge@3:16 \">\"=>\">=\" C[\"x > 1\"]",
            "bor-to-band@3:25 \"|\"=>\"&\" E[\"x | b\"]",
            "xor-to-band@4:17 \"^\"=>\"&\" E[\"a ^ b\"]",
            "shl-to-shr@4:22 \"<<\"=>\">>\" E[\"(a ^ b) << 1\"]",
            "shr-to-shl@4:27 \">>\"=>\"<<\" E[\"(a ^ b) << 1 >> 1\"]",
        ]
    );
}

// --- skips ----------------------------------------------------------------------

#[test]
fn every_place_passed_over_is_counted_under_the_outermost_reason() {
    let src = "#![no_std]\nconst A: bool = true;\nstatic B: i32 = 1 + 2;\nconst fn c(x: i32) -> i32 { x + 1 }\n#[cfg(feature = \"extra\")]\nfn d(x: i32) -> i32 { x - 1 }\n#[cfg(test)]\nmod tests {\n    fn t(x: i32) -> bool { x > 0 }\n}\n#[test]\nfn u() { assert!(1 < 2); }\nfn m(x: i32) -> i32 {\n    println!(\"{}\", x + 1);\n    let a = [0u8; 2 + 2];\n    a.len() as i32 * x\n}\nenum E { X = 1 + 1 }\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert!(d.no_std);
    assert_eq!(
        render(&d),
        [
            "return-default@16:5 \"a.len() as i32 * x\"=>\"Default::default()\" E[\"a.len() as i32 * x\"]",
            "mul-to-div@16:20 \"*\"=>\"/\" E[\"a.len() as i32 * x\"]",
        ]
    );
    assert_eq!(
        skips(&d),
        [
            ("const-context", 6),
            ("macro-invocation", 1),
            ("cfg-attribute", 2),
            ("test-code", 3),
        ]
    );
    for skip in &d.skips {
        assert_eq!(skip.path, "src/lib.rs");
        assert!(!skip.reason.explanation().is_empty());
    }
}

#[test]
fn skip_reasons_are_named_explained_and_ranked() {
    let names: Vec<&str> = SkipReason::ALL.iter().map(|r| r.name()).collect();
    assert_eq!(
        names,
        [
            "const-context",
            "macro-invocation",
            "cfg-attribute",
            "test-code",
            "unsupported-site",
            "excluded",
            "test-only-file",
            "proc-macro-crate",
            "no-std-crate",
        ]
    );
    for reason in SkipReason::ALL {
        assert_eq!(SkipReason::parse(reason.name()), Some(reason));
        assert!(reason.explanation().len() > 20, "{reason:?}");
    }
    assert_eq!(SkipReason::parse("nope"), None);
    let mut unsorted = [
        Skip {
            path: "b.rs".to_owned(),
            reason: SkipReason::TestCode,
            count: 1,
        },
        Skip {
            path: "a.rs".to_owned(),
            reason: SkipReason::TestCode,
            count: 1,
        },
        Skip {
            path: "z.rs".to_owned(),
            reason: SkipReason::ConstContext,
            count: 1,
        },
    ];
    unsorted.sort();
    let order: Vec<(&str, &str)> = unsorted
        .iter()
        .map(|s| (s.reason.name(), s.path.as_str()))
        .collect();
    assert_eq!(
        order,
        [
            ("const-context", "z.rs"),
            ("test-code", "a.rs"),
            ("test-code", "b.rs")
        ]
    );
}

// --- positions and sites --------------------------------------------------------------

#[test]
fn positions_count_bytes_and_chars_and_spans_are_absolute_past_a_bom_and_shebang() {
    let src = "\u{feff}#!/usr/bin/env cargo\r\n// comment\r\nmod inner {\r\n    /// doc\r\n    #[inline]\r\n    pub fn f(a: i32) -> i32 { let s = \"日本\"; a + s.len() as i32 }\r\n}\r\n";
    let d = discover(src);
    assert_coherent(src, &d);
    let add = d
        .candidates
        .iter()
        .find(|f| f.candidate.rule.name == "add-to-sub")
        .expect("add-to-sub");
    assert_eq!(add.position.line, 6);
    assert_eq!(add.position.byte_column, 51);
    assert_eq!(add.position.char_column, 47);
    assert_eq!(add.hint.super_depth, 1);
    let allow_at = add.hint.allow_at.expect("inside a fn") as usize;
    assert!(
        src[allow_at..].starts_with("/// doc"),
        "{:?}",
        &src[allow_at..allow_at + 12]
    );
    let ret = d
        .candidates
        .iter()
        .find(|f| f.candidate.rule.name == "return-default")
        .expect("return-default");
    assert_eq!(
        (
            ret.position.line,
            ret.position.byte_column,
            ret.position.char_column
        ),
        (6, 49, 45)
    );
}

#[test]
fn the_allow_attribute_goes_on_the_innermost_fn_and_top_level_code_has_depth_zero() {
    let src = "struct S;\nimpl S {\n    fn m(&self, x: i32) -> i32 {\n        fn inner(y: i32) -> i32 { y * 2 }\n        inner(x) + 1\n    }\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    let at = |rule: &str| {
        let f = d
            .candidates
            .iter()
            .find(|f| f.candidate.rule.name == rule)
            .expect(rule);
        (
            f.hint.super_depth,
            f.hint.allow_at.map(|o| &src[o as usize..o as usize + 8]),
        )
    };
    assert_eq!(at("mul-to-div"), (0, Some("fn inner")));
    assert_eq!(at("add-to-sub"), (0, Some("fn m(&se")));
    let returns: Vec<(u32, &str)> = d
        .candidates
        .iter()
        .filter(|f| f.candidate.rule.name == "return-default")
        .map(|f| (f.position.line, f.hint.site_text.as_str()))
        .collect();
    assert_eq!(returns, [(4, "y * 2"), (5, "inner(x) + 1")]);
}

#[test]
fn closures_and_async_blocks_have_no_known_return_type_unless_spelled() {
    let src = "fn c() -> i32 {\n    let f = |a: i32| -> i32 { a * 3 };\n    let g = |a: i32| a * 3;\n    let h = async { 1 + 1 };\n    drop(h);\n    f(1) + g(1)\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    let returns: Vec<(u32, &str)> = d
        .candidates
        .iter()
        .filter(|f| f.candidate.rule.family == rust_mutants::rule::Family::ReturnReplacement)
        .map(|f| (f.position.line, f.hint.site_text.as_str()))
        .collect();
    assert_eq!(returns, [(2, "a * 3"), (6, "f(1) + g(1)")]);
}

// --- errors, determinism, selection ----------------------------------------------------

#[test]
fn a_file_that_does_not_parse_is_an_error_naming_the_line() {
    let selection = Selection::tier(registry(), Tier::All);
    let error = discover_file("src/bad.rs", b"fn f( {", &selection).unwrap_err();
    match &error {
        SyntaxError::Parse { path, line, .. } => {
            assert_eq!(path, "src/bad.rs");
            assert_eq!(*line, 1);
        }
        other => panic!("{other:?}"),
    }
    let error = discover_file("src/bin.rs", &[0xff, 0xfe], &selection).unwrap_err();
    assert!(matches!(error, SyntaxError::NotUtf8 { .. }), "{error:?}");
}

#[test]
fn the_selection_limits_the_rules_and_discovery_is_deterministic() {
    let src = "fn f(a: i32) -> bool {\n    a + 1 > 2\n}\n";
    let balanced = Selection::tier(registry(), Tier::Balanced);
    let d = discover_file("src/lib.rs", src.as_bytes(), &balanced).expect("discover");
    let again = discover_file("src/lib.rs", src.as_bytes(), &balanced).expect("again");
    assert_eq!(d, again);
    let only = Selection::rules(registry(), &["gt-to-ge"]).expect("known rule");
    let d = discover_file("src/lib.rs", src.as_bytes(), &only).expect("discover");
    assert_eq!(render(&d), ["gt-to-ge@2:11 \">\"=>\">=\" E[\"a + 1 > 2\"]"]);
    let unknown = Selection::rules(registry(), &["no-such-rule"]).unwrap_err();
    assert!(
        matches!(unknown, rust_mutants::rule::RuleError::UnknownRule { .. }),
        "{unknown:?}"
    );
    assert_eq!(d.source_digest.len(), 64);
    assert_eq!(d.path, "src/lib.rs");
}

#[test]
fn the_trace_record_lists_every_decision_in_source_order() {
    let src = "fn f(a: i32) -> i32 {\n    println!(\"{a}\");\n    a + 1\n}\n";
    let d = discover(src);
    let record = d.trace_record();
    assert_eq!(record.path, "src/lib.rs");
    assert_eq!(record.candidates, 2);
    let sites: Vec<(u32, u32, &str, Option<&str>, Option<&str>)> = record
        .sites
        .iter()
        .map(|s| {
            (
                s.line,
                s.column,
                s.rule.as_str(),
                s.form.as_deref(),
                s.skip.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        sites,
        [
            (2, 5, "macro-invocation", None, Some("macro-invocation")),
            (3, 5, "return-default", Some("E"), None),
            (3, 7, "add-to-sub", Some("E"), None),
        ]
    );
    assert_eq!(record.skips.len(), 1);
    assert_eq!(record.skips[0].reason, "macro-invocation");
    assert_eq!(record.skips[0].count, 1);
    let json = serde_json::to_string(&record).expect("json");
    assert!(json.contains("\"candidates\":2"), "{json}");
}

// --- the golden ----------------------------------------------------------------------------

#[test]
fn the_families_input_exercises_every_rule_and_matches_the_golden() {
    let root =
        mjutest_devkit::paths::workspace_root().join("crates/rust-mutants/tests/testdata/syntax");
    let src = std::fs::read_to_string(root.join("families.input")).expect("input");
    let d = discover(&src);
    assert_coherent(&src, &d);
    let mut seen: Vec<&str> = d.candidates.iter().map(|f| f.candidate.rule.name).collect();
    seen.sort_unstable();
    seen.dedup();
    let all: Vec<&str> = registry().rules().iter().map(|r| r.name).collect();
    let missing: Vec<&&str> = all.iter().filter(|name| !seen.contains(name)).collect();
    assert!(
        missing.is_empty(),
        "rules without a candidate in families.input: {missing:?}"
    );
    let mut lines = render(&d);
    for skip in &d.skips {
        lines.push(format!("skip {} {}", skip.reason.name(), skip.count));
    }
    let mut text = lines.join("\n");
    text.push('\n');
    mjutest_devkit::golden::golden(&root.join("families.golden"), text.as_bytes()).expect("golden");
}

#[test]
fn forms_display_as_their_letters() {
    assert_eq!(Form::C.to_string(), "C");
    assert_eq!(Form::E.to_string(), "E");
    assert_eq!(Form::S.to_string(), "S");
}
