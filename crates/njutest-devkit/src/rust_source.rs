// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! This repository's own Rust source, read as items rather than lines, so whatever surrounds an item cannot move the answer.

/// Rust source that does not hold what a question about it asked for.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RustSourceError {
    /// The text is not a Rust file.
    #[error("the source is not Rust: {message}")]
    Unparsed {
        /// What the parser said.
        message: String,
    },
    /// No top-level constant has the name asked for.
    #[error("the source declares no top-level constant {name}")]
    NoConstant {
        /// The constant asked for.
        name: String,
    },
    /// The constant is not an array literal.
    #[error("the constant {name} is not an array literal")]
    NotAnArray {
        /// The constant asked for.
        name: String,
    },
    /// An element of the array is not what the question reads.
    #[error("the constant {name} lists an element that is not {wanted}")]
    UnexpectedElement {
        /// The constant asked for.
        name: String,
        /// What every element was asked to be.
        wanted: &'static str,
    },
}

/// Every top-level constant of `text`.
fn constants(text: &str) -> Result<Vec<syn::ItemConst>, RustSourceError> {
    let file = syn::parse_file(text).map_err(|error| RustSourceError::Unparsed {
        message: error.to_string(),
    })?;
    Ok(file
        .items
        .into_iter()
        .filter_map(|item| match item {
            syn::Item::Const(constant) => Some(constant),
            _ => None,
        })
        .collect())
}

/// The elements of the top-level array constant `name` of `text`.
fn elements(text: &str, name: &str) -> Result<Vec<syn::Expr>, RustSourceError> {
    let constant = constants(text)?
        .into_iter()
        .find(|constant| constant.ident == name)
        .ok_or_else(|| RustSourceError::NoConstant {
            name: name.to_owned(),
        })?;
    match *constant.expr {
        syn::Expr::Array(array) => Ok(array.elems.into_iter().collect()),
        _ => Err(RustSourceError::NotAnArray {
            name: name.to_owned(),
        }),
    }
}

/// The name of every public top-level `&str` constant of `text`.
///
/// # Errors
/// [`RustSourceError::Unparsed`] when `text` is not Rust.
pub fn public_text_constants(text: &str) -> Result<Vec<String>, RustSourceError> {
    Ok(constants(text)?
        .into_iter()
        .filter(|constant| matches!(constant.vis, syn::Visibility::Public(_)))
        .filter(|constant| {
            matches!(&*constant.ty, syn::Type::Reference(reference)
                if matches!(&*reference.elem, syn::Type::Path(path) if path.path.is_ident("str")))
        })
        .map(|constant| constant.ident.to_string())
        .collect())
}

/// The bare names the top-level array constant `name` of `text` lists, in order.
///
/// # Errors
/// A [`RustSourceError`] when `text` is not Rust, declares no such array, or lists something other than a bare name.
pub fn names_listed(text: &str, name: &str) -> Result<Vec<String>, RustSourceError> {
    elements(text, name)?
        .iter()
        .map(|element| match element {
            syn::Expr::Path(path) => path.path.get_ident().map(ToString::to_string),
            _ => None,
        })
        .map(|listed| {
            listed.ok_or_else(|| RustSourceError::UnexpectedElement {
                name: name.to_owned(),
                wanted: "a bare name",
            })
        })
        .collect()
}

/// The string literals the top-level array constant `name` of `text` lists, in order.
///
/// # Errors
/// A [`RustSourceError`] when `text` is not Rust, declares no such array, or lists something other than a string literal.
pub fn strings_listed(text: &str, name: &str) -> Result<Vec<String>, RustSourceError> {
    elements(text, name)?
        .iter()
        .map(|element| match element {
            syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(literal),
                ..
            }) => Some(literal.value()),
            _ => None,
        })
        .map(|listed| {
            listed.ok_or_else(|| RustSourceError::UnexpectedElement {
                name: name.to_owned(),
                wanted: "a string literal",
            })
        })
        .collect()
}
