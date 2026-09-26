// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The registry of critical decisions: every cell names what the tree defines, and every hole is one somebody owns.

use std::collections::BTreeSet;

use xtask::invariants::{Cell, InvariantError, Layer, check, defined, gaps, rows};

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
        check(&read, &ledger, &names(&["every_swap_keeps_its_tree"]), 5),
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
    let refused = check(&read, &ledger, &BTreeSet::new(), 5);
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
    let refused = check(&read, &fewer, &names(&["a_check_nobody_wrote"]), 5);
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
    let refused = check(&read, &ledger, &names(&["a_check_nobody_wrote"]), 6);
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

    let Ok(ledger) = gaps(&owned("swap")) else {
        panic!("the ledger reads");
    };
    let refused = check(&read, &ledger, &names(&["a_check_nobody_wrote"]), 4);
    assert!(
        refused.as_ref().is_err_and(|refused| refused
            .iter()
            .any(|error| matches!(error, InvariantError::Grown { count: 5, most: 4 }))),
        "the ledger may shrink and never grow: {refused:?}"
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
