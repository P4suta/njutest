// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The registry of critical decisions: every cell names what the tree defines, and every hole is one somebody owns.

#![expect(
    clippy::expect_used,
    reason = "a scratch repository that cannot be made leaves no base to test"
)]

use std::collections::BTreeSet;

use xtask::invariants::{Cell, InvariantError, Layer, check, defined, gaps, regressions, rows};

/// A registry page whose one row holds `decision` by `oracle` and leaves every other layer open.
fn page(decision: &str, oracle: &str) -> String {
    format!(
        "# Invariants\n\n\
         | Decision | Invariant | Types | Self-check | Oracle | Plant | Mutation | States | Blind |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n\
         | {decision} | what it promises | none | none | {oracle} | none | none | none | nothing |\n"
    )
}

/// A ledger owning every layer of `decision` but the oracle.
fn owned(decision: &str) -> String {
    let mut ledger = String::new();
    for layer in ["types", "self-check", "plant", "mutation", "states"] {
        ledger.push_str(decision);
        ledger.push(' ');
        ledger.push_str(layer);
        ledger.push_str(" somebody\n");
    }
    ledger
}

fn names(list: &[&str]) -> BTreeSet<String> {
    list.iter().map(|name| (*name).to_owned()).collect()
}

#[test]
fn a_row_holds_each_layer_by_names_or_leaves_it_open() {
    let read = rows(&page("swap", "`every_swap_keeps_its_tree`, `a_specimen`"));
    let Ok([row]) = read.as_deref() else {
        panic!("one row: {read:?}");
    };
    assert_eq!(row.decision, "swap");
    assert_eq!(
        row.cells.get(&Layer::Oracle),
        Some(&Cell::Held(vec![
            "every_swap_keeps_its_tree".to_owned(),
            "a_specimen".to_owned()
        ]))
    );
    assert_eq!(row.cells.get(&Layer::Types), Some(&Cell::Open));
    assert_eq!(row.cells.len(), Layer::ALL.len(), "every layer has a cell");
}

#[test]
fn a_registry_that_holds_together_counts_its_decisions_and_held_cells() {
    let read = rows(&page("swap", "`every_swap_keeps_its_tree`"));
    let ledger = gaps(&owned("swap"));
    let (Ok(read), Ok(ledger)) = (read, ledger) else {
        panic!("both read");
    };
    assert_eq!(
        check(&read, &ledger, &names(&["every_swap_keeps_its_tree"])),
        Ok((1, 1))
    );
}

#[test]
fn every_way_the_registry_and_the_tree_can_disagree_is_refused_by_name() {
    let (Ok(read), Ok(mut ledger)) = (
        rows(&page("swap", "`a_check_nobody_wrote`")),
        gaps(&owned("swap")),
    ) else {
        panic!("both read");
    };
    let refused = check(&read, &ledger, &BTreeSet::new());
    assert!(
        refused.as_ref().is_err_and(|refused| {
            refused.iter().any(|error| matches!(
            error,
            InvariantError::Unheld { name, layer: "oracle", .. } if name == "a_check_nobody_wrote"
        ))
        }),
        "a cell naming what the tree does not define holds nothing: {refused:?}"
    );

    let Ok(mut fewer) = gaps(&owned("swap")) else {
        panic!("the ledger reads");
    };
    fewer.retain(|gap| gap.layer != Layer::Plant);
    let refused = check(&read, &fewer, &names(&["a_check_nobody_wrote"]));
    assert!(
        refused.as_ref().is_err_and(|refused| refused
            .iter()
            .any(|error| matches!(error, InvariantError::Unowned { layer: "plant", .. }))),
        "an open layer nobody owns is refused: {refused:?}"
    );

    let Ok(stale) = gaps("swap oracle somebody\n") else {
        panic!("the line reads");
    };
    ledger.extend(stale);
    let refused = check(&read, &ledger, &names(&["a_check_nobody_wrote"]));
    assert!(
        refused
            .as_ref()
            .is_err_and(|refused| refused.iter().any(|error| matches!(
                error,
                InvariantError::Stale {
                    layer: "oracle",
                    ..
                }
            ))),
        "a hole the table has closed cannot stay listed: {refused:?}"
    );
}

