// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Syntactic discovery: every candidate a file yields, with its guard site, and every place deliberately passed over, with its reason.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::as_conversions,
    clippy::type_complexity,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{
    FileDiscovery, Form, LineIndex, PositionError, Selection, Skip, SkipReason, SyntaxError,
    discover_file,
};

fn registry() -> &'static Registry {
    static REGISTRY: Registry = Registry::canonical();
    &REGISTRY
}

fn discover(source: &str) -> FileDiscovery {
    let selection = Selection::tier(registry(), Tier::All);
    discover_file("src/lib.rs", source.as_bytes(), &selection).expect("discover")
}

/// Discovery with every rule of the registry named, the ones no tier chooses included.
fn discover_every_rule(source: &str) -> FileDiscovery {
    let names: Vec<&str> = registry().rules().iter().map(|rule| rule.name).collect();
    let selection = Selection::rules(registry(), &names).expect("every registered rule");
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
                std::str::from_utf8(&found.candidate.original)
                    .expect("the Rust source fixture is exact UTF-8"),
                std::str::from_utf8(&found.candidate.replacement)
                    .expect("the generated Rust replacement is exact UTF-8"),
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

#[test]
fn comparisons_in_conditions_take_form_c_and_nested_arithmetic_form_e() {
    let src = "fn f(a: i32, b: i32) -> bool {\n    if a + 1 < b {\n        return true;\n    }\n    a > b\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        render(&d),
        [
            "negate-condition@2:8 \"a + 1 < b\"=>\"!(a + 1 < b)\" C[\"a + 1 < b\"]",
            "condition-to-true@2:8 \"a + 1 < b\"=>\"true\" C[\"a + 1 < b\"]",
            "condition-to-false@2:8 \"a + 1 < b\"=>\"false\" C[\"a + 1 < b\"]",
            "add-to-sub@2:10 \"+\"=>\"-\" E[\"a + 1\"]",
            "int-increment@2:12 \"1\"=>\"2\" E[\"1\"]",
            "int-decrement@2:12 \"1\"=>\"0\" E[\"1\"]",
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
            "condition-to-true@3:12 \"x && !x\"=>\"true\" C[\"x && !x\"]",
            "condition-to-false@3:12 \"x && !x\"=>\"false\" C[\"x && !x\"]",
            "and-to-or@3:14 \"&&\"=>\"||\" C[\"x && !x\"]",
            "remove-not@3:17 \"!x\"=>\"x\" C[\"!x\"]",
            "break-to-continue@4:13 \"break\"=>\"continue\" E[\"break\"]",
            "gt-to-ge@6:33 \">\"=>\">=\" C[\"n > 0\"]",
            "int-increment@6:35 \"0\"=>\"1\" E[\"0\"]",
            "break-to-continue@7:13 \"break\"=>\"continue\" E[\"break\"]",
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
            "int-increment@2:14 \"0\"=>\"1\" E[\"0\"]",
            "range-to-inclusive@2:15 \"..\"=>\"..=\" S[\"for i in 0..n {\\n        let s = &v[i..=n];\\n        drop(s);\\n    }\"]",
            "inclusive-to-range@3:21 \"..=\"=>\"..\" E[\"&v[i..=n]\"]",
            "delete-call-statement@4:9 \"drop(s);\"=>\"\" S[\"drop(s);\"]",
            "int-increment@6:15 \"0\"=>\"1\" E[\"0\"]",
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
            "condition-to-true@2:8 \"x < 0\"=>\"true\" C[\"x < 0\"]",
            "condition-to-false@2:8 \"x < 0\"=>\"false\" C[\"x < 0\"]",
            "lt-to-le@2:10 \"<\"=>\"<=\" C[\"x < 0\"]",
            "int-increment@2:12 \"0\"=>\"1\" E[\"0\"]",
            "return-ok-default@3:16 \"Err(String::new())\"=>\"Ok(Default::default())\" E[\"Err(String::new())\"]",
            "return-ok-default@5:5 \"Ok(x)\"=>\"Ok(Default::default())\" E[\"Ok(x)\"]",
            "return-err-default@5:5 \"Ok(x)\"=>\"Err(Default::default())\" E[\"Ok(x)\"]",
            "return-default@8:5 \"Some(x)\"=>\"Default::default()\" E[\"Some(x)\"]",
            "return-some-default@8:5 \"Some(x)\"=>\"Some(Default::default())\" E[\"Some(x)\"]",
            "return-some-default@11:5 \"None\"=>\"Some(Default::default())\" E[\"None\"]",
            "add-to-sub@14:24 \"+\"=>\"-\" E[\"y + 1\"]",
            "int-increment@14:26 \"1\"=>\"2\" E[\"1\"]",
            "int-decrement@14:26 \"1\"=>\"0\" E[\"1\"]",
            "return-default@15:5 \"f(x)\"=>\"Default::default()\" E[\"f(x)\"]",
            "return-err-default@18:5 \"Ok(())\"=>\"Err(Default::default())\" E[\"Ok(())\"]",
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
            "int-increment@5:7 \"0\"=>\"1\" E[\"0\"]",
            "add-assign-to-sub-assign@5:10 \"+=\"=>\"-=\" S[\"v[0] += 2;\"]",
            "int-increment@5:13 \"2\"=>\"3\" E[\"2\"]",
            "int-decrement@5:13 \"2\"=>\"1\" E[\"2\"]",
            "delete-assignment@6:5 \"v[0] = v[0] * 2;\"=>\"\" S[\"v[0] = v[0] * 2;\"]",
            "int-increment@6:7 \"0\"=>\"1\" E[\"0\"]",
            "int-increment@6:14 \"0\"=>\"1\" E[\"0\"]",
            "mul-to-div@6:17 \"*\"=>\"/\" E[\"v[0] * 2\"]",
            "int-increment@6:19 \"2\"=>\"3\" E[\"2\"]",
            "int-decrement@6:19 \"2\"=>\"1\" E[\"2\"]",
            "return-err-default@7:5 \"Ok(())\"=>\"Err(Default::default())\" E[\"Ok(())\"]",
            "return-ok-default@9:37 \"Ok(1)\"=>\"Ok(Default::default())\" E[\"Ok(1)\"]",
            "return-err-default@9:37 \"Ok(1)\"=>\"Err(Default::default())\" E[\"Ok(1)\"]",
            "int-increment@9:40 \"1\"=>\"2\" E[\"1\"]",
            "int-decrement@9:40 \"1\"=>\"0\" E[\"1\"]",
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
            "delete-match-arm@3:14 \"x > 1\"=>\"false\" C[\"x > 1\"]",
            "remove-match-guard@3:14 \"x > 1\"=>\"true\" C[\"x > 1\"]",
            "gt-to-ge@3:16 \">\"=>\">=\" C[\"x > 1\"]",
            "int-increment@3:18 \"1\"=>\"2\" E[\"1\"]",
            "int-decrement@3:18 \"1\"=>\"0\" E[\"1\"]",
            "return-default@3:23 \"x | b\"=>\"Default::default()\" E[\"x | b\"]",
            "bor-to-band@3:25 \"|\"=>\"&\" E[\"x | b\"]",
            "return-default@4:14 \"(a ^ b) << 1 >> 1\"=>\"Default::default()\" E[\"(a ^ b) << 1 >> 1\"]",
            "xor-to-band@4:17 \"^\"=>\"&\" E[\"a ^ b\"]",
            "shl-to-shr@4:22 \"<<\"=>\">>\" E[\"(a ^ b) << 1\"]",
            "int-increment@4:25 \"1\"=>\"2\" E[\"1\"]",
            "int-decrement@4:25 \"1\"=>\"0\" E[\"1\"]",
            "shr-to-shl@4:27 \">>\"=>\"<<\" E[\"(a ^ b) << 1 >> 1\"]",
            "int-increment@4:30 \"1\"=>\"2\" E[\"1\"]",
            "int-decrement@4:30 \"1\"=>\"0\" E[\"1\"]",
        ]
    );
}

