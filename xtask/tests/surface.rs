// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Crate-surface declarations make the premise of the dead-code proof explicit.

use std::path::{Path, PathBuf};

use njutest_devkit::result::{ResultState, result_state};
use xtask::surface::{Harness, Package, check, harness_product, has_public_root};

fn library(name: &str) -> PathBuf {
    PathBuf::from(format!("/workspace/crates/{name}/src/lib.rs"))
}

fn harness(name: &str) -> Harness {
    Harness {
        name: format!("surface-{name}"),
        product: Some(library(name)),
        public_root: false,
    }
}

fn parsed_product(source: &str, harness: &Path) -> Option<PathBuf> {
    let parsed = harness_product(source, harness);
    assert_eq!(
        result_state(&parsed),
        ResultState::Returned,
        "the literal compiler surface did not parse: {parsed:?}"
    );
    match parsed {
        Ok(product) => product,
        Err(_already_reported) => None,
    }
}

fn package(name: &str, declared: Option<&str>, publishable: bool) -> Package {
    Package {
        name: name.to_owned(),
        declared: declared.map(ToOwned::to_owned),
        publishable,
        library: true,
        library_path: Some(library(name)),
        binary: name == "runner",
    }
}

#[test]
fn every_incidental_crate_and_only_one_is_compiled_as_a_private_surface() {
    let packages = [
        package("api", Some("public"), true),
        package("runner", Some("incidental"), true),
        package("fixtures", Some("test-support"), false),
    ];
    assert!(check(&packages, &[harness("runner")]).is_empty());

    assert_eq!(
        check(&packages, &[]),
        ["runner has incidental visibility but no private compiler surface"]
    );
    assert_eq!(
        check(&packages, &[harness("api"), harness("runner")]),
        ["the private compiler surface names api, but that crate is not incidental"]
    );
}

#[test]
fn an_unmade_or_misspelled_decision_is_not_inferred() {
    let packages = [
        package("absent", None, false),
        package("invented", Some("private-ish"), false),
    ];
    assert_eq!(
        check(&packages, &[]),
        [
            "absent has no [package.metadata.njutest] surface declaration",
            "invented declares unknown surface \"private-ish\"; expected public, incidental, or test-support",
        ]
    );
}

#[test]
fn declarations_that_contradict_cargo_are_refused() {
    let mut public_without_library = package("api", Some("public"), true);
    public_without_library.library = false;
    let mut incidental_without_binary = package("runner", Some("incidental"), true);
    incidental_without_binary.binary = false;
    let unpublishable_public = package("hidden-api", Some("public"), false);
    let publishable_test_support = package("fixtures", Some("test-support"), true);
    assert_eq!(
        check(
            &[
                public_without_library,
                unpublishable_public,
                incidental_without_binary,
                publishable_test_support,
            ],
            &[harness("runner")],
        ),
        [
            "api calls its surface public but has no library or proc-macro target",
            "hidden-api calls its surface public but Cargo forbids publishing it",
            "runner calls its surface incidental but does not have both a library and a binary target",
            "fixtures calls itself test-support but Cargo still permits publishing it",
        ]
    );
}

#[test]
fn the_harness_set_is_the_compiler_surface_binaries_not_a_second_ledger() {
    let packages = [package("runner", Some("incidental"), true)];
    assert_eq!(
        check(
            &packages,
            &[Harness {
                name: "runner".to_owned(),
                product: None,
                public_root: false,
            }]
        ),
        [
            "compiler-surfaces binary \"runner\" is not named surface-<crate>",
            "runner has incidental visibility but no private compiler surface",
        ]
    );
    assert_eq!(
        check(&packages, &[harness("runner"), harness("runner")]),
        ["compiler-surfaces has more than one binary for runner"]
    );
}

#[test]
fn a_harness_name_cannot_stand_in_for_including_the_product() {
    let packages = [package("runner", Some("incidental"), true)];
    assert_eq!(
        check(
            &packages,
            &[Harness {
                name: "surface-runner".to_owned(),
                product: None,
                public_root: false,
            }]
        ),
        [
            "compiler-surfaces binary \"surface-runner\" does not privately include runner's actual library target as mod product",
            "runner has incidental visibility but no private compiler surface",
        ]
    );
    assert_eq!(
        parsed_product(
            "#[path = \"../../../crates/runner/src/lib.rs\"]\nmod product;\nfn main() {}\n",
            Path::new("/workspace/compiler-surfaces/src/bin/runner.rs"),
        ),
        Some(library("runner"))
    );
    assert_eq!(
        parsed_product(
            "#[cfg(any())]\n#[path = \"../../../crates/runner/src/lib.rs\"]\nmod product;\nfn main() {}\n",
            Path::new("/workspace/compiler-surfaces/src/bin/runner.rs"),
        ),
        None,
        "a conditionally absent product module proves no surface"
    );
}

#[test]
fn a_public_product_module_or_wrapper_cannot_turn_dead_code_reachable() {
    let source = Path::new("/workspace/compiler-surfaces/src/bin/runner.rs");
    assert_eq!(
        parsed_product(
            "#[path = \"../../../crates/runner/src/lib.rs\"]\npub mod product;\nfn main() {}\n",
            source,
        ),
        None,
        "the included product itself must be private"
    );
    for exported in [
        "pub use product::*;",
        "pub use product::Thing;",
        "pub use product::Thing as Renamed;",
        "pub use product::{Thing, Other};",
        "pub fn wrapper() {}",
        "#[macro_export] macro_rules! wrapper { () => {} }",
        "macro_rules! expose { () => { pub use product::*; } } expose!();",
        "macro_rules! expose { () => { pub fn wrapper() {} } } expose!();",
        "include!(\"generated.rs\");",
    ] {
        let text = format!(
            "#[path = \"../../../crates/runner/src/lib.rs\"]\nmod product;\n{exported}\nfn main() {{}}\n"
        );
        assert!(has_public_root(&text), "{exported}");
    }
    assert!(
        !has_public_root(
            "#[path = \"../../../crates/runner/src/lib.rs\"]\nmod product;\npub(crate) use product::Thing;\nfn main() {}\n"
        ),
        "crate visibility remains subject to the binary crate's dead-code analysis"
    );
}

#[test]
fn a_same_named_directory_elsewhere_is_not_the_product_target() {
    let packages = [package("runner", Some("incidental"), true)];
    let malicious = Harness {
        name: "surface-runner".to_owned(),
        product: parsed_product(
            "#[path = \"../../../../tmp/runner/src/lib.rs\"]\nmod product;\nfn main() {}\n",
            Path::new("/workspace/compiler-surfaces/src/bin/runner.rs"),
        ),
        public_root: false,
    };
    assert_eq!(
        check(&packages, &[malicious]),
        [
            "compiler-surfaces binary \"surface-runner\" does not privately include runner's actual library target as mod product",
            "runner has incidental visibility but no private compiler surface",
        ]
    );
}

#[test]
fn a_malformed_harness_fails_closed() {
    let parsed = harness_product(
        "mod product {",
        Path::new("/workspace/compiler-surfaces/src/bin/runner.rs"),
    );
    assert_eq!(
        result_state(&parsed),
        ResultState::Refused,
        "an unparsed root claimed a compiler surface: {parsed:?}"
    );
}
