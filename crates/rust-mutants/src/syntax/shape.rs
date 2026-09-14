// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What the syntax alone says about a signature, a type, a pattern, or a block.
//!
//! The walk asks these before it proposes anything: what a function says it
//! returns, whether a type spells a default, whether an arm can be deleted,
//! which attributes are on the thing in hand. None of them looks at more than
//! the node it is given, and none of them decides anything — the walk does
//! that, and keeping the questions apart from the decisions is what keeps
//! either readable.

use std::collections::BTreeSet;

use proc_macro2::{TokenStream, TokenTree};
use syn::{Attribute, Block, Expr, Item, Meta, Pat, ReturnType, Stmt, Type};

use super::SkipReason;
use super::walk::ReturnKind;

/// What a signature says the function returns.
pub(super) fn return_kind(output: &ReturnType) -> ReturnKind {
    return_kind_within(output, &BTreeSet::new(), &BTreeSet::new())
}

/// What a return type says about the replacements a rule can offer for it.
///
/// `generic` is the type parameters the signature introduces and `defaultable`
/// the ones something bound to `Default`. A parameter nothing bound is a type
/// the syntax cannot say has a default, and offering one is a candidate the
/// compiler refuses: predicting the refusal and stating it is what keeps a
/// reader from reading a refusal as a fact about the program.
pub(super) fn return_kind_within(
    output: &ReturnType,
    generic: &BTreeSet<String>,
    defaultable: &BTreeSet<String>,
) -> ReturnKind {
    match output {
        ReturnType::Default => ReturnKind::Unit,
        ReturnType::Type(_, ty) => return_kind_of(ty, generic, defaultable),
    }
}

pub(super) fn return_kind_of(
    ty: &Type,
    generic: &BTreeSet<String>,
    defaultable: &BTreeSet<String>,
) -> ReturnKind {
    match ty {
        Type::Tuple(t) if t.elems.is_empty() => ReturnKind::Unit,
        Type::Never(_) => ReturnKind::Never,
        Type::Paren(p) => return_kind_of(&p.elem, generic, defaultable),
        Type::Group(g) => return_kind_of(&g.elem, generic, defaultable),
        Type::Reference(one) if borrows_a_default(&one.elem) => ReturnKind::Other,
        Type::ImplTrait(_)
        | Type::Reference(_)
        | Type::Ptr(_)
        | Type::FnPtr(_)
        | Type::TraitObject(_)
        | Type::Slice(_)
        | Type::Macro(_)
        | Type::Infer(_) => ReturnKind::Unstated,
        Type::Path(p) if p.qself.is_some() || wraps_a_trait_object(p) => ReturnKind::Unstated,
        Type::Path(p) => {
            let first = p.path.segments.first().map(|s| s.ident.to_string());
            if p.path.segments.len() > 1
                && first.as_deref().is_some_and(|name| generic.contains(name))
            {
                return ReturnKind::Unstated;
            }
            let last = p.path.segments.last();
            match last.map(|s| s.ident.to_string()).as_deref() {
                Some("bool") => ReturnKind::Bool,
                Some("Result") => ReturnKind::Result {
                    ok: argument_defaults(last, 0, generic, defaultable),
                    err: argument_names_a_default(last, 1, defaultable),
                },
                Some("Option") => {
                    ReturnKind::Option(argument_defaults(last, 0, generic, defaultable))
                }
                Some(name)
                    if p.path.segments.len() == 1
                        && generic.contains(name)
                        && !defaultable.contains(name) =>
                {
                    ReturnKind::Unstated
                }
                _ => ReturnKind::Other,
            }
        }
        _ => ReturnKind::Other,
    }
}

/// Whether the `nth` type argument of a segment is one the syntax can say has a default.
///
/// `Option<T>` and `Result<T, E>` have a default whatever `T` is — `None` and
/// nothing — but `Some(Default::default())` and `Ok(Default::default())` need
/// one for `T`, so the inner type is asked about separately.
pub(super) fn argument_defaults(
    segment: Option<&syn::PathSegment>,
    nth: usize,
    generic: &BTreeSet<String>,
    defaultable: &BTreeSet<String>,
) -> bool {
    let Some(syn::PathArguments::AngleBracketed(args)) = segment.map(|one| &one.arguments) else {
        return false;
    };
    let mut types = args.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    let Some(ty) = types.nth(nth) else {
        return false;
    };
    !matches!(
        return_kind_of(ty, generic, defaultable),
        ReturnKind::Unstated
    )
}

/// Whether the `nth` type argument of `segment` is one the syntax names a default for.
///
/// The `Err` position is read more strictly than the others. A crate's error
/// type is by convention the crate's own and by convention does not implement
/// `Default`, so offering `Err(Default::default())` for every named type would
/// spend a compiler refusal at nearly every `Result` in a program and say
/// nothing about the tests. The other direction of the same question —
/// success where the code says failure — is `return-ok-default`, which is
/// offered wherever the `Ok` type has a default, so nothing is lost.
pub(super) fn argument_names_a_default(
    segment: Option<&syn::PathSegment>,
    nth: usize,
    defaultable: &BTreeSet<String>,
) -> bool {
    let Some(syn::PathArguments::AngleBracketed(args)) = segment.map(|one| &one.arguments) else {
        return false;
    };
    let mut types = args.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });
    types
        .nth(nth)
        .is_some_and(|ty| names_a_default(ty, defaultable))
}

