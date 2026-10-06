// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::path::Path;

use super::archive;
use crate::cargo::{COMPILATIONS, names_products, products_name};

const KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const OBSERVATION: &str = "fedcba9876543210fedcba9876543210";

#[test]
fn a_products_directory_spells_short_prefixes_of_the_keys_its_record_keeps_whole() {
    let named = products_name(KEY, OBSERVATION).expect("both are keys");
    assert_eq!(named, "0123456789abcdef.fedcba9876543210.products");
    assert!(names_products(&named));
    assert!(
        !names_products(&format!("{KEY}.{OBSERVATION}.products")),
        "a name that spells whole keys is not one the engine makes"
    );
    assert!(!names_products("0123456789abcdef.fedcba9876543210"));
    assert!(!names_products(
        "0123456789ABCDEF.fedcba9876543210.products"
    ));
    let refused = products_name("not a key", OBSERVATION).expect_err("only keys are named");
    assert!(
        refused.to_string().contains("not a lowercase hex key"),
        "{refused}"
    );
}

#[test]
fn an_execution_origin_is_only_a_products_directory_the_engine_names() {
    let target = Path::new("target");
    let products = target
        .join(COMPILATIONS)
        .join(products_name(KEY, OBSERVATION).expect("both are keys"));
    let executable = products.join("debug").join("deps").join("tests");
    assert_eq!(
        archive(&executable, target),
        Some(products.as_path()),
        "an executable the build cache keeps names the products directory it came from"
    );
    let whole = target
        .join(COMPILATIONS)
        .join(format!("{KEY}.{OBSERVATION}.products"))
        .join("tests");
    assert_eq!(
        archive(&whole, target),
        None,
        "a directory that spells whole keys is no products directory the engine made"
    );
    assert_eq!(
        archive(&target.join("debug").join("deps").join("tests"), target),
        None
    );
}
