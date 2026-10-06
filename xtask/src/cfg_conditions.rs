// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

#![expect(
    clippy::redundant_pub_crate,
    reason = "a private module shares crate-only CFG decisions between two gates"
)]

use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum CfgTruth {
    Always,
    Never,
    Variable,
}

#[derive(Clone, Copy)]
pub(crate) enum CfgWorld {
    Any,
    Production,
}

struct CfgConstraint {
    guards: Vec<syn::Meta>,
    requirement: syn::Meta,
}

pub(crate) struct CfgScope {
    world: CfgWorld,
    conditions: Vec<CfgConstraint>,
}

impl CfgScope {
    pub(crate) const fn new(world: CfgWorld) -> Self {
        Self {
            world,
            conditions: Vec::new(),
        }
    }

    pub(crate) const fn mark(&self) -> usize {
        self.conditions.len()
    }

    pub(crate) fn push(&mut self, attribute: &syn::Attribute) -> bool {
        let previous = self.conditions.len();
        cfg_constraints(&attribute.meta, &[], &mut self.conditions);
        self.conditions.len() > previous
    }

    pub(crate) fn truncate(&mut self, mark: usize) {
        self.conditions.truncate(mark);
    }

    pub(crate) fn possible(&self) -> bool {
        let mut atoms = BTreeSet::new();
        for condition in &self.conditions {
            for guard in &condition.guards {
                cfg_atoms(guard, &mut atoms, self.world);
            }
            cfg_atoms(&condition.requirement, &mut atoms, self.world);
        }
        let atoms: Vec<String> = atoms.into_iter().collect();
        self.assignment_possible(&atoms, 0, &mut BTreeMap::new())
    }

    fn assignment_possible(
        &self,
        atoms: &[String],
        next: usize,
        values: &mut BTreeMap<String, bool>,
    ) -> bool {
        let mut unresolved = false;
        for condition in &self.conditions {
            match cfg_constraint_truth(condition, values, self.world) {
                CfgTruth::Never => return false,
                CfgTruth::Variable => unresolved = true,
                CfgTruth::Always => {}
            }
        }
        if !unresolved {
            return true;
        }
        let Some(atom) = atoms.get(next) else {
            return false;
        };
        values.insert(atom.clone(), true);
        if self.assignment_possible(atoms, next.saturating_add(1), values) {
            values.remove(atom);
            return true;
        }
        values.insert(atom.clone(), false);
        let possible = self.assignment_possible(atoms, next.saturating_add(1), values);
        values.remove(atom);
        possible
    }
}

pub(crate) fn cfg_constant(meta: &syn::Meta) -> CfgTruth {
    cfg_truth(meta, &BTreeMap::new(), CfgWorld::Any)
}

fn fixed_truth(meta: &syn::Meta, world: CfgWorld) -> Option<CfgTruth> {
    if !matches!(world, CfgWorld::Production) {
        return None;
    }
    match meta {
        syn::Meta::Path(path) if path.is_ident("test") => Some(CfgTruth::Never),
        syn::Meta::NameValue(value) if value.path.is_ident("feature") => match &value.value {
            syn::Expr::Lit(literal) if matches!(&literal.lit, syn::Lit::Str(feature) if feature.value() == "testkit") => {
                Some(CfgTruth::Never)
            }
            _ => None,
        },
        _ => None,
    }
}

fn cfg_atom(meta: &syn::Meta) -> String {
    let path = meta
        .path()
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::");
    match meta {
        syn::Meta::Path(_) => format!("path:{path}"),
        syn::Meta::NameValue(value) => {
            let value = match &value.value {
                syn::Expr::Lit(literal) => match &literal.lit {
                    syn::Lit::Str(value) => value.value(),
                    _ => format!("{:?}", literal.lit),
                },
                other => format!("{other:?}"),
            };
            format!("value:{path}={value:?}")
        }
        syn::Meta::List(list) => format!("list:{path}({})", list.tokens),
    }
}