/// Whether the syntax names a type the standard library gives a `Default`, or a parameter something bound to it.
pub(super) fn names_a_default(ty: &Type, defaultable: &BTreeSet<String>) -> bool {
    match ty {
        Type::Tuple(one) => one.elems.is_empty(),
        Type::Paren(p) => names_a_default(&p.elem, defaultable),
        Type::Group(g) => names_a_default(&g.elem, defaultable),
        Type::Reference(one) => one.mutability.is_none() && borrows_a_default(&one.elem),
        Type::Path(p) if p.qself.is_none() => p.path.segments.last().is_some_and(|segment| {
            let name = segment.ident.to_string();
            defaultable.contains(&name) || DEFAULTS.contains(&name.as_str())
        }),
        _ => false,
    }
}

/// The types the standard library gives a `Default` to that a program is likely to spell as an error.
pub(super) const DEFAULTS: [&str; 26] = [
    "String", "Vec", "VecDeque", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "Option", "bool",
    "char", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize",
    "f32", "f64", "str", "PathBuf",
];

/// The name an `impl` block goes by: the type it is for, or `<Type as Trait>` when it implements one.
pub(super) fn implemented(block: &syn::ItemImpl) -> String {
    let name = type_name(&block.self_ty);
    match &block.trait_ {
        Some((path, _)) => {
            let trait_name = path
                .segments
                .last()
                .map_or_else(String::new, |segment| segment.ident.to_string());
            format!("<{name} as {trait_name}>")
        }
        None => name,
    }
}

/// The last segment of a type's name, which is what a reader writes.
pub(super) fn type_name(ty: &Type) -> String {
    match ty {
        Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.to_string()),
        Type::Paren(one) => type_name(&one.elem),
        Type::Group(one) => type_name(&one.elem),
        Type::Reference(one) => type_name(&one.elem),
        _ => String::new(),
    }
}

/// The guard an arm's pattern carries, when it carries one. In `syn` the guard is part of the pattern rather than of the arm.
pub(super) fn guard_of(pat: &Pat) -> Option<&Expr> {
    match pat {
        Pat::Guard(one) => Some(&one.guard),
        Pat::Paren(one) => guard_of(&one.pat),
        _ => None,
    }
}

/// Whether the pattern matches everything and asks nothing, so that the arms after it are unreachable and the match is exhaustive with it.
pub(super) fn is_bare_wildcard(pat: &Pat) -> bool {
    match pat {
        Pat::Wild(_) => true,
        Pat::Paren(one) => is_bare_wildcard(&one.pat),
        _ => false,
    }
}

/// Whether an arm can be taken out of the match without the match ceasing to be exhaustive.
///
/// The syntax can say so for one shape only: an arm that is not itself a bare
/// `_`, with a bare `_` somewhere below it. Every other arm may be the one
/// carrying exhaustiveness, and a mutation the compiler refuses says nothing
/// about the tests.
pub(super) fn deletable_arm(arms: &[syn::Arm], position: usize) -> bool {
    let Some(arm) = arms.get(position) else {
        return false;
    };
    if is_bare_wildcard(&arm.pat) {
        return false;
    }
    arms.get(position.saturating_add(1)..)
        .unwrap_or_default()
        .iter()
        .any(|later| is_bare_wildcard(&later.pat))
}

/// The value a block ends with, when it ends with one rather than with a statement.
pub(super) fn block_tail(block: &Block) -> Option<&Expr> {
    match block.stmts.last() {
        Some(Stmt::Expr(expr, None)) => Some(expr),
        _ => None,
    }
}

/// Whether the type is a pointer the standard library gives no default to: a `Box`, an `Rc`, or an `Arc` around a trait object, which has no size for a default to have.
pub(super) fn wraps_a_trait_object(path: &syn::TypePath) -> bool {
    let Some(segment) = path.path.segments.last() else {
        return false;
    };
    if !matches!(segment.ident.to_string().as_str(), "Box" | "Rc" | "Arc") {
        return false;
    }
    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return false;
    };
    args.args.iter().any(|arg| {
        matches!(
            arg,
            syn::GenericArgument::Type(Type::TraitObject(_) | Type::ImplTrait(_))
        )
    })
}

/// Whether a reference to this type has a default: the standard library gives one to a shared or unique reference to a slice or to `str`, and to no other reference.
pub(super) fn borrows_a_default(ty: &Type) -> bool {
    match ty {
        Type::Slice(_) => true,
        Type::Paren(p) => borrows_a_default(&p.elem),
        Type::Group(g) => borrows_a_default(&g.elem),
        Type::Path(p) => {
            p.qself.is_none()
                && p.path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "str")
        }
        _ => false,
    }
}

