// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Every xtask error carries a code, every code it carries is documented, and every documented `XT` code exists.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::collections::BTreeSet;

use syn::visit::Visit as _;
use xtask::error::XtCode;

fn documented() -> BTreeSet<String> {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    text.lines()
        .filter_map(|line| {
            let cell = line.strip_prefix("| `")?;
            let (code, _) = cell.split_once('`')?;
            code.starts_with("XT").then(|| code.to_owned())
        })
        .collect()
}

#[test]
fn every_xtask_code_is_documented_and_every_documented_code_exists() {
    let declared: BTreeSet<String> = XtCode::ALL.iter().map(|c| c.code().to_owned()).collect();
    assert_eq!(
        declared,
        documented(),
        "docs/errors.md and xtask::error::XtCode disagree"
    );
}

#[test]
fn xtask_codes_are_unique_well_formed_and_sorted() {
    let codes: Vec<&str> = XtCode::ALL.iter().map(|c| c.code()).collect();
    let unique: BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(unique.len(), codes.len(), "duplicate codes: {codes:?}");
    let mut sorted = codes.clone();
    sorted.sort_unstable();
    assert_eq!(codes, sorted, "codes are listed in code order");
    for code in &codes {
        assert!(
            code.len() == 6
                && code.starts_with("XT")
                && code.chars().skip(2).all(|c| c.is_ascii_digit()),
            "malformed code {code}"
        );
    }
}

#[test]
fn every_documented_row_says_what_the_code_says() {
    let path = njutest_devkit::paths::workspace_root().join("docs/errors.md");
    let text = std::fs::read_to_string(&path).expect("docs/errors.md");
    for code in XtCode::ALL {
        let row = format!(
            "| `{}` | {} | {} |",
            code.code(),
            code.meaning(),
            code.remedy()
        );
        assert!(
            text.lines().any(|line| line == row),
            "docs/errors.md lacks {row}"
        );
    }
}

/// The error types of one file, by derive or by name, and the types it gives a code.
#[derive(Default)]
struct Declared {
    errors: BTreeSet<String>,
    coded: BTreeSet<String>,
}

impl<'ast> syn::visit::Visit<'ast> for Declared {
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if is_error(&item.ident, &item.attrs) {
            self.errors.insert(item.ident.to_string());
        }
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if is_error(&item.ident, &item.attrs) {
            self.errors.insert(item.ident.to_string());
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let codes = item.trait_.as_ref().is_some_and(|(trait_, _for)| {
            trait_
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Coded")
        });
        if codes
            && let syn::Type::Path(type_) = item.self_ty.as_ref()
            && let Some(name) = type_.path.segments.last()
        {
            self.coded.insert(name.ident.to_string());
        }
        syn::visit::visit_item_impl(self, item);
    }
}

/// Whether a type named `ident` with `attrs` is an error: it derives `Error`, or its name says it is one.
fn is_error(ident: &syn::Ident, attrs: &[syn::Attribute]) -> bool {
    ident.to_string().ends_with("Error")
        || attrs.iter().any(|attribute| {
            let syn::Meta::List(list) = &attribute.meta else {
                return false;
            };
            list.path.is_ident("derive")
                && list
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                    )
                    .expect("a derive list is a list of paths")
                    .iter()
                    .any(|path| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == "Error")
                    })
        })
}

/// Every error type among `sources` that the file declaring it gives no `impl Coded`, as `path: name`.
fn uncoded(sources: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in sources {
        let file = syn::parse_file(text).expect("an xtask source parses");
        let mut declared = Declared::default();
        declared.visit_file(&file);
        found.extend(
            declared
                .errors
                .difference(&declared.coded)
                .map(|name| format!("{path}: {name}")),
        );
    }
    found
}

/// Every source file of xtask's library, by its path from the workspace root.
fn xtask_sources() -> Vec<(String, String)> {
    let root = njutest_devkit::paths::workspace_root();
    let mut sources: Vec<(String, String)> = walkdir::WalkDir::new(root.join("xtask/src"))
        .sort_by_file_name()
        .into_iter()
        .map(|entry| entry.expect("xtask/src is walkable"))
        .filter(|entry| {
            entry
                .path()
                .extension()
                .is_some_and(|extension| extension == "rs")
        })
        .map(|entry| {
            let path = entry
                .path()
                .strip_prefix(&root)
                .expect("under the workspace root")
                .display()
                .to_string();
            let text = std::fs::read_to_string(entry.path()).expect("a source file reads");
            (path, text)
        })
        .collect();
    sources.sort();
    sources
}

#[test]
fn an_error_type_with_no_code_is_found_by_its_derive_or_its_name() {
    let specimen = vec![(
        "xtask/src/specimen.rs".to_owned(),
        "#[derive(Debug, thiserror::Error)]\n\
         pub enum PlantedError {\n    #[error(\"planted\")]\n    Planted,\n}\n\
         #[derive(Debug, thiserror::Error)]\n\
         #[error(\"contradicted\")]\n\
         pub struct Contradiction;\n\
         #[derive(Debug)]\n\
         pub(crate) enum NamedError {\n    Named,\n}\n\
         #[derive(Debug, thiserror::Error)]\n\
         pub enum CodedError {\n    #[error(\"coded\")]\n    Coded,\n}\n\
         impl crate::error::Coded for CodedError {\n    \
             fn code(&self) -> crate::error::XtCode {\n        \
                 crate::error::XtCode::GateRefused\n    }\n}\n\
         mod inner {\n    #[derive(Debug, thiserror::Error)]\n    \
             #[error(\"inner\")]\n    pub struct InnerError;\n}\n"
            .to_owned(),
    )];
    assert_eq!(
        uncoded(&specimen),
        vec![
            "xtask/src/specimen.rs: Contradiction",
            "xtask/src/specimen.rs: InnerError",
            "xtask/src/specimen.rs: NamedError",
            "xtask/src/specimen.rs: PlantedError",
        ],
        "an error is found by what it derives or what it is named, wherever in the file it is, \
         and only the one given a code is left out"
    );
}

#[test]
fn every_xtask_error_type_carries_a_code() {
    let sources = xtask_sources();
    assert!(
        sources.len() > 20,
        "the walk reads xtask's library: {}",
        sources.len()
    );
    let uncoded = uncoded(&sources);
    assert!(
        uncoded.is_empty(),
        "an xtask failure is read by its code, so every error type implements \
         `xtask::error::Coded` in the file that declares it, and the gate maps it with \
         `error.coded()`; these have none:\n{}",
        uncoded.join("\n")
    );
}