#[test]
fn every_place_passed_over_is_counted_under_the_outermost_reason() {
    let src = "#![no_std]\nconst A: bool = true;\nstatic B: i32 = 1 + 2;\nconst fn c(x: i32) -> i32 { x + 1 }\n#[cfg(feature = \"extra\")]\nfn d(x: i32) -> i32 { x - 1 }\n#[cfg(test)]\nmod tests {\n    fn t(x: i32) -> bool { x > 0 }\n}\n#[test]\nfn u() { assert!(1 < 2); }\nfn m(x: i32) -> i32 {\n    println!(\"{}\", x + 1);\n    let a = [0u8; 2 + 2];\n    a.len() as i32 * x\n}\nenum E { X = 1 + 1 }\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert!(d.no_std);
    assert_eq!(
        render(&d),
        [
            "int-increment@15:14 \"0u8\"=>\"1u8\" E[\"0u8\"]",
            "return-default@16:5 \"a.len() as i32 * x\"=>\"Default::default()\" E[\"a.len() as i32 * x\"]",
            "mul-to-div@16:20 \"*\"=>\"/\" E[\"a.len() as i32 * x\"]",
        ]
    );
    assert_eq!(
        skips(&d),
        [
            ("const-context", 16),
            ("macro-invocation", 1),
            ("cfg-attribute", 4),
            ("test-code", 8),
            ("const-fn-body", 4),
        ],
        "the body of a const fn is its own reason: what the compiler may evaluate at a call is \
         not what it evaluates in an initializer"
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
            "no-std-crate",
            "included-expression",
            "generated-outside-workspace",
            "forbidden-lints",
            "const-fn-body",
            "let-condition",
            "open-range",
            "unstated-return-type",
            "loop-value",
            "annotated",
            "configured",
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
fn a_line_index_borrows_one_source_and_reports_utf8_byte_offsets_exactly() {
    let source = "a日本\nz";
    let index = LineIndex::new(source).expect("small source");
    assert_eq!(
        index.position(2).expect("inside a scalar has a position"),
        rust_mutants::syntax::Position {
            line: 1,
            byte_column: 3,
            char_column: 2,
        }
    );
    assert_eq!(
        index.position(99).expect("past the end clamps exactly"),
        rust_mutants::syntax::Position {
            line: 2,
            byte_column: 2,
            char_column: 2,
        }
    );
}

#[test]
fn the_source_too_large_error_explains_the_one_based_wire_boundary() {
    let error = PositionError::SourceTooLarge { bytes: usize::MAX };
    assert!(error.to_string().contains("one-based u32"));
}

#[test]
fn nested_functions_keep_their_own_sites_and_the_file_root_runtime_depth() {
    let src = "struct S;\nimpl S {\n    fn m(&self, x: i32) -> i32 {\n        fn inner(y: i32) -> i32 { y * 2 }\n        inner(x) + 1\n    }\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    let depth = |rule: &str| {
        let f = d
            .candidates
            .iter()
            .find(|f| f.candidate.rule.name == rule)
            .expect(rule);
        f.hint.super_depth
    };
    assert_eq!(depth("mul-to-div"), 0);
    assert_eq!(depth("add-to-sub"), 0);
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

#[test]
fn a_file_that_does_not_parse_is_an_error_naming_the_line() {
    let selection = Selection::tier(registry(), Tier::All);
    let error = discover_file("src/bad.rs", b"fn f( {", &selection).expect_err("invalid syntax");
    assert!(matches!(&error, SyntaxError::Parse { .. }), "{error:?}");
    let SyntaxError::Parse { path, line, .. } = &error else {
        return;
    };
    assert_eq!(path, "src/bad.rs");
    assert_eq!(*line, 1);
    let error = discover_file("src/bin.rs", &[0xff, 0xfe], &selection)
        .expect_err("non-UTF-8 source is refused");
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
    let unknown =
        Selection::rules(registry(), &["no-such-rule"]).expect_err("an unknown rule is refused");
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
    let record = d.trace_record().expect("candidate count fits the trace");
    assert_eq!(record.path, "src/lib.rs");
    assert_eq!(record.candidates, 4);
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
            (3, 9, "int-increment", Some("E"), None),
            (3, 9, "int-decrement", Some("E"), None),
        ]
    );
    assert_eq!(record.skips.len(), 1);
    assert_eq!(record.skips[0].reason, "macro-invocation");
    assert_eq!(record.skips[0].count, 1);
    let json = serde_json::to_string(&record).expect("json");
    assert!(json.contains("\"candidates\":4"), "{json}");
}

