// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The declared meaning of each workspace crate's Rust visibility.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

use syn::visit::Visit;

/// Whether a crate's Rust-visible names are an API, an implementation seam, or test apparatus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// Other repositories are intended to compile against this crate.
    Public,
    /// Visibility joins this package's binary and integration tests; it is not a promised API.
    Incidental,
    /// The whole crate is development apparatus and is never published.
    TestSupport,
}

impl Surface {
    /// Reads the exact vocabulary accepted in `[package.metadata.njutest]`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "public" => Some(Self::Public),
            "incidental" => Some(Self::Incidental),
            "test-support" => Some(Self::TestSupport),
            _ => None,
        }
    }
}

/// The facts Cargo supplies about one workspace package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Package {
    /// Cargo's package name.
    pub name: String,
    /// The literal metadata value, absent when nobody made the decision.
    pub declared: Option<String>,
    /// Whether Cargo is allowed to publish it to any registry.
    pub publishable: bool,
    /// Whether the package has a library or proc-macro target.
    pub library: bool,
    /// The canonical source of the library target, when there is one.
    pub library_path: Option<PathBuf>,
    /// Whether the package has a binary target.
    pub binary: bool,
}

/// One compiler-surface binary and the product library it actually includes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Harness {
    /// Cargo's binary target name.
    pub name: String,
    /// The canonical source named by `#[path = "…"] mod product;`.
    pub product: Option<PathBuf>,
    /// Whether the binary root exports anything outside its crate.
    pub public_root: bool,
}

/// The product crate included by a compiler-surface source.
///
/// A target name is only a label. The dead-code proof exists only when the
/// target's actual syntax includes the product library as a private module.
///
/// # Errors
///
/// Returns the parser error when the harness is not valid Rust. Invalid syntax
/// cannot prove that the product has a private, dead-code-checked surface.
pub fn harness_product(source: &str, harness_source: &Path) -> syn::Result<Option<PathBuf>> {
    let parsed = syn::parse_file(source)?;
    Ok(parsed.items.iter().find_map(|item| {
        let syn::Item::Mod(module) = item else {
            return None;
        };
        if module.ident != "product"
            || !matches!(module.vis, syn::Visibility::Inherited)
            || module.attrs.iter().any(|attribute| {
                attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
            })
        {
            return None;
        }
        let path = module.attrs.iter().find_map(|attribute| {
            let syn::Meta::NameValue(value) = &attribute.meta else {
                return None;
            };
            if !value.path.is_ident("path") {
                return None;
            }
            let syn::Expr::Lit(literal) = &value.value else {
                return None;
            };
            let syn::Lit::Str(path) = &literal.lit else {
                return None;
            };
            Some(path.value())
        })?;
        Some(lexical(&harness_source.parent()?.join(Path::new(&path))))
    }))
}

/// Whether the harness root makes any item externally reachable.
///
/// A binary-root `pub` item is exempt from `dead_code`; allowing even a
/// wrapper would therefore let the harness make arbitrary product code look
/// used. Root macros are rejected outright: an unexpanded syntax tree cannot
/// prove that a macro or `include!` does not manufacture such a wrapper.
#[must_use]
pub fn has_public_root(source: &str) -> bool {
    let Ok(parsed) = syn::parse_file(source) else {
        return true;
    };
    if parsed
        .items
        .iter()
        .any(|item| matches!(item, syn::Item::Macro(_)))
    {
        return true;
    }
    if parsed.items.iter().any(|item| {
        item_visibility(item)
            .is_some_and(|visibility| matches!(visibility, syn::Visibility::Public(_)))
    }) {
        return true;
    }
    let mut macros = ExportedMacro { found: false };
    macros.visit_file(&parsed);
    macros.found
}

