// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A 128-bit integer in a type serde writes, which no JSON reader of this repository reads back exactly.

use std::collections::BTreeSet;

use syn::visit::Visit;

use super::{Finding, Kind};

/// Why a 128-bit integer in a serialized type is refused, in the words a person needs to fix it.
pub(super) const REMEDY: &str = "hold it as `rust_mutants::wide::Wide`, which a record spells as \
    its decimal digits in a string. serde_json writes a 128-bit integer as the number it is, and \
    a reader that goes through `serde_json::Value`, as every `strictjson` module does, holds a \
    number past 64 bits only as a float, so the record cannot be read back: the engine's \
    toolchain stamp held a Windows file identity, which ReFS makes 128 bits wide, and no \
    toolchain identity or observation it published was ever read back on that filesystem. A \
    value that happens to fit 64 bits reads back and hides the defect from every run where it \
    does, so the type is refused rather than the value: a field of a type that derives \
    `Serialize` or `Deserialize`, directly or under `cfg_attr`, whose type names `u128`, `i128`, \
    `NonZeroU128`, `NonZeroI128`, or an alias the same file declares for one";

/// The integer types wider than the 64 bits a JSON number reads back exactly within.
const WIDE: [&str; 4] = ["u128", "i128", "NonZeroU128", "NonZeroI128"];

/// Every place `parsed` names a 128-bit integer in a field of a type serde reads or writes, each at its line.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut survey = Survey {
        wide: wide_aliases(parsed),
        lines: Vec::new(),
    };
    survey.visit_file(parsed);
    survey
        .lines
        .into_iter()
        .map(|line| Finding {
            kind: Kind::WideRecordInteger,
            file: file.to_owned(),
            line,
        })
        .collect()
}

/// The names that stand for a 128-bit integer in `parsed`: the integers themselves and every alias the file declares for a type naming one, however deep the aliases chain.
fn wide_aliases(parsed: &syn::File) -> BTreeSet<String> {
    let mut aliases = Aliases::default();
    aliases.visit_file(parsed);
    let mut wide: BTreeSet<String> = WIDE.iter().map(|name| (*name).to_owned()).collect();
    loop {
        let before = wide.len();
        for (name, ty) in &aliases.declared {
            if !Mentions::of(ty, &wide).is_empty() {
                wide.insert(name.clone());
            }
        }
        if wide.len() == before {
            return wide;
        }
    }
}

/// Every type alias a file declares, by name.
#[derive(Default)]
struct Aliases {
    declared: Vec<(String, syn::Type)>,
}

impl<'ast> Visit<'ast> for Aliases {
    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        self.declared
            .push((item.ident.to_string(), (*item.ty).clone()));
        syn::visit::visit_item_type(self, item);
    }
}

/// The lines at which one type names a wide integer.
struct Mentions<'names> {
    wide: &'names BTreeSet<String>,
    lines: Vec<usize>,
}

impl<'names> Mentions<'names> {
    fn of(ty: &syn::Type, wide: &'names BTreeSet<String>) -> Vec<usize> {
        let mut mentions = Self {
            wide,
            lines: Vec::new(),
        };
        mentions.visit_type(ty);
        mentions.lines
    }
}

impl<'ast> Visit<'ast> for Mentions<'_> {
    fn visit_path_segment(&mut self, segment: &'ast syn::PathSegment) {
        if self.wide.contains(&segment.ident.to_string()) {
            self.lines.push(segment.ident.span().start().line);
        }
        syn::visit::visit_path_segment(self, segment);
    }
}

/// The lines of one file at which a serialized type names a wide integer.
struct Survey {
    wide: BTreeSet<String>,
    lines: Vec<usize>,
}

/// The last name of `path`, as a derive is spelled however it was imported.
fn last(path: &syn::Path) -> Option<String> {
    path.segments
        .last()
        .map(|segment| segment.ident.to_string())
}

/// Whether `meta` derives serde's `Serialize` or `Deserialize`, directly or under `cfg_attr`.
fn derives_serde(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| {
                paths.iter().any(|path| {
                    last(path).is_some_and(|name| name == "Serialize" || name == "Deserialize")
                })
            });
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| nested.iter().skip(1).any(derives_serde))
}

impl Survey {
    fn serialized(attrs: &[syn::Attribute]) -> bool {
        attrs.iter().any(|attribute| derives_serde(&attribute.meta))
    }

    fn fields(&mut self, fields: &syn::Fields) {
        for field in fields {
            self.lines.extend(Mentions::of(&field.ty, &self.wide));
        }
    }
}

impl<'ast> Visit<'ast> for Survey {
    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if Self::serialized(&item.attrs) {
            self.fields(&item.fields);
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if Self::serialized(&item.attrs) {
            for variant in &item.variants {
                self.fields(&variant.fields);
            }
        }
        syn::visit::visit_item_enum(self, item);
    }
}