#[test]
fn the_families_input_exercises_every_rule_and_matches_the_golden() {
    let root =
        njutest_devkit::paths::workspace_root().join("crates/rust-mutants/tests/testdata/syntax");
    let src = std::fs::read_to_string(root.join("families.input")).expect("input");
    let d = discover_every_rule(&src);
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
    njutest_devkit::golden::golden(&root.join("families.golden"), text.as_bytes()).expect("golden");
}

#[test]
fn the_families_input_in_crlf_finds_the_same_candidates_at_the_same_places() {
    let root =
        njutest_devkit::paths::workspace_root().join("crates/rust-mutants/tests/testdata/syntax");
    let src = std::fs::read_to_string(root.join("families.input")).expect("input");
    let with_crlf = rust_mutants::testkit::source::crlf(&src);
    let lf = discover(&src);
    let crlf = discover(&with_crlf);
    assert_coherent(&with_crlf, &crlf);
    let differences: Vec<String> = render(&lf)
        .into_iter()
        .zip(
            render(&crlf)
                .into_iter()
                .map(|line| line.replace("\\r\\n", "\\n")),
        )
        .filter(|(one, other)| one != other)
        .map(|(one, other)| format!("{one}\n  became {other}"))
        .collect();
    assert!(
        differences.is_empty(),
        "a file whose lines end the other way is the same program, and the walk finds the same \
         candidates at the same lines and columns:\n{}",
        differences.join("\n")
    );
    assert_eq!(
        render(&lf).len(),
        render(&crlf).len(),
        "and finds neither more nor fewer of them"
    );
    let skips = |found: &FileDiscovery| {
        let mut lines: Vec<String> = found
            .skips
            .iter()
            .map(|skip| format!("{} {}", skip.reason.name(), skip.count))
            .collect();
        lines.sort();
        lines
    };
    assert_eq!(skips(&lf), skips(&crlf), "and passes the same places over");
}

