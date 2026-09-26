// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

/// The error types of one file, by derive, by a written `Error` impl, or by name, and the types it gives a code.
#[derive(Default)]
struct Declared {
    errors: BTreeSet<String>,
    coded: BTreeSet<String>,
}

impl<'ast> syn::visit::Visit<'ast> for Declared {
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if is_error(&item.ident, &item.attrs) {
            self.errors.insert(item.ident.to_string());
        }
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        if is_error(&item.ident, &item.attrs) {
            self.errors.insert(item.ident.to_string());
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let implemented = item.trait_.as_ref().and_then(|(trait_, _for)| {
            trait_
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
        });
        if let Some(implemented) = implemented
            && let syn::Type::Path(type_) = item.self_ty.as_ref()
            && let Some(name) = type_.path.segments.last()
        {
            match implemented.as_str() {
                "Coded" => {
                    self.coded.insert(name.ident.to_string());
                }
                "Error" => {
                    self.errors.insert(name.ident.to_string());
                }
                _ => {}
            }
        }
        syn::visit::visit_item_impl(self, item);
    }
}

/// Whether a type named `ident` with `attrs` is an error: it derives `Error`, or its name says it is one.
fn is_error(ident: &syn::Ident, attrs: &[syn::Attribute]) -> bool {
    ident.to_string().ends_with("Error")
        || attrs.iter().any(|attribute| {
            let syn::Meta::List(list) = &attribute.meta else {
                return false;
            };
            list.path.is_ident("derive")
                && list
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                    )
                    .expect("a derive list is a list of paths")
                    .iter()
                    .any(|path| {
                        path.segments
                            .last()
                            .is_some_and(|segment| segment.ident == "Error")
                    })
        })
}

/// Every error type among `sources` that the file declaring it gives no `impl Coded`, as `path: name`.
fn uncoded(sources: &[(String, String)]) -> Vec<String> {
    let mut found = Vec::new();
    for (path, text) in sources {
        let file = syn::parse_file(text).expect("an xtask source parses");
        let mut declared = Declared::default();
        syn::visit::Visit::visit_file(&mut declared, &file);
        found.extend(
            declared
                .errors
                .difference(&declared.coded)
                .map(|name| format!("{path}: {name}")),
        );
    }
    found
}
