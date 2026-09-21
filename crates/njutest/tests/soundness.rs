// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The soundness inventory: every place a crate steps outside what the compiler guarantees, counted from the source rather than guessed at.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use njutest::soundness::{Item, Kind, inventory, of_source};

fn package_directory(root: &std::path::Path) -> std::path::PathBuf {
    rust_mutants::canonical::canonical(root).expect("the package directory is in one spelling")
}

fn kinds(source: &str) -> Vec<Kind> {
    of_source("src/lib.rs", source)
        .expect("the source parses")
        .into_iter()
        .map(|item| item.kind)
        .collect()
}

#[test]
fn safe_rust_has_nothing_to_inventory() {
    assert!(
        kinds("pub fn f(a: i32) -> i32 { a + 1 }\n").is_empty(),
        "a crate the compiler already vouches for has no inventory"
    );
    assert!(kinds("").is_empty());
}

#[test]
fn every_place_the_compiler_stops_vouching_is_counted_once() {
    let source = "\
pub fn block() {
    unsafe { core::ptr::null::<u8>().read() };
}

pub unsafe fn function() {}

pub unsafe trait Marker {}

unsafe impl Marker for u8 {}

pub static mut COUNTER: u32 = 0;

unsafe extern \"C\" {
    pub fn getpid() -> i32;
}
";
    let found = of_source("src/lib.rs", source).expect("the source parses");
    let kinds: Vec<Kind> = found.iter().map(|item| item.kind).collect();
    assert!(kinds.contains(&Kind::Block), "{kinds:?}");
    assert!(kinds.contains(&Kind::Function), "{kinds:?}");
    assert!(kinds.contains(&Kind::Trait), "{kinds:?}");
    assert!(kinds.contains(&Kind::Implementation), "{kinds:?}");
    assert!(kinds.contains(&Kind::StaticMut), "{kinds:?}");
    assert!(kinds.contains(&Kind::ForeignBlock), "{kinds:?}");
    assert!(
        found
            .iter()
            .all(|item| item.line > 0 && !item.path.is_empty()),
        "every item says where it is: {found:?}"
    );
}

#[test]
fn an_unsafe_block_inside_a_function_body_is_found_however_deep_it_is() {
    let source = "\
pub fn outer() {
    let closure = || {
        if true {
            unsafe { core::ptr::null::<u8>().read() };
        }
    };
    closure();
}
";
    assert_eq!(kinds(source), [Kind::Block], "{source}");
}

#[test]
fn a_source_that_does_not_parse_says_so_rather_than_reading_as_safe() {
    assert!(
        of_source("src/lib.rs", "pub fn ( {").is_err(),
        "a file this release cannot read is not a file with no unsafe in it"
    );
}

#[test]
fn every_kind_has_a_wire_name_that_reads_back() {
    for kind in Kind::ALL {
        assert!(!kind.name().is_empty());
        assert_eq!(Kind::parse(kind.name()), Some(kind));
    }
    assert_eq!(Kind::parse("something-else"), None);
}

#[test]
fn an_inventory_of_a_tree_names_the_package_each_item_belongs_to() {
    let repo = njutest_devkit::repo::Repo::new();
    repo.package("demo")
        .lib("pub fn f() {\n    unsafe { core::ptr::null::<u8>().read() };\n}\n");
    repo.write("src/safe.rs", "pub fn g() {}\n");

    let taken = inventory(
        repo.root(),
        &[("demo".to_owned(), package_directory(repo.root()))],
    )
    .expect("the tree reads");
    assert_eq!(taken.items.len(), 1, "{taken:?}");
    let item: &Item = &taken.items[0];
    assert_eq!(item.package, "demo");
    assert_eq!(item.path, "src/lib.rs");
    assert_eq!(item.kind, Kind::Block);
    assert_eq!(taken.packages, ["demo"]);
    assert!(taken.unreadable.is_empty());
}

#[test]
fn a_file_the_inventory_cannot_read_is_named_rather_than_passed_over() {
    let repo = njutest_devkit::repo::Repo::new();
    repo.package("demo").lib("pub fn f() {}\n");
    repo.write("src/broken.rs", "pub fn ( {\n");

    let taken = inventory(
        repo.root(),
        &[("demo".to_owned(), package_directory(repo.root()))],
    )
    .expect("the tree reads");
    assert_eq!(
        taken.unreadable,
        ["src/broken.rs"],
        "a file that was not read is not a file with nothing in it"
    );
    assert!(taken.items.is_empty());
    assert!(
        taken.packages.is_empty(),
        "nothing was found, so no package holds anything"
    );
}
