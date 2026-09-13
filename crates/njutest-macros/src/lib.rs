// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The attribute macros behind `njutest::integration` and `njutest::unit`.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{LitStr, Token};

/// Declares that the annotated test needs the named managed resources.
#[proc_macro_attribute]
pub fn integration(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let item = TokenStream2::from(item);
    finish(item.clone(), check_integration(attribute.into(), &item))
}

/// Declares that the annotated test needs no managed resource.
#[proc_macro_attribute]
pub fn unit(attribute: TokenStream, item: TokenStream) -> TokenStream {
    let item = TokenStream2::from(item);
    finish(item.clone(), check_unit(attribute.into(), &item))
}

fn finish(item: TokenStream2, check: syn::Result<()>) -> TokenStream {
    let mut output = item;
    if let Err(error) = check {
        output.extend(error.to_compile_error());
    }
    output.into()
}

struct Capabilities(Punctuated<LitStr, Token![,]>);

impl Parse for Capabilities {
    fn parse(input: ParseStream<'_>) -> syn::Result<Self> {
        Punctuated::parse_terminated(input).map(Self)
    }
}

fn check_integration(attribute: TokenStream2, item: &TokenStream2) -> syn::Result<()> {
    let names = syn::parse2::<Capabilities>(attribute).map_err(|error| {
        syn::Error::new(
            error.span(),
            "njutest::integration takes string literals: the capability names",
        )
    })?;
    if names.0.is_empty() {
        return Err(syn::Error::new(
            Span::call_site(),
            "integration requires at least one capability",
        ));
    }
    for (index, name) in names.0.iter().enumerate() {
        if name.value().trim().is_empty() {
            let position = index.saturating_add(1);
            return Err(syn::Error::new(
                name.span(),
                format!("integration capability {position} must not be blank"),
            ));
        }
    }
    require_function(item, "integration")
}

fn check_unit(attribute: TokenStream2, item: &TokenStream2) -> syn::Result<()> {
    if let Some(first) = attribute.into_iter().next() {
        return Err(syn::Error::new(
            first.span(),
            "njutest::unit takes no arguments",
        ));
    }
    require_function(item, "unit")
}

fn require_function(item: &TokenStream2, attribute: &str) -> syn::Result<()> {
    match syn::parse2::<syn::Item>(item.clone()) {
        Ok(syn::Item::Fn(_)) => Ok(()),
        Ok(_) | Err(_) => Err(syn::Error::new(
            Span::call_site(),
            format!("#[njutest::{attribute}] applies to a function"),
        )),
    }
}