#[test]
fn forms_display_as_their_letters() {
    assert_eq!(Form::C.to_string(), "C");
    assert_eq!(Form::E.to_string(), "E");
    assert_eq!(Form::S.to_string(), "S");
    assert_eq!(Form::M.to_string(), "M");
}

fn by_rule(discovery: &FileDiscovery, names: &[&str]) -> Vec<String> {
    discovery
        .candidates
        .iter()
        .filter(|found| names.contains(&found.candidate.rule.name))
        .map(|found| {
            format!(
                "{}@{} {:?}=>{:?} {}",
                found.candidate.rule.name,
                found.position.line,
                std::str::from_utf8(&found.candidate.original)
                    .expect("the Rust source fixture is exact UTF-8"),
                std::str::from_utf8(&found.candidate.replacement)
                    .expect("the generated Rust replacement is exact UTF-8"),
                found.hint.form,
            )
        })
        .collect()
}

#[test]
fn a_boolean_method_is_negated_except_where_another_rule_already_asks() {
    let src = "use std::collections::BTreeMap;\nfn f(s: &str, v: &[i32], m: &BTreeMap<i32, i32>, o: Option<i32>) -> bool {\n    if s.is_empty() {\n        return v.contains(&1);\n    }\n    while s.starts_with(\"a\") {\n        break;\n    }\n    if !m.contains_key(&1) && s.ends_with(\"b\") {\n        return o.is_some();\n    }\n    m.is_empty()\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["negate-bool-method"]),
        [
            "negate-bool-method@4 \"v.contains(&1)\"=>\"!(v.contains(&1))\" E",
            "negate-bool-method@9 \"s.ends_with(\\\"b\\\")\"=>\"!(s.ends_with(\\\"b\\\"))\" C",
            "negate-bool-method@12 \"m.is_empty()\"=>\"!(m.is_empty())\" E",
        ],
        "the whole of an `if` or `while` condition is what `negate-condition` and \
         `negate-loop-condition` already ask about, what sits under a `!` is what `remove-not` \
         asks about, and `is_some` and its three companions are what the swaps ask about"
    );
}

