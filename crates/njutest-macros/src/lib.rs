// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The derive that gives a closed fieldless enum the whole list of its variants.

#![forbid(unsafe_code)]

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::quote;
use syn::spanned::Spanned;
use syn::{Data, DeriveInput, Fields};

/// Derives an `ALL` array directly from every unit variant of an enum.
///
/// The compiler refuses data-bearing variants, generic enums, conditionally compiled variants, and `#[non_exhaustive]`: each would make a single closed array ambiguous or incomplete.
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