/// The type parameters a signature introduces, and the ones something bound to `Default`.
pub(super) fn parameters(generics: &syn::Generics) -> (BTreeSet<String>, BTreeSet<String>) {
    let mut named = BTreeSet::new();
    let mut defaultable = BTreeSet::new();
    for param in &generics.params {
        let syn::GenericParam::Type(one) = param else {
            continue;
        };
        let name = one.ident.to_string();
        if one.bounds.iter().any(spells_default) {
            let _added = defaultable.insert(name.clone());
        }
        let _added = named.insert(name);
    }
    let Some(clause) = &generics.where_clause else {
        return (named, defaultable);
    };
    for predicate in &clause.predicates {
        let syn::WherePredicate::Type(one) = predicate else {
            continue;
        };
        let Type::Path(path) = &one.bounded_ty else {
            continue;
        };
        if path.path.segments.len() != 1 || !one.bounds.iter().any(spells_default) {
            continue;
        }
        if let Some(segment) = path.path.segments.first() {
            let _added = defaultable.insert(segment.ident.to_string());
        }
    }
    (named, defaultable)
}

/// Whether one bound is `Default`, by the name it is written with.
pub(super) fn spells_default(bound: &syn::TypeParamBound) -> bool {
    let syn::TypeParamBound::Trait(one) = bound else {
        return false;
    };
    one.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "Default")
}

/// The reason attributes suppress what they decorate: `#[test]` and `#[bench]` are test code, a `cfg` mentioning `test` is test code, and any other `cfg` is a configuration the walker does not evaluate.
pub(super) fn suppression_of(attrs: &[Attribute]) -> Option<SkipReason> {
    let mut cfg = None;
    for attr in attrs {
        let path = attr.path();
        if path.is_ident("test") || path.is_ident("bench") {
            return Some(SkipReason::TestCode);
        }
        if path.is_ident("cfg") {
            if let Meta::List(list) = &attr.meta
                && mentions_test(&list.tokens)
            {
                return Some(SkipReason::TestCode);
            }
            cfg = Some(SkipReason::CfgAttribute);
        }
    }
    cfg
}

pub(super) fn mentions_test(tokens: &TokenStream) -> bool {
    tokens.clone().into_iter().any(|tree| match tree {
        TokenTree::Ident(ident) => ident == "test",
        TokenTree::Group(group) => mentions_test(&group.stream()),
        TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    })
}

pub(super) fn item_attrs(item: &Item) -> &[Attribute] {
    match item {
        Item::Const(i) => &i.attrs,
        Item::Enum(i) => &i.attrs,
        Item::ExternCrate(i) => &i.attrs,
        Item::Fn(i) => &i.attrs,
        Item::ForeignMod(i) => &i.attrs,
        Item::Impl(i) => &i.attrs,
        Item::Macro(i) => &i.attrs,
        Item::Mod(i) => &i.attrs,
        Item::Static(i) => &i.attrs,
        Item::Struct(i) => &i.attrs,
        Item::Trait(i) => &i.attrs,
        Item::TraitAlias(i) => &i.attrs,
        Item::Type(i) => &i.attrs,
        Item::Union(i) => &i.attrs,
        Item::Use(i) => &i.attrs,
        _ => &[],
    }
}

pub(super) fn expr_attrs(expr: &Expr) -> &[Attribute] {
    match expr {
        Expr::Array(e) => &e.attrs,
        Expr::Assign(e) => &e.attrs,
        Expr::Async(e) => &e.attrs,
        Expr::Await(e) => &e.attrs,
        Expr::Binary(e) => &e.attrs,
        Expr::Block(e) => &e.attrs,
        Expr::Break(e) => &e.attrs,
        Expr::Call(e) => &e.attrs,
        Expr::Cast(e) => &e.attrs,
        Expr::Closure(e) => &e.attrs,
        Expr::Const(e) => &e.attrs,
        Expr::Continue(e) => &e.attrs,
        Expr::Field(e) => &e.attrs,
        Expr::ForLoop(e) => &e.attrs,
        Expr::Group(e) => &e.attrs,
        Expr::If(e) => &e.attrs,
        Expr::Index(e) => &e.attrs,
        Expr::Infer(e) => &e.attrs,
        Expr::Let(e) => &e.attrs,
        Expr::Lit(e) => &e.attrs,
        Expr::Loop(e) => &e.attrs,
        Expr::Macro(e) => &e.attrs,
        Expr::Match(e) => &e.attrs,
        Expr::MethodCall(e) => &e.attrs,
        Expr::Paren(e) => &e.attrs,
        Expr::Path(e) => &e.attrs,
        Expr::Range(e) => &e.attrs,
        Expr::RawAddr(e) => &e.attrs,
        Expr::Reference(e) => &e.attrs,
        Expr::Repeat(e) => &e.attrs,
        Expr::Return(e) => &e.attrs,
        Expr::Struct(e) => &e.attrs,
        Expr::Try(e) => &e.attrs,
        Expr::TryBlock(e) => &e.attrs,
        Expr::Tuple(e) => &e.attrs,
        Expr::Unary(e) => &e.attrs,
        Expr::Unsafe(e) => &e.attrs,
        Expr::While(e) => &e.attrs,
        Expr::Yield(e) => &e.attrs,
        _ => &[],
    }
}