#[test]
fn an_err_default_is_offered_only_where_the_error_type_spells_a_default() {
    let src = "pub struct BoundError;\nfn a() -> Result<i32, String> {\n    Ok(1)\n}\nfn b() -> Result<i32, Box<dyn std::error::Error>> {\n    Ok(1)\n}\nfn c() -> Result<i32, BoundError> {\n    Ok(1)\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["return-err-default"]),
        ["return-err-default@3 \"Ok(1)\"=>\"Err(Default::default())\" E"],
        "a `String` spells a default; a boxed trait object cannot have one, and a crate's own \
         error type by convention does not, so offering either is a candidate the compiler \
         refuses at nearly every `Result` in a program"
    );
    assert!(
        d.skips
            .iter()
            .any(|skip| skip.reason == SkipReason::UnstatedReturnType),
        "and the place it was not offered says why"
    );
}

#[test]
fn a_saturating_operation_is_asked_what_happens_when_it_wraps_instead() {
    let src = "fn f(a: i32, b: i32) -> i32 {\n    let x = a.saturating_add(b);\n    let y = x.saturating_sub(b);\n    y.saturating_mul(2)\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(
            &d,
            &[
                "saturating-add-to-wrapping-add",
                "saturating-sub-to-wrapping-sub",
                "saturating-mul-to-wrapping-mul",
            ]
        ),
        [
            "saturating-add-to-wrapping-add@2 \"saturating_add\"=>\"wrapping_add\" E",
            "saturating-sub-to-wrapping-sub@3 \"saturating_sub\"=>\"wrapping_sub\" E",
            "saturating-mul-to-wrapping-mul@4 \"saturating_mul\"=>\"wrapping_mul\" E",
        ],
        "a quantity that clamps at a boundary is one the boundary is load-bearing for, \
         and a mutant that wraps is the edit that asks whether anything noticed"
    );
}

#[test]
fn iterator_and_slice_method_swaps_edit_only_the_identifier() {
    let src = "fn f(v: &[i32]) -> bool {\n    let _ = v.iter().skip(1).take(2).sum::<i32>();\n    let _ = v.first();\n    let _ = v.iter().product::<i32>();\n    let _ = v.last();\n    v.iter().all(|n| *n > 0) && v.iter().any(|n| *n > 0)\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(
            &d,
            &[
                "skip-to-take",
                "take-to-skip",
                "sum-to-product",
                "product-to-sum",
                "first-to-last",
                "last-to-first",
                "all-to-any",
                "any-to-all",
            ]
        ),
        [
            "skip-to-take@2 \"skip\"=>\"take\" E",
            "take-to-skip@2 \"take\"=>\"skip\" E",
            "sum-to-product@2 \"sum\"=>\"product\" E",
            "first-to-last@3 \"first\"=>\"last\" E",
            "product-to-sum@4 \"product\"=>\"sum\" E",
            "last-to-first@5 \"last\"=>\"first\" E",
            "all-to-any@6 \"all\"=>\"any\" C",
            "any-to-all@6 \"any\"=>\"all\" C",
        ],
        "the identifier is the whole of the edit"
    );

    let sites: Vec<&str> = d
        .candidates
        .iter()
        .filter(|one| one.candidate.rule.name.ends_with("-to-take"))
        .map(|one| one.hint.site_text.as_str())
        .collect();
    assert_eq!(
        sites,
        ["v.iter().skip(1).take(2).sum::<i32>()"],
        "`skip` and `take` do not produce the same type, so the guard stands at the end of \
         the chain, where the two branches meet again"
    );
}

#[test]
fn an_arm_is_deletable_only_when_a_bare_wildcard_follows_it() {
    let src = "fn f(n: i32) -> i32 {\n    match n {\n        0 => 1,\n        n if n < 5 => 2,\n        _ => 3,\n    }\n}\nfn g(n: i32) -> i32 {\n    match n {\n        0 => 1,\n        n if n < 5 => 2,\n        n => n,\n    }\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["delete-match-arm", "remove-match-guard"]),
        [
            "delete-match-arm@3 \"\"=>\"false\" M",
            "delete-match-arm@4 \"n < 5\"=>\"false\" C",
            "remove-match-guard@4 \"n < 5\"=>\"true\" C",
            "remove-match-guard@11 \"n < 5\"=>\"true\" C",
        ],
        "an arm the match still covers without it is one a suite should notice the loss of, \
         and an arm the match needs for exhaustiveness is a mutation that does not compile"
    );
}

