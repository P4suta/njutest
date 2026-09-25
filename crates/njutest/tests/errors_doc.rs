// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every error code the runner can report is documented, and every documented code exists.

#![expect(
    clippy::expect_used,
    clippy::disallowed_methods,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::{BTreeMap, BTreeSet};

use njutest::error::{NjCode, error_codes};

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
fn every_runner_error_code_is_documented_and_every_documented_code_exists() {
    let in_code: BTreeSet<String> = error_codes().iter().map(|c| c.code.to_owned()).collect();
    assert!(
        !in_code.is_empty(),
        "the runner must declare its error codes"
    );
    assert_eq!(
        in_code,
        documented_codes("NJ"),
        "docs/errors.md and njutest::error::error_codes disagree"
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
                && code.starts_with("NJ")
                && code.chars().skip(2).all(|c| c.is_ascii_digit()),
            "malformed code {code}"
        );
    }
}

#[test]
fn every_failure_reports_a_declared_code() {
    let declared: BTreeSet<&str> = error_codes().iter().map(|c| c.code).collect();
    for sample in njutest::testkit::every_failure() {
        let carried = sample.code().code;
        let engine = rust_mutants::error::error_codes()
            .iter()
            .any(|code| code.code == carried);
        assert!(
            declared.contains(carried) || engine,
            "{sample:?} reports {carried} and neither ledger declares it, so a person \
             searching docs/errors.md for what they were told finds nothing. The engine's \
             codes travel through the runner under their own names on purpose: renaming \
             them would make a user's report unsearchable"
        );
    }
}

#[test]
fn every_code_the_runner_declares_is_one_some_place_reports() {
    let names = constants();
    let mut reported = BTreeSet::new();
    for text in sources() {
        for code in error_codes() {
            let constant = names.get(code.code).map_or("", String::as_str);
            if text.contains(code.code) || (!constant.is_empty() && text.contains(constant)) {
                reported.extend([code.code]);
            }
        }
    }
    let orphaned: Vec<&str> = error_codes()
        .iter()
        .map(|code| code.code)
        .filter(|code| !reported.contains(code))
        .collect();
    assert!(
        orphaned.is_empty(),
        "these codes are declared and documented and no place in the runner reports \
         them. A code in the ledger that nothing can print is a page about something \
         that cannot happen, and a reader who trusts the ledger to be the whole list \
         has no way to tell which entries are real: {orphaned:?}"
    );
}

/// The constant each code is declared under, as the ledger names it, refusing any declaration it cannot read.
fn constants() -> BTreeMap<String, String> {
    let (table, rest) = ledger_parts();
    let variants: BTreeMap<String, String> = table
        .split("Self::")
        .skip(1)
        .filter_map(|arm| {
            let (variant, rest) = arm.split_once("=>ErrorCode{")?;
            let (_, quoted) = rest.split_once("code:\"")?;
            let (code, _) = quoted.split_once('"')?;
            Some((variant.to_owned(), code.to_owned()))
        })
        .collect();
    assert_eq!(
        variants.len(),
        NjCode::ALL.len(),
        "the scanner reads one arm per code, or a law resting on it would pass over what it \
         could not read: {variants:?}"
    );
    let declared = rest.matches(":ErrorCode=").count();
    let named: BTreeMap<String, String> = rest
        .split(';')
        .filter_map(|statement| {
            let (head, tail) = statement.split_once(":ErrorCode=NjCode::")?;
            let (variant, _) = tail.split_once('.')?;
            let name = head.rsplit("const").next()?;
            Some((variants.get(variant)?.clone(), name.to_owned()))
        })
        .collect();
    assert_eq!(
        named.len(),
        declared,
        "every `: ErrorCode =` outside the table is a constant read from one of its arms, or \
         the scanner cannot say whether it is reported: {named:?}"
    );
    named
}

/// `text` with every whitespace character taken out, so no layout a formatter picks changes what is read.
fn squeezed(text: &str) -> String {
    text.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The ledger's table and the rest of the ledger, each squeezed.
fn ledger_parts() -> (String, String) {
    let text = ledger();
    let (before, table) = text
        .split_once("mod table {")
        .expect("the ledger holds its table");
    let (table, after) = table
        .split_once("\n}\n")
        .expect("the table closes at the top level");
    (squeezed(table), squeezed(&format!("{before}{after}")))
}

/// The ledger's own text.
fn ledger() -> String {
    std::fs::read_to_string(
        njutest_devkit::paths::workspace_root().join("crates/njutest/src/error.rs"),
    )
    .expect("the error ledger")
}

/// The text of every Rust file of the runner, with the ledger's declarations taken out.
fn sources() -> Vec<String> {
    let root = njutest_devkit::paths::workspace_root().join("crates/njutest/src");
    let mut found = Vec::new();
    let mut pending = vec![root];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .expect("a directory")
            .map(|entry| entry.expect("every source entry is readable"))
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|kind| kind == "rs") {
                found.push(if path.file_name().is_some_and(|name| name == "error.rs") {
                    declarations_removed()
                } else {
                    squeezed(&std::fs::read_to_string(&path).expect("a source file"))
                });
            }
        }
    }
    found
}

/// The ledger, squeezed, with the table and every declaration taken out, so a declaration is not a report.
fn declarations_removed() -> String {
    let rest = ledger_parts().1;
    rest.split(';')
        .filter(|statement| !statement.contains(":ErrorCode=NjCode::"))
        .collect::<Vec<_>>()
        .join(";")
}

#[test]
fn every_configuration_failure_has_a_code_in_the_configuration_area() {
    for kind in njutest::config::ConfigErrorKind::ALL {
        assert!(
            kind.code().code.starts_with("NJ1"),
            "{kind:?} is not in the configuration area"
        );
        assert!(!kind.code().summary.is_empty());
    }
}

/// A diagnostic that names what went wrong and stops has left a reader to find the way out.
#[test]
fn every_code_says_what_to_do_about_it() {
    let silent: Vec<&str> = error_codes()
        .iter()
        .filter(|code| code.remedy.trim().is_empty())
        .map(|code| code.code)
        .collect();
    assert!(
        silent.is_empty(),
        "these say what went wrong and nothing a reader can act on. Where the answer is \
         that the fault is this tool's, that is worth saying too: somebody reading it \
         would otherwise spend an afternoon looking for the mistake they made. {silent:?}"
    );
}

/// The page a reader searches by code says the same thing the code carries.
#[test]
fn every_code_documents_the_remedy_it_carries() {
    let text =
        std::fs::read_to_string(njutest_devkit::paths::workspace_root().join("docs/errors.md"))
            .expect("docs/errors.md");
    let documented: BTreeMap<String, String> = text
        .lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, rest) = cell.split_once("` | ")?;
            let remedy = rest.rsplit_once(" |")?.0.rsplit_once(" | ")?.1;
            code.starts_with("NJ")
                .then(|| (code.to_owned(), remedy.trim().to_owned()))
        })
        .collect();
    let wrong: Vec<String> = error_codes()
        .iter()
        .filter(|code| documented.get(code.code).map_or("", String::as_str) != code.remedy)
        .map(|code| code.code.to_owned())
        .collect();
    assert!(
        wrong.is_empty(),
        "a reader who searches the page by code is told something the code does not \
         carry, which is worse than being told nothing: {wrong:?}"
    );
}