#[test]
fn a_cell_that_is_neither_open_nor_names_and_a_ledger_line_out_of_shape_are_refused() {
    for oracle in ["a prose sentence", "`two words`", "``", "`a`,`b`"] {
        let read = rows(&page("swap", oracle));
        assert!(
            matches!(read, Err(InvariantError::Shape { .. })),
            "{oracle:?} is neither `none` nor names: {read:?}"
        );
    }
    for line in [
        "swap oracle",
        "swap proofs somebody",
        "swap oracle somebody else",
    ] {
        let read = gaps(line);
        assert!(
            matches!(read, Err(InvariantError::Shape { .. })),
            "{line:?} is not a ledger line: {read:?}"
        );
    }
    let twice = gaps("swap oracle somebody\nswap oracle somebody\n");
    assert!(
        matches!(twice, Err(InvariantError::Shape { .. })),
        "{twice:?}"
    );
}

#[test]
fn only_what_an_item_introducer_names_is_defined() {
    let found = defined(
        "fn laws() {}\npub struct Grouping;\nenum Binding { Or }\nconst LIMIT: u8 = 1;\n\
         macro_rules! value { () => {} }\nlet not_an_item = 1;\n",
    );
    assert_eq!(
        found,
        names(&["Binding", "Grouping", "LIMIT", "laws", "value"]),
        "a binding, a variant and a type named in a signature are not items the registry can \
         name"
    );
}

#[test]
fn this_repository_holds_its_own_registry() {
    let said = xtask::gates::invariants(&xtask::gates::workspace_root());
    assert!(
        said.as_ref()
            .is_ok_and(|line| line.starts_with("invariants: ")),
        "{said:?}"
    );
}

/// A registry page of `decision` holding its oracle, with `blind` said about it and its types held when `holds_types`.
fn row_page(decision: &str, holds_types: bool, blind: &str) -> String {
    let types = if holds_types { "`Typed`" } else { "none" };
    format!(
        "| Decision | Invariant | Types | Self-check | Oracle | Plant | Mutation | States | Blind |\n\
         | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n\
         | {decision} | what it promises | {types} | none | `an_oracle` | none | none | none | {blind} |\n"
    )
}

#[test]
fn a_named_oracle_says_what_it_cannot_see() {
    let Ok(read) = rows(&row_page("swap", false, "")) else {
        panic!("the page reads");
    };
    let Ok(ledger) =
        gaps("swap types a\nswap self-check a\nswap plant a\nswap mutation a\nswap states a\n")
    else {
        panic!("the ledger reads");
    };
    let refused = check(&read, &ledger, &names(&["an_oracle"]));
    assert!(
        refused.as_ref().is_err_and(|refused| refused
            .iter()
            .any(|error| matches!(error, InvariantError::Unblind { .. }))),
        "an oracle whose blind spot nobody wrote down is read as covering everything: {refused:?}"
    );
}

#[test]
fn a_layer_that_held_at_the_base_may_not_open_and_a_decision_may_only_leave_by_a_rename() {
    let parse = |text: &str| rows(text).unwrap_or_else(|error| panic!("{error}: {text}"));
    let base = parse(&row_page("swap", true, "unary operators"));
    assert!(
        regressions(&base, &parse(&row_page("swap", true, "unary operators"))).is_empty(),
        "nothing moved"
    );
    let reopened = regressions(&base, &parse(&row_page("swap", false, "unary operators")));
    assert!(
        matches!(
            reopened.as_slice(),
            [InvariantError::Reopened { layer: "types", .. }]
        ),
        "a layer that held may not open again: {reopened:?}"
    );
    let vanished = regressions(
        &base,
        &parse(&row_page("operator-swap", true, "unary operators")),
    );
    assert!(
        matches!(vanished.as_slice(), [InvariantError::Vanished { decision }] if decision == "swap"),
        "a decision that disappears takes what held it along, unless it says it was renamed: {vanished:?}"
    );
    let renamed = regressions(
        &base,
        &parse(&row_page(
            "operator-swap (was swap)",
            true,
            "unary operators",
        )),
    );
    assert!(
        renamed.is_empty(),
        "a marked rename carries the base row over: {renamed:?}"
    );
    let grown = regressions(&base, &{
        let mut head = parse(&row_page("swap", true, "unary operators"));
        head.extend(parse(&row_page("verdict", false, "the report it reads")));
        head
    });
    assert!(
        grown.is_empty(),
        "a decision the base did not have enters with what it owes: {grown:?}"
    );
}