#[test]
fn a_guard_written_onto_an_arm_is_the_one_splice_that_adds_syntax() {
    let src = "fn f(n: i32) -> i32 {\n    match n {\n        0 => 1,\n        _ => 3,\n    }\n}\n";
    let d = discover(src);
    let found = d
        .candidates
        .iter()
        .find(|one| one.candidate.rule.name == "delete-match-arm")
        .expect("the deletable arm");
    assert!(
        found.candidate.span.is_empty(),
        "there is no guard to replace, so the edit is the empty place a guard would go: {:?}",
        found.candidate.span
    );
    assert_eq!(
        found.hint.site_text, "0",
        "the site is the pattern the guard goes after, which the guard keeps verbatim"
    );
    assert_eq!(found.hint.site.end, found.candidate.span.start);
    assert_eq!(found.hint.form, Form::M);
}

#[test]
fn break_and_continue_swap_with_their_label_and_never_carry_a_value() {
    let src = "fn f(v: &[i32]) -> i32 {\n    let mut n = 0;\n    'outer: for x in v {\n        for y in v {\n            if *y == 0 {\n                continue 'outer;\n            }\n            if *x == 0 {\n                break;\n            }\n            n += 1;\n        }\n    }\n    let m = loop {\n        break 7;\n    };\n    n + m\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["break-to-continue", "continue-to-break"]),
        [
            "continue-to-break@6 \"continue 'outer\"=>\"break 'outer\" E",
            "break-to-continue@9 \"break\"=>\"continue\" E",
        ],
        "the label says which loop, so a swap keeps it; a `break` that carries a value has no \
         `continue` to become"
    );
}

#[test]
fn a_terminal_else_is_deleted_only_where_the_if_stands_as_a_statement() {
    let src = "fn f(a: i32, b: i32) -> i32 {\n    let mut n = 0;\n    if a > b {\n        n += 1;\n    } else if a == b {\n        n += 2;\n    } else {\n        n += 3;\n    }\n    let m = if a > b { 1 } else { 2 };\n    if a > 0 {\n        n += 4;\n    }\n    n + m\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["delete-else-branch"]),
        ["delete-else-branch@7 \" else {\\n        n += 3;\\n    }\"=>\"\" S"],
        "an `if` that stands as a statement can lose its last `else` and still be a statement; \
         an `if` that is a value cannot, and an `if` without an `else` has none to lose"
    );
}

#[test]
fn integer_literals_move_by_one_in_their_own_radix_and_keep_their_suffix() {
    let src = "fn f() -> (i32, u8, u32, usize, i64) {\n    let a = 10;\n    let b = 0xffu8;\n    let c = 0b1010;\n    let d = 0;\n    let e = 1_000i64;\n    (a, b, c, d, e)\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["int-increment", "int-decrement"]),
        [
            "int-increment@2 \"10\"=>\"11\" E",
            "int-decrement@2 \"10\"=>\"9\" E",
            "int-decrement@3 \"0xffu8\"=>\"0xfeu8\" E",
            "int-increment@4 \"0b1010\"=>\"0b1011\" E",
            "int-decrement@4 \"0b1010\"=>\"0b1001\" E",
            "int-increment@5 \"0\"=>\"1\" E",
            "int-increment@6 \"1_000i64\"=>\"1001i64\" E",
            "int-decrement@6 \"1_000i64\"=>\"999i64\" E",
        ],
        "a literal is respelled in the radix it was written in and keeps its suffix; `0` has \
         no predecessor the syntax spells, and a value the suffix cannot hold has no successor"
    );
}

#[test]
fn a_non_empty_string_becomes_empty() {
    let src = "fn f() -> (&'static str, &'static str) {\n    (\"hello\", \"\")\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(
        by_rule(&d, &["string-to-empty"]),
        ["string-to-empty@2 \"\\\"hello\\\"\"=>\"\\\"\\\"\" E"],
        "a message nobody checks is a message nobody would miss, and a string already empty \
         has nowhere to go"
    );
}

