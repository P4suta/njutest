// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Which mutations a model checker can be asked about at all, answered from a signature and nothing else.

use crate::modelled::Unaskable;

/// The types a checker mints a symbolic value of without being told how.
///
/// A closed list, and deliberately shorter than what Kani can actually do.
/// The direction to be wrong in is settled: a type left off means the
/// mutation goes to the tests, which costs a run. A type wrongly on would
/// mean a harness that does not compile, and a milestone whose first act on
/// a real project is to fail to build is one nobody runs twice.
const MINTED: [&str; 18] = [
    "bool", "char", "f32", "f64", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32",
    "u64", "u128", "usize", "()", "!",
];

/// The wrappers a checker mints when what is inside is minted.
const MINTED_AROUND: [&str; 1] = ["Option"];

/// Whether every argument of `signature` can be given a symbolic value.
///
/// Answered by reading the signature, so a run knows which of its survivors
/// are worth starting a checker for before it starts one. That number is the
/// whole of what says whether this observer is worth its contract on a given
/// project, and it costs nothing to find out.
///
/// # Errors
/// The first argument no symbolic value can be made for, named as the
/// signature spells it.
pub fn askable(signature: &syn::Signature) -> Result<(), Unaskable> {
    for argument in &signature.inputs {
        let syn::FnArg::Typed(held) = argument else {
            return Err(Unaskable::NotArbitrary {
                argument: "self".to_owned(),
            });
        };
        if !minted(&held.ty) {
            return Err(Unaskable::NotArbitrary {
                argument: spelled(held),
            });
        }
    }
    Ok(())
}

/// Whether a checker mints a symbolic value of this type.
fn minted(held: &syn::Type) -> bool {
    match held {
        syn::Type::Tuple(tuple) => tuple.elems.is_empty() || tuple.elems.iter().all(minted),
        syn::Type::Array(array) => minted(&array.elem),
        syn::Type::Path(path) => path.qself.is_none() && named(&path.path),
        syn::Type::Paren(inner) => minted(&inner.elem),
        syn::Type::Never(_) => true,
        _ => false,
    }
}

/// Whether a named type is one of the minted ones, or a wrapper around one.
fn named(path: &syn::Path) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    let name = last.ident.to_string();
    match &last.arguments {
        syn::PathArguments::None => MINTED.contains(&name.as_str()),
        syn::PathArguments::AngleBracketed(inside) => {
            MINTED_AROUND.contains(&name.as_str())
                && inside.args.iter().all(|one| match one {
                    syn::GenericArgument::Type(held) => minted(held),
                    _ => false,
                })
        }
        syn::PathArguments::Parenthesized(_) => false,
    }
}

/// The argument as a reader would name it: the binding and the head of its type.
///
/// Not the whole type. A reader who is told `order: Order` knows which
/// argument and which type to go and look at, and a rendering of every
/// generic parameter would put the answer further from the question.
fn spelled(held: &syn::PatType) -> String {
    let name = match held.pat.as_ref() {
        syn::Pat::Ident(ident) => ident.ident.to_string(),
        _ => "an argument".to_owned(),
    };
    format!("{name}: {}", head(&held.ty))
}

/// The head of a type, as a reader would say it aloud.
fn head(held: &syn::Type) -> String {
    match held {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map_or_else(|| "a type".to_owned(), |last| last.ident.to_string()),
        syn::Type::Reference(one) => format!("&{}", head(&one.elem)),
        syn::Type::Slice(one) => format!("[{}]", head(&one.elem)),
        syn::Type::Array(one) => format!("[{}; _]", head(&one.elem)),
        syn::Type::Tuple(one) if one.elems.is_empty() => "()".to_owned(),
        syn::Type::Tuple(_) => "a tuple".to_owned(),
        _ => "a type".to_owned(),
    }
}

/// What a run can say about a whole catalogue before it starts a checker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Reach {
    /// How many could be asked.
    pub askable: u32,
    /// How many could not, because of an argument no symbolic value can be made for.
    pub unaskable: u32,
}

impl Reach {
    /// Counts one more, whichever it is.
    pub const fn counted(&mut self, asked: bool) {
        if asked {
            self.askable = self.askable.saturating_add(1);
        } else {
            self.unaskable = self.unaskable.saturating_add(1);
        }
    }

    /// How many were looked at.
    #[must_use]
    pub const fn considered(self) -> u32 {
        self.askable.saturating_add(self.unaskable)
    }
}
