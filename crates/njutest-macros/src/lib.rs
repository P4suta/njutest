// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Compiler-enforced declarations shared by the njutest workspace.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields, LitStr, Token};

/// Derives an `ALL` array directly from every unit variant of an enum.
///
/// The compiler refuses data-bearing variants, generic enums, conditionally
/// compiled variants, and `#[non_exhaustive]`: each would make a single closed
/// array ambiguous or incomplete.
#[proc_macro_derive(AllVariants)]
pub fn all_variants(item: TokenStream) -> TokenStream {
    match expand_all_variants(item.into()) {
        Ok(output) => output.into(),
        Err(error) => error.to_compile_error().into(),
    }
}

fn expand_all_variants(item: TokenStream2) -> syn::Result<TokenStream2> {
    let input = syn::parse2::<DeriveInput>(item)?;
    if !input.generics.params.is_empty() || input.generics.where_clause.is_some() {
        return Err(syn::Error::new(
            input.generics.span(),
            "AllVariants requires a non-generic enum",
        ));
    }
    if let Some(attribute) = input
        .attrs
        .iter()
        .find(|attribute| attribute.path().is_ident("non_exhaustive"))
    {
        return Err(syn::Error::new(
            attribute.span(),
            "AllVariants requires a closed enum; remove #[non_exhaustive]",
        ));
    }
    let Data::Enum(data) = input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "AllVariants can only be derived for an enum",
        ));
    };
    let mut variants = Vec::with_capacity(data.variants.len());
    for variant in data.variants {
        if !matches!(variant.fields, Fields::Unit) {
            return Err(syn::Error::new(
                variant.fields.span(),
                "AllVariants requires every variant to carry no fields",
            ));
        }
        if let Some(attribute) = variant.attrs.iter().find(|attribute| {
            attribute.path().is_ident("cfg") || attribute.path().is_ident("cfg_attr")
        }) {
            return Err(syn::Error::new(
                attribute.span(),
                "AllVariants refuses conditionally compiled variants",
            ));
        }
        variants.push(variant.ident);
    }
    let ident = input.ident;
    let count = variants.len();
    Ok(quote! {
        impl #ident {
            /// Every variant, in declaration order.
            pub const ALL: [Self; #count] = [#(Self::#variants),*];
        }
    })
}

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

#[cfg(test)]
mod tests {
    use quote::quote;

    use super::expand_all_variants;

    #[test]
    fn generic_enums_are_refused() {
        let actual = match expand_all_variants(quote!(
            enum State<T> {
                Waiting,
            }
        )) {
            Ok(_) => None,
            Err(error) => Some(error.to_string()),
        };
        assert_eq!(
            actual,
            Some("AllVariants requires a non-generic enum".to_owned())
        );
    }

    #[test]
    fn non_exhaustive_enums_are_refused() {
        let actual = match expand_all_variants(quote!(
            #[non_exhaustive]
            enum State {
                Waiting,
            }
        )) {
            Ok(_) => None,
            Err(error) => Some(error.to_string()),
        };
        assert_eq!(
            actual,
            Some("AllVariants requires a closed enum; remove #[non_exhaustive]".to_owned())
        );
    }
}