#[test]
fn an_end_of_line_marker_hides_that_lines_candidates_and_states_the_text() {
    let src = "fn f(a: i32, b: i32) -> i32 {\n    if a > b { a } else { b } // rust-mutants: skip the bound is the caller's\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert!(d.candidates.is_empty(), "{:?}", render(&d));
    assert_eq!(
        skips(&d),
        [("annotated", 7)],
        "every edit that starts on the marked line is hidden, and the count says how many"
    );
    let claim = d.annotations.first().expect("a claim");
    assert_eq!(claim.line, 2);
    assert!(claim.matched);
    assert_eq!(claim.reason, "the bound is the caller's");
    assert!(
        d.decisions
            .iter()
            .all(|one| one.note.as_deref() == Some("the bound is the caller's")),
        "a reader asking why gets the reason its author wrote: {:?}",
        d.decisions
    );
}

#[test]
fn an_own_line_marker_hides_the_next_item_statement_arm_or_else_block() {
    let src = "// rust-mutants: skip generated\nfn f(a: i32) -> i32 {\n    a + 1\n}\nfn g(a: i32, b: i32) -> i32 {\n    let mut n = 0;\n    // rust-mutants: skip measured elsewhere\n    if a > b {\n        n += a;\n    }\n    n + b\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    let left: Vec<&str> = d
        .candidates
        .iter()
        .map(|found| found.candidate.rule.name)
        .collect();
    assert_eq!(
        left,
        ["int-increment", "return-default", "add-to-sub"],
        "a marker with the line to itself takes the whole of what starts on the next one"
    );
    assert_eq!(d.annotations.len(), 2);
    assert!(d.annotations.iter().all(|claim| claim.matched));
}

#[test]
fn a_marker_inside_a_string_is_not_a_marker_and_a_block_comment_marker_is() {
    let src = "fn f() -> &'static str {\n    \"rust-mutants: skip nothing\"\n}\nfn g(a: i32) -> i32 {\n    /* rust-mutants: skip the offset is a constant */ a + 1\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    assert_eq!(d.annotations.len(), 1, "{:?}", d.annotations);
    assert_eq!(d.annotations[0].line, 5);
    assert!(
        render(&d)
            .iter()
            .any(|line| line.starts_with("string-to-empty@2")),
        "a string that says the words is a string: {:?}",
        render(&d)
    );
}

#[test]
fn a_marker_without_a_reason_is_rm2008_and_an_unknown_directive_is_rm2009() {
    let selection = Selection::tier(registry(), Tier::All);
    let bare = "// rust-mutants: skip\nfn f(a: i32) -> i32 {\n    a + 1\n}\n";
    let error = discover_file("src/lib.rs", bare.as_bytes(), &selection)
        .expect_err("a marker without a reason is refused");
    assert!(
        matches!(error, SyntaxError::AnnotationWithoutReason { line: 1, .. }),
        "{error:?}"
    );
    assert_eq!(error.code().code, "RM2008", "{error}");
    let unknown = "// rust-mutants: hide me\nfn f(a: i32) -> i32 {\n    a + 1\n}\n";
    let error = discover_file("src/lib.rs", unknown.as_bytes(), &selection)
        .expect_err("an unknown directive is refused");
    assert!(
        matches!(error, SyntaxError::UnknownAnnotation { line: 1, .. }),
        "{error:?}"
    );
    assert_eq!(error.code().code, "RM2009", "{error}");
}

#[test]
fn a_marker_that_hides_nothing_is_reported_so_it_can_be_removed() {
    let src = "// rust-mutants: skip nothing to hide\n// a plain comment\nfn f() {}\n";
    let d = discover(src);
    assert_eq!(d.annotations.len(), 1);
    assert!(
        !d.annotations[0].matched,
        "a marker over a place no rule targets is one somebody should take out: {:?}",
        d.annotations
    );
}

#[test]
fn a_marker_over_cfg_code_is_matched_by_the_sites_the_walker_still_sees() {
    let src = "// rust-mutants: skip windows only\n#[cfg(windows)]\nfn f(a: i32) -> i32 {\n    a + 1\n}\n";
    let d = discover(src);
    assert!(
        d.annotations[0].matched,
        "the walker sees the sites whatever the platform, so the answer does not change with it"
    );
}