/// Runs `git` in `root`, for a test's own scratch repository, seeing none of the user's Git configuration.
fn git(root: &std::path::Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
    let status = command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .expect("git runs");
    assert!(status.status.success(), "git {args:?}: {:?}", status.stderr);
}

/// A scratch repository whose `origin/main` holds `base` and whose `HEAD` holds `head`, each a list of files.
fn repository(base: &[(&str, &str)], head: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a scratch repository");
    let root = directory.path();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "ratchet-test"]);
    git(root, &["config", "user.email", "ratchet@example.invalid"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    let write = |files: &[(&str, &str)]| {
        for (path, text) in files {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().expect("a parent")).expect("its directory");
            std::fs::write(path, text).expect("the file");
        }
    };
    write(base);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    git(root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    write(head);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "head"]);
    directory
}

#[test]
fn a_bound_raised_in_the_change_it_bounds_is_refused_against_the_base() {
    let registry = row_page("swap", true, "unary operators");
    let files = |waivers: &str, seams: &str| {
        [
            ("docs/invariants.md", registry.clone()),
            (
                "xtask/waiver_ceiling.txt",
                format!("# the header\n{waivers}\n"),
            ),
            ("xtask/seam_ceiling.txt", format!("# the header\n{seams}\n")),
        ]
    };
    let as_files =
        |files: &[(&'static str, String)]| -> Vec<(&'static str, String)> { files.to_vec() };
    let base = as_files(&files("3", "7"));
    let base: Vec<(&str, &str)> = base
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();

    let raised = as_files(&files("4", "7"));
    let raised: Vec<(&str, &str)> = raised
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    let refused = xtask::gates::ratchets(repository(&base, &raised).path());
    assert!(
        refused.as_ref().is_err_and(|error| error
            .0
            .contains("xtask/waiver_ceiling.txt holds 4, and held 3 at the base")),
        "the ceiling and the ledger raised together in one change is the review the ceiling exists for: {refused:?}"
    );

    let lowered = as_files(&files("2", "7"));
    let lowered: Vec<(&str, &str)> = lowered
        .iter()
        .map(|(path, text)| (*path, text.as_str()))
        .collect();
    let passed = xtask::gates::ratchets(repository(&base, &lowered).path());
    assert!(
        passed
            .as_ref()
            .is_ok_and(|line| line.contains("where this change meets origin/main")),
        "a ledger that shrank passes, and the line names the base it compared with: {passed:?}"
    );

    let reopened = row_page("swap", false, "unary operators");
    let refused = xtask::gates::ratchets(
        repository(&base, &[("docs/invariants.md", reopened.as_str())]).path(),
    );
    assert!(
        refused
            .as_ref()
            .is_err_and(|error| error.0.contains("was held at types at the base")),
        "a registry layer that held at the base may not open: {refused:?}"
    );
}

#[test]
fn a_tree_with_no_base_to_compare_with_is_refused_rather_than_passed() {
    let directory = tempfile::tempdir().expect("a scratch directory");
    let refused = xtask::gates::ratchets(directory.path());
    assert!(
        refused
            .as_ref()
            .is_err_and(|error| error.0.contains("fetch origin/main")),
        "without the base, what may never grow cannot be held, and passing would be a guess: {refused:?}"
    );
}
