// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The registry of critical decisions: every cell names what the tree defines, and every hole is one somebody owns.

#![expect(
    clippy::expect_used,
    reason = "a scratch repository that cannot be made leaves no base to test"
)]

use std::collections::BTreeSet;

use xtask::invariants::{
    Cell, Definition, InvariantError, Kind, Layer, Tree, check, definitions, gaps, module_of,
    regressions, rows,
};

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

/// A tree whose only definitions are tests named `list`, each in a test file of its own crate.
fn names(list: &[&str]) -> Tree {
    Tree {
        definitions: list
            .iter()
            .map(|name| Definition {
                path: vec!["app".to_owned(), "tests".to_owned(), (*name).to_owned()],
                kind: Kind::Test,
                file: "crates/app/tests/tests.rs".to_owned(),
                line: 1,
                production: false,
            })
            .collect(),
        reached: BTreeSet::new(),
        receipts: std::collections::BTreeMap::new(),
    }
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
    let refused = check(&read, &ledger, &names(&[]));
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
fn only_what_rust_defines_as_an_item_is_a_definition_with_its_module_path_and_kind() {
    let source = "//! fn in_a_doc() is no item\n\
         // fn in_a_comment() is none either\n\
         const SAID: &str = \"fn in_a_string() {}\";\n\
         pub struct Grouping;\n\
         enum Binding { Or }\n\
         fn laws() { let not_an_item = 1; }\n\
         impl Grouping { pub fn keeps(&self) -> bool { true } }\n\
         macro_rules! machine { () => { const fn step(n: u8) -> u8 { n } }; }\n\
         #[cfg(test)]\n\
         mod tests {\n\
             #[test]\n\
             fn a_law_holds() {}\n\
             proptest::proptest! { #[test] fn every_case_holds(n in 0..3_u8) { let _ = n; } }\n\
         }\n\
         #[cfg(kani)]\n\
         mod proofs { #[kani::proof] fn bounded() {} }\n";
    let found = definitions("crates/app/src/swap.rs", source, true).expect("the source parses");
    let described: Vec<(String, Kind, bool)> = found
        .iter()
        .map(|one| (one.qualified(), one.kind, one.production))
        .collect();
    let expected = [
        ("app::swap::SAID", Kind::Constant, true),
        ("app::swap::Grouping", Kind::Type, true),
        ("app::swap::Binding", Kind::Type, true),
        ("app::swap::laws", Kind::Function, true),
        ("app::swap::Grouping::keeps", Kind::Function, true),
        ("app::swap::machine", Kind::Macro, true),
        ("app::swap::step", Kind::Function, false),
        ("app::swap::tests", Kind::Module, false),
        ("app::swap::tests::a_law_holds", Kind::Test, false),
        ("app::swap::tests::every_case_holds", Kind::Property, false),
        ("app::swap::proofs", Kind::Module, true),
        ("app::swap::proofs::bounded", Kind::Harness, true),
    ];
    assert_eq!(
        described,
        expected
            .iter()
            .map(|(path, kind, production)| ((*path).to_owned(), *kind, *production))
            .collect::<Vec<_>>(),
        "a comment, a doc, a string, a binding and a variant are no items, a method stands under \
         its type, a macro's items under its module, and only a production build's items are \
         production"
    );
    assert_eq!(
        module_of("crates/rust-mutants/src/runner/mod.rs"),
        ["rust_mutants", "runner"]
    );
    assert_eq!(
        module_of("xtask/tests/support/asked.rs"),
        ["xtask", "tests", "support", "asked"]
    );
    assert_eq!(module_of("crates/njutest/src/main.rs"), ["njutest"]);
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
    let mut command = xtask::repository::git(root);
    for (name, _) in std::env::vars_os() {
        if name.as_encoded_bytes().starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
    let status = command
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
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

/// The registry row the planted trees below are held to, and the ledger owning what it leaves open.
const PLANTED_ROW: (&str, &str) = (
    "| Decision | Invariant | Types | Self-check | Oracle | Plant | Mutation | States | Blind |\n\
     | --- | --- | --- | --- | --- | --- | --- | --- | --- |\n\
     | swap | a swap writes the file's own tree | `Grouping` | `keeps` | `a_swap_is_its_tree` | none | none | none | unary operators |\n",
    "swap plant somebody\nswap mutation somebody\nswap states somebody\n",
);

/// The files of a crate `app` whose swap module holds the planted row honestly.
const HONEST: [(&str, &str); 3] = [
    (
        "crates/app/src/lib.rs",
        "pub mod swap;\n\npub fn run() -> bool {\n    swap::keeps(&swap::Grouping)\n}\n",
    ),
    (
        "crates/app/src/swap.rs",
        "pub struct Grouping;\n\npub fn keeps(_grouping: &Grouping) -> bool {\n    true\n}\n",
    ),
    (
        "crates/app/tests/swap.rs",
        "#[test]\nfn a_swap_is_its_tree() {\n    assert!(app::swap::keeps(&app::swap::Grouping));\n}\n",
    ),
];

/// A scratch repository of one workspace crate `app` holding `registry`, `ledger`, and `sources`, each replacing a file of [`HONEST`] at its path or adding one.
fn planted(registry: &str, ledger: &str, sources: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("a scratch repository");
    let root = directory.path();
    git(root, &["init", "-q"]);
    let mut files: std::collections::BTreeMap<&str, String> = [
        (
            "Cargo.toml",
            "[workspace]\nmembers = [\"crates/app\"]\nresolver = \"2\"\n".to_owned(),
        ),
        (
            "crates/app/Cargo.toml",
            "[package]\nname = \"app\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n\
             [package.metadata.njutest]\nsurface = \"incidental\"\n"
                .to_owned(),
        ),
        ("docs/invariants.md", registry.to_owned()),
        ("xtask/invariant_gaps.txt", ledger.to_owned()),
    ]
    .into_iter()
    .collect();
    for (path, text) in HONEST.iter().chain(sources) {
        files.insert(path, (*text).to_owned());
    }
    for (path, text) in files {
        let path = root.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("its directory");
        std::fs::write(path, text).expect("the file");
    }
    directory
}

/// What the invariants gate says of a planted tree, refused in the words it refuses in.
fn held(
    registry: &str,
    ledger: &str,
    sources: &[(&str, &str)],
) -> Result<String, xtask::gates::GateError> {
    let tree = planted(registry, ledger, sources);
    xtask::gates::invariants(&canonical(&tree))
}

/// The path of `tree` with every link resolved, which is how cargo names the crates in it.
fn canonical(tree: &tempfile::TempDir) -> std::path::PathBuf {
    tree.path()
        .canonicalize()
        .expect("the scratch repository's own path")
}

#[test]
fn a_planted_row_the_tree_holds_honestly_passes() {
    let (registry, ledger) = PLANTED_ROW;
    let said = held(registry, ledger, &[]);
    assert!(
        said.as_ref()
            .is_ok_and(|line| line.contains("1 critical decisions")),
        "each cell names one definition of the kind its layer holds: {said:?}"
    );
}

#[test]
fn a_cell_naming_a_word_in_a_comment_or_a_string_holds_nothing() {
    let (registry, ledger) = PLANTED_ROW;
    for (shape, text) in [
        (
            "a doc comment",
            "//! Once held by fn a_swap_is_its_tree, which is gone.\n",
        ),
        (
            "a line comment",
            "// fn a_swap_is_its_tree() was here\npub const KEPT: u8 = 1;\n",
        ),
        (
            "a string",
            "pub const SAID: &str = \"fn a_swap_is_its_tree() {}\";\n",
        ),
    ] {
        let said = held(registry, ledger, &[("crates/app/tests/swap.rs", text)]);
        assert!(
            said.as_ref().is_err_and(|refused| refused
                .0
                .contains("the tree defines nothing by that name")
                && refused.0.contains("`a_swap_is_its_tree`")),
            "a word in {shape} is no definition, and a cell naming it names nothing: {said:?}"
        );
    }
}

#[test]
fn a_bare_name_two_files_define_is_refused_naming_both_and_its_qualified_form_holds() {
    let (registry, ledger) = PLANTED_ROW;
    let twice = [
        (
            "crates/app/src/lib.rs",
            "pub mod other;\npub mod swap;\n\npub fn run() -> bool {\n    swap::keeps(&swap::Grouping) && other::keeps()\n}\n",
        ),
        (
            "crates/app/src/other.rs",
            "pub fn keeps() -> bool {\n    false\n}\n",
        ),
    ];
    let said = held(registry, ledger, &twice);
    assert!(
        said.as_ref()
            .is_err_and(|refused| refused.0.contains("`keeps`")
                && refused.0.contains("app::swap::keeps")
                && refused.0.contains("app::other::keeps")),
        "a name two files define says which one only when it is written qualified: {said:?}"
    );
    let qualified = registry.replace("`keeps`", "`swap::keeps`");
    let said = held(&qualified, ledger, &twice);
    assert!(
        said.is_ok(),
        "a qualified name resolves to the one definition it names: {said:?}"
    );
}

#[test]
fn a_cell_naming_a_definition_of_another_kind_than_its_layer_holds_is_refused() {
    let (registry, ledger) = PLANTED_ROW;
    let constant = [(
        "crates/app/src/swap.rs",
        "pub struct Grouping;\n\npub const ROTTEN: u8 = 0;\n\npub fn keeps(_grouping: &Grouping) -> bool {\n    true\n}\n",
    )];
    let planted_by_a_constant = registry.replace(
        "| none | none | none | unary",
        "| `ROTTEN` | none | none | unary",
    );
    let ledger_without_plant = ledger.replace("swap plant somebody\n", "");
    for (shape, row, sources, ledger) in [
        (
            "a constant held up as a plant",
            planted_by_a_constant.as_str(),
            &constant[..],
            ledger_without_plant.as_str(),
        ),
        (
            "a type held up as an oracle",
            &registry.replace("`a_swap_is_its_tree`", "`Grouping`"),
            &[][..],
            ledger,
        ),
        (
            "a test held up as a self-check",
            &registry.replace("| `keeps` |", "| `a_swap_is_its_tree` |"),
            &[][..],
            ledger,
        ),
    ] {
        let said = held(row, ledger, sources);
        assert!(
            said.as_ref()
                .is_err_and(|refused| refused.0.contains("and the ")
                    && refused.0.contains(" layer is held by ")),
            "{shape} holds nothing at that layer: {said:?}"
        );
    }
    let only_tested = [
        (
            "crates/app/src/swap.rs",
            "pub struct Grouping;\n\npub fn keeps(_grouping: &Grouping) -> bool {\n    true\n}\n\npub fn verified() -> bool {\n    true\n}\n",
        ),
        (
            "crates/app/tests/swap.rs",
            "#[test]\nfn a_swap_is_its_tree() {\n    assert!(app::swap::keeps(&app::swap::Grouping) && app::swap::verified());\n}\n",
        ),
    ];
    let said = held(
        &registry.replace("| `keeps` |", "| `verified` |"),
        ledger,
        &only_tested,
    );
    assert!(
        said.as_ref()
            .is_err_and(|refused| refused.0.contains("`verified`")
                && refused.0.contains("nothing that ships")),
        "a self-check only a test calls checks nothing a run does: {said:?}"
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
