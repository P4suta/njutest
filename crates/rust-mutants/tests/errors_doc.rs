// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every error code the engine can report is documented, and every documented code exists.
//! The table in `docs/errors.md` is the reader-facing ledger; this test keeps it from drifting from the code in either direction.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use rust_mutants::error::error_codes;

fn documented_codes(prefix: &str) -> BTreeSet<String> {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    text.lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, _) = cell.split_once('`')?;
            code.starts_with(prefix).then(|| code.to_owned())
        })
        .collect()
}

#[test]
fn every_engine_error_code_is_documented_and_every_documented_code_exists() {
    let in_code: BTreeSet<String> = error_codes().iter().map(|c| c.code.to_owned()).collect();
    assert!(
        !in_code.is_empty(),
        "the engine must declare its error codes"
    );
    let in_docs = documented_codes("RM");
    assert_eq!(
        in_code, in_docs,
        "docs/errors.md and rust_mutants::error::error_codes disagree"
    );
}

#[test]
fn error_codes_are_unique_well_formed_and_sorted() {
    let codes: Vec<&str> = error_codes().iter().map(|c| c.code).collect();
    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "codes are listed in code order");
    for code in &codes {
        assert!(
            code.len() == 6
                && code.starts_with("RM")
                && code.chars().skip(2).all(|c| c.is_ascii_digit()),
            "malformed code {code}"
        );
    }
}

#[test]
fn every_variant_reports_a_declared_code() {
    let declared: BTreeSet<&str> = error_codes().iter().map(|c| c.code).collect();
    let snapshot = rust_mutants::snapshot::cleanup_guard(
        std::path::Path::new("relative"),
        std::path::Path::new("/parent"),
    )
    .expect_err("a relative path is refused");
    let cargo = rust_mutants::cargo::parse_dep_info("").expect_err("empty dep-info is refused");
    let discover = rust_mutants::discover::DiscoverError::UnknownPackage {
        name: "x".to_owned(),
    };
    let instrument = rust_mutants::instrument::plan_file(
        &rust_mutants::catalog::Builder::new()
            .build()
            .expect("catalog"),
        "src/lib.rs",
        &rust_mutants::syntax::discover_file(
            "src/lib.rs",
            b"pub fn f(a: i32) -> i32 { a + 1 }\n",
            &rust_mutants::syntax::Selection::tier(
                &rust_mutants::rule::Registry::canonical(),
                rust_mutants::rule::Tier::All,
            ),
        )
        .expect("discover")
        .candidates,
    )
    .expect_err("an empty catalog holds nothing");
    let artifact = rust_mutants::equivalence::artifacts::digests([(
        "missing",
        std::path::Path::new("this-artifact-does-not-exist"),
    )])
    .expect_err("the artifact does not exist");
    let samples = [
        rust_mutants::EngineError::Interrupted,
        rust_mutants::EngineError::from(snapshot),
        rust_mutants::EngineError::from(cargo),
        rust_mutants::EngineError::from(discover),
        rust_mutants::EngineError::from(instrument),
        rust_mutants::EngineError::from(rust_mutants::validate::ValidateError::NotIsolated {
            suspects: 2,
        }),
        rust_mutants::EngineError::from(artifact),
        rust_mutants::EngineError::from(rust_mutants::sentinel::SentinelError::Unwritable {
            path: std::path::PathBuf::from("planted"),
            source: std::io::Error::other("refused"),
        }),
    ];
    for sample in &samples {
        assert!(
            declared.contains(sample.code().code),
            "{sample:?} reports an undeclared code"
        );
    }
}

#[test]
fn every_code_documents_the_remedy_it_carries() {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    let documented: std::collections::BTreeMap<String, String> = text
        .lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, rest) = cell.split_once("` | ")?;
            let remedy = rest.rsplit_once(" |")?.0.rsplit_once(" | ")?.1;
            code.starts_with("RM")
                .then(|| (code.to_owned(), remedy.trim().to_owned()))
        })
        .collect();
    let mut wrong = Vec::new();
    for code in error_codes() {
        let said = documented.get(code.code).map_or("", String::as_str);
        let carried = code.remedy.unwrap_or("—");
        if said != carried {
            wrong.push(format!(
                "{}: table says {said:?}, code carries {carried:?}",
                code.code
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "the remedy a reader is told is the remedy the code carries: {wrong:?}"
    );
}

/// A diagnostic that names what went wrong and stops has left a reader to find the way out.
#[test]
fn every_code_says_what_to_do_about_it() {
    let silent: Vec<&str> = error_codes()
        .iter()
        .filter(|code| code.remedy.is_none_or(str::is_empty))
        .map(|code| code.code)
        .collect();
    assert!(
        silent.is_empty(),
        "these say what went wrong and nothing a reader can act on. Where the answer is \
         that the fault is this tool's, that is worth saying too: somebody reading it \
         would otherwise spend an afternoon looking for the mistake they made. {silent:?}"
    );
}