fn cfg_truth(meta: &syn::Meta, values: &BTreeMap<String, bool>, world: CfgWorld) -> CfgTruth {
    let syn::Meta::List(list) = meta else {
        return cfg_atomic_truth(meta, values, world);
    };
    if !list.path.is_ident("all") && !list.path.is_ident("any") && !list.path.is_ident("not") {
        return cfg_atomic_truth(meta, values, world);
    }
    let arguments = match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(arguments) => arguments,
        Err(_opaque_condition) => return cfg_atomic_truth(meta, values, world),
    };
    if list.path.is_ident("all") {
        let mut variable = false;
        for argument in &arguments {
            match cfg_truth(argument, values, world) {
                CfgTruth::Never => return CfgTruth::Never,
                CfgTruth::Variable => variable = true,
                CfgTruth::Always => {}
            }
        }
        return if variable {
            CfgTruth::Variable
        } else {
            CfgTruth::Always
        };
    }
    if list.path.is_ident("any") {
        let mut variable = false;
        for argument in &arguments {
            match cfg_truth(argument, values, world) {
                CfgTruth::Always => return CfgTruth::Always,
                CfgTruth::Variable => variable = true,
                CfgTruth::Never => {}
            }
        }
        return if variable {
            CfgTruth::Variable
        } else {
            CfgTruth::Never
        };
    }
    if !list.path.is_ident("not") || arguments.len() != 1 {
        return cfg_atomic_truth(meta, values, world);
    }
    match arguments.first().map(|one| cfg_truth(one, values, world)) {
        Some(CfgTruth::Always) => CfgTruth::Never,
        Some(CfgTruth::Never) => CfgTruth::Always,
        Some(CfgTruth::Variable) | None => CfgTruth::Variable,
    }
}

fn cfg_atomic_truth(
    meta: &syn::Meta,
    values: &BTreeMap<String, bool>,
    world: CfgWorld,
) -> CfgTruth {
    if let Some(truth) = fixed_truth(meta, world) {
        return truth;
    }
    match values.get(&cfg_atom(meta)) {
        Some(true) => CfgTruth::Always,
        Some(false) => CfgTruth::Never,
        None => CfgTruth::Variable,
    }
}

fn cfg_atoms(meta: &syn::Meta, found: &mut BTreeSet<String>, world: CfgWorld) {
    if fixed_truth(meta, world).is_some() {
        return;
    }
    if let syn::Meta::List(list) = meta
        && (list.path.is_ident("all") || list.path.is_ident("any") || list.path.is_ident("not"))
    {
        match list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
        ) {
            Ok(arguments) if !list.path.is_ident("not") || arguments.len() == 1 => {
                for argument in &arguments {
                    cfg_atoms(argument, found, world);
                }
                return;
            }
            Ok(_) | Err(_) => {}
        }
    }
    found.insert(cfg_atom(meta));
}

fn cfg_constraints(meta: &syn::Meta, guards: &[syn::Meta], found: &mut Vec<CfgConstraint>) {
    let syn::Meta::List(list) = meta else {
        return;
    };
    if list.path.is_ident("cfg") {
        match list.parse_args::<syn::Meta>() {
            Ok(requirement) => found.push(CfgConstraint {
                guards: guards.to_vec(),
                requirement,
            }),
            Err(_opaque_condition) => {}
        }
        return;
    }
    if !list.path.is_ident("cfg_attr") {
        return;
    }
    let arguments = match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(arguments) => arguments,
        Err(_opaque_attributes) => return,
    };
    let mut arguments = arguments.iter();
    let Some(condition) = arguments.next() else {
        return;
    };
    let mut nested_guards = guards.to_vec();
    nested_guards.push(condition.clone());
    for attribute in arguments {
        cfg_constraints(attribute, &nested_guards, found);
    }
}

fn cfg_constraint_truth(
    constraint: &CfgConstraint,
    values: &BTreeMap<String, bool>,
    world: CfgWorld,
) -> CfgTruth {
    let mut guard = CfgTruth::Always;
    for condition in &constraint.guards {
        match cfg_truth(condition, values, world) {
            CfgTruth::Never => return CfgTruth::Always,
            CfgTruth::Variable => guard = CfgTruth::Variable,
            CfgTruth::Always => {}
        }
    }
    match (guard, cfg_truth(&constraint.requirement, values, world)) {
        (CfgTruth::Always, requirement) => requirement,
        (CfgTruth::Variable, CfgTruth::Always) | (CfgTruth::Never, _) => CfgTruth::Always,
        (CfgTruth::Variable, CfgTruth::Never | CfgTruth::Variable) => CfgTruth::Variable,
    }
}

pub(crate) fn item_attributes(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Const(one) => &one.attrs,
        syn::Item::Enum(one) => &one.attrs,
        syn::Item::ExternCrate(one) => &one.attrs,
        syn::Item::Fn(one) => &one.attrs,
        syn::Item::ForeignMod(one) => &one.attrs,
        syn::Item::Impl(one) => &one.attrs,
        syn::Item::Macro(one) => &one.attrs,
        syn::Item::Mod(one) => &one.attrs,
        syn::Item::Static(one) => &one.attrs,
        syn::Item::Struct(one) => &one.attrs,
        syn::Item::Trait(one) => &one.attrs,
        syn::Item::TraitAlias(one) => &one.attrs,
        syn::Item::Type(one) => &one.attrs,
        syn::Item::Union(one) => &one.attrs,
        syn::Item::Use(one) => &one.attrs,
        _ => &[],
    }
}
