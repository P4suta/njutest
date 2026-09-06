// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a probed site becomes, and that a probed file still compiles and still runs the program it would have run.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::process::Command;

use rust_mutants::probe::form::Question;
use rust_mutants::probe::instrument::{BINDING, Site, rewrite};
use rust_mutants::probe::runtime::{MODULE_STEM, render};
use rust_mutants::span::Span;

const fn site(question: Question, super_depth: u32) -> Site {
    Site {
        index: 7,
        slot: 0,
        span: Span { start: 0, end: 0 },
        question,
        super_depth,
    }
}

fn build(source: &str, name: &str) -> std::process::Output {
    let dir = mjutest_devkit::paths::workspace_root()
        .join("target/probe-instrument")
        .join(format!("{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a place to build");
    let path = dir.join(format!("{name}.rs"));
    std::fs::write(&path, source).expect("write");
    Command::new("rustc")
        .args(["--edition", "2024", "--crate-type", "lib"])
        .arg("--out-dir")
        .arg(&dir)
        .arg(&path)
        .output()
        .expect("rustc runs")
}

#[test]
fn a_rewrite_binds_the_value_once_and_gives_it_back_unchanged() {
    let text = rewrite(&site(Question::Default, 0), "compute()", MODULE_STEM);
    assert!(text.starts_with("({ "), "{text}");
    assert!(text.ends_with(&format!("{BINDING} }})")), "{text}");
    assert_eq!(
        text.matches("compute()").count(),
        1,
        "evaluated once: {text}"
    );
    assert!(!text.contains('\n'), "a rewrite costs no line: {text}");
    assert!(text.contains("infect(0, 7)"), "{text}");
}

#[test]
fn each_question_asks_its_own_way() {
    let default = rewrite(&site(Question::Default, 0), "x", MODULE_STEM);
    assert!(default.contains("if !(&__rmp_value).probed()"), "{default}");

    let truth = rewrite(&site(Question::True, 0), "x", MODULE_STEM);
    assert!(truth.contains("if !__rmp_value"), "{truth}");

    let ok = rewrite(&site(Question::OkDefault, 0), "x", MODULE_STEM);
    assert!(ok.contains("Result::Ok(inner)"), "{ok}");
    assert!(ok.contains("Result::Err(_)"), "{ok}");

    let some = rewrite(&site(Question::SomeDefault, 0), "x", MODULE_STEM);
    assert!(some.contains("Option::Some(inner)"), "{some}");
    assert!(some.contains("Option::None"), "{some}");
}

#[test]
fn a_site_inside_a_module_reaches_the_runtime_at_the_file_root() {
    let deep = rewrite(&site(Question::Default, 2), "x", MODULE_STEM);
    assert!(deep.contains("super::super::__rmp::infect"), "{deep}");
    assert!(
        deep.contains("use super::super::__rmp::{FloatRefuse as _, Probe as _};"),
        "{deep}"
    );
    let shallow = rewrite(&site(Question::Default, 0), "x", MODULE_STEM);
    assert!(
        shallow.contains("use self::__rmp::{FloatRefuse as _, Probe as _};"),
        "a use path's first segment names a crate unless it says otherwise: {shallow}"
    );
}

#[test]
fn a_probed_file_compiles_at_the_root_and_inside_a_module() {
    let runtime = render(MODULE_STEM, &"a".repeat(64), 4, &[0, 1, 2, 3]);
    let source = format!(
        "pub fn number() -> i32 {{ {} }}\n\
         pub fn truth() -> bool {{ {} }}\n\
         pub fn wrapped() -> Result<i32, ()> {{ {} }}\n\
         pub fn maybe() -> Option<i32> {{ {} }}\n\
         pub mod inner {{\n\
         \x20   pub fn number() -> i32 {{ {} }}\n\
         }}\n{runtime}",
        rewrite(&site(Question::Default, 0), "1 + 1", MODULE_STEM),
        rewrite(
            &Site {
                slot: 1,
                ..site(Question::True, 0)
            },
            "false",
            MODULE_STEM
        ),
        rewrite(
            &Site {
                slot: 2,
                ..site(Question::OkDefault, 0)
            },
            "Ok(3)",
            MODULE_STEM
        ),
        rewrite(
            &Site {
                slot: 3,
                ..site(Question::SomeDefault, 0)
            },
            "Some(4)",
            MODULE_STEM
        ),
        rewrite(&site(Question::Default, 1), "5", MODULE_STEM),
    );
    let output = build(&source, "probed");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn a_probe_of_a_float_returning_function_is_refused_by_the_compiler() {
    let runtime = render(MODULE_STEM, &"a".repeat(64), 1, &[0]);
    let source = format!(
        "pub fn ratio() -> f64 {{ {} }}\n{runtime}",
        rewrite(&site(Question::Default, 0), "1.5", MODULE_STEM)
    );
    let output = build(&source, "probed_float");
    assert!(
        !output.status.success(),
        "a float probe is a compile error rather than a wrong answer"
    );
}