#[test]
fn every_candidate_names_the_item_it_sits_in() {
    let src = "pub mod inner {\n    pub struct Counter {\n        pub n: u32,\n    }\n    impl Counter {\n        pub fn bump(&mut self) -> bool {\n            self.n += 1;\n            self.n > 10\n        }\n    }\n    impl std::fmt::Debug for Counter {\n        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {\n            write!(f, \"{}\", self.n + 1)\n        }\n    }\n}\npub fn free(a: i32) -> i32 {\n    a + 1\n}\n";
    let d = discover(src);
    assert_coherent(src, &d);
    let items: Vec<(&str, &str)> = d
        .candidates
        .iter()
        .map(|found| (found.candidate.rule.name, found.item.as_str()))
        .collect();
    assert!(
        items.contains(&("gt-to-ge", "inner::Counter::bump")),
        "a method is named by its module, its type and itself: {items:?}"
    );
    assert!(
        items.contains(&("return-default", "free")),
        "a free function is named by itself: {items:?}"
    );
    assert!(
        items
            .iter()
            .all(|(_, item)| !item.is_empty() && !item.starts_with("::")),
        "nothing is nameless and nothing starts with a separator: {items:?}"
    );
}

#[test]
fn a_borrowed_empty_slice_is_already_what_the_replacement_would_write() {
    let src = "fn s() -> &'static [i32] {\n    &[]\n}\n";
    assert!(
        !render(&discover(src))
            .iter()
            .any(|one| one.contains("return-default")),
        "`&[]` is what `<&[T]>::default()` produces, so replacing it writes the same value \
         again: an equivalent mutant a reader has to explain away, offered where the syntax \
         could have refused it: {:?}",
        render(&discover(src))
    );
}

#[test]
fn a_borrowed_value_that_is_not_the_default_is_still_offered() {
    let src = "fn s() -> &'static [i32] {\n    &[1]\n}\n";
    assert!(
        render(&discover(src))
            .iter()
            .any(|one| one.contains("return-default")),
        "a borrow of something is not a borrow of nothing, and refusing this one would drop a \
         mutation the tests can notice: {:?}",
        render(&discover(src))
    );
}

#[test]
fn a_condition_is_asked_to_stand_still_as_well_as_to_invert() {
    let src = "fn f(a: i32, b: i32) -> i32 {\n    if a < b {\n        1\n    } else {\n        2\n    }\n}\n";
    let rendered = render(&discover(src));
    assert!(
        rendered.contains(&"condition-to-true@2:8 \"a < b\"=>\"true\" C[\"a < b\"]".to_owned())
            && rendered
                .contains(&"condition-to-false@2:8 \"a < b\"=>\"false\" C[\"a < b\"]".to_owned()),
        "a suite that kills `negate-condition` has one test whose branch changed, and that \
         one test kills exactly one of these two: the pair says which side is checked and \
         which is not, where the negation says only that one of them is. It is the same \
         question `remove-match-guard` and `delete-match-arm` already ask of an arm: {rendered:?}"
    );
}

#[test]
fn a_condition_that_is_already_a_literal_is_not_asked_to_become_itself() {
    let src = "fn f() -> i32 {\n    if true {\n        1\n    } else {\n        2\n    }\n}\n";
    let rendered = render(&discover(src));
    assert!(
        !rendered.iter().any(|one| one.contains("condition-to-true")),
        "writing `true` where `true` is written is a mutation with no mutation in it, and an \
         equivalent mutant offered by construction is worse than one nobody thought of: \
         {rendered:?}"
    );
}

#[test]
fn a_loop_is_not_asked_to_run_forever() {
    let src = "fn f(mut n: i32) {\n    while n > 0 {\n        n -= 1;\n    }\n}\n";
    let rendered = render(&discover(src));
    assert!(
        !rendered.iter().any(|one| one.contains("condition-to-true")),
        "`while true` does not answer a question about the tests: it hangs, the run bounds it, \
         and the bound is recorded as a kill that nobody learned anything from. The cost is the \
         whole timeout and the signal is zero: {rendered:?}"
    );
}