const fn item_visibility(item: &syn::Item) -> Option<&syn::Visibility> {
    match item {
        syn::Item::Const(item) => Some(&item.vis),
        syn::Item::Enum(item) => Some(&item.vis),
        syn::Item::ExternCrate(item) => Some(&item.vis),
        syn::Item::Fn(item) => Some(&item.vis),
        syn::Item::Mod(item) => Some(&item.vis),
        syn::Item::Static(item) => Some(&item.vis),
        syn::Item::Struct(item) => Some(&item.vis),
        syn::Item::Trait(item) => Some(&item.vis),
        syn::Item::TraitAlias(item) => Some(&item.vis),
        syn::Item::Type(item) => Some(&item.vis),
        syn::Item::Union(item) => Some(&item.vis),
        syn::Item::Use(item) => Some(&item.vis),
        _ => None,
    }
}

struct ExportedMacro {
    found: bool,
}

impl Visit<'_> for ExportedMacro {
    fn visit_item_macro(&mut self, item: &syn::ItemMacro) {
        self.found |= item
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("macro_export"));
        syn::visit::visit_item_macro(self, item);
    }
}

fn lexical(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push(component.as_os_str());
                }
            }
        }
    }
    normalized
}

/// Every contradiction between the declarations, Cargo targets, and compiler harness binaries.
#[must_use]
pub fn check(packages: &[Package], harness_targets: &[Harness]) -> Vec<String> {
    let (mut problems, harnessed) = check_harnesses(packages, harness_targets);
    let (package_problems, incidental) = check_packages(packages);
    problems.extend(package_problems);
    for missing in incidental.difference(&harnessed) {
        problems.push(format!(
            "{missing} has incidental visibility but no private compiler surface"
        ));
    }
    for extra in harnessed.difference(&incidental) {
        problems.push(format!(
            "the private compiler surface names {extra}, but that crate is not incidental"
        ));
    }
    problems
}

fn check_harnesses(
    packages: &[Package],
    harness_targets: &[Harness],
) -> (Vec<String>, BTreeSet<String>) {
    let mut problems = Vec::new();
    let mut harnessed = BTreeSet::new();
    for target in harness_targets {
        let Some(name) = target
            .name
            .strip_prefix("surface-")
            .filter(|name| !name.is_empty())
        else {
            problems.push(format!(
                "compiler-surfaces binary {:?} is not named surface-<crate>",
                target.name
            ));
            continue;
        };
        if target.public_root {
            problems.push(format!(
                "compiler-surfaces binary {:?} has a public root item, which is exempt from dead_code",
                target.name
            ));
            continue;
        }
        let expected = packages
            .iter()
            .find(|package| package.name == name)
            .and_then(|package| package.library_path.as_ref());
        if target.product.as_ref() != expected {
            problems.push(format!(
                "compiler-surfaces binary {:?} does not privately include {name}'s actual library target as mod product",
                target.name
            ));
            continue;
        }
        if !harnessed.insert(name.to_owned()) {
            problems.push(format!(
                "compiler-surfaces has more than one binary for {name}"
            ));
        }
    }
    (problems, harnessed)
}

fn check_packages(packages: &[Package]) -> (Vec<String>, BTreeSet<String>) {
    let mut problems = Vec::new();
    let mut incidental = BTreeSet::new();
    for package in packages {
        let Some(declared) = package.declared.as_deref() else {
            problems.push(format!(
                "{} has no [package.metadata.njutest] surface declaration",
                package.name
            ));
            continue;
        };
        let Some(surface) = Surface::parse(declared) else {
            problems.push(format!(
                "{} declares unknown surface {declared:?}; expected public, incidental, or test-support",
                package.name
            ));
            continue;
        };
        match surface {
            Surface::Public if !package.library => problems.push(format!(
                "{} calls its surface public but has no library or proc-macro target",
                package.name
            )),
            Surface::Public if !package.publishable => problems.push(format!(
                "{} calls its surface public but Cargo forbids publishing it",
                package.name
            )),
            Surface::Incidental => {
                incidental.insert(package.name.clone());
                if !package.library || !package.binary {
                    problems.push(format!(
                        "{} calls its surface incidental but does not have both a library and a binary target",
                        package.name
                    ));
                }
            }
            Surface::TestSupport if package.publishable => problems.push(format!(
                "{} calls itself test-support but Cargo still permits publishing it",
                package.name
            )),
            Surface::Public | Surface::TestSupport => {}
        }
    }
    (problems, incidental)
}
