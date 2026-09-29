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
    /// No top-level struct with named fields has the name asked for.
    #[error("the source declares no top-level struct {name} with named fields")]
    NoStruct {
        /// The struct asked for.
        name: String,
    },
    /// A `serde` attribute of a field is not one this reader can follow.
    #[error("a serde attribute of {name} cannot be read: {message}")]
    UnreadAttribute {
        /// The struct asked for.
        name: String,
        /// What the parser said.
        message: String,
    },
}

/// Every top-level constant of `text`.
fn constants(text: &str) -> Result<Vec<syn::ItemConst>, RustSourceError> {
    let file = crate::lexed::file(text).map_err(|error| RustSourceError::Unparsed {
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

/// The name every field of the top-level struct `name` of `text` is deserialized under, a `serde(rename)` applied, in declaration order.
///
/// # Errors
/// A [`RustSourceError`] when `text` is not Rust, declares no such struct with named fields, or holds a `serde` attribute this reader cannot follow.
pub fn serde_field_names(text: &str, name: &str) -> Result<Vec<String>, RustSourceError> {
    let file = crate::lexed::file(text).map_err(|error| RustSourceError::Unparsed {
        message: error.to_string(),
    })?;
    let fields = file
        .items
        .iter()
        .find_map(|item| match item {
            syn::Item::Struct(declared) if declared.ident == name => match &declared.fields {
                syn::Fields::Named(fields) => Some(fields),
                syn::Fields::Unnamed(_) | syn::Fields::Unit => None,
            },
            _ => None,
        })
        .ok_or_else(|| RustSourceError::NoStruct {
            name: name.to_owned(),
        })?;
    fields
        .named
        .iter()
        .filter_map(|field| field.ident.as_ref().map(|ident| (field, ident)))
        .map(|(field, ident)| {
            serde_name(field, ident).map_err(|error| RustSourceError::UnreadAttribute {
                name: name.to_owned(),
                message: error.to_string(),
            })
        })
        .collect()
}

/// The name one field is deserialized under: its `serde(rename)` where it has one, and `ident` otherwise.
fn serde_name(field: &syn::Field, ident: &syn::Ident) -> syn::Result<String> {
    let mut renamed = None;
    for attribute in field
        .attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
    {
        attribute.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                let value = meta.value()?.parse::<syn::LitStr>()?;
                renamed = Some(value.value());
            } else if meta.input.peek(syn::Token![=]) {
                let passed_over = meta.value()?.parse::<syn::Expr>()?;
                drop(passed_over);
            }
            Ok(())
        })?;
    }
    Ok(renamed.unwrap_or_else(|| ident.to_string()))
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
