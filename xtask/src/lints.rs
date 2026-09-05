// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Two things this repository does not write.
//!
//! **`#[allow]`.** A lint switched off with `allow` stays off forever, and
//! nothing ever says so: the code it was hiding can be deleted and the
//! attribute remains, silently covering whatever arrives next. `#[expect]`
//! is the same waiver with an expiry — the compiler fails the build when the
//! lint stops firing — so a waiver here is always one somebody still needs.
//!
//! **`Box<dyn Trait>`.** Rust has two ways to accept more than one
//! implementation, and the one that costs an allocation and a vtable is the
//! second choice: a closed set is an enum the compiler checks exhaustively,
//! an open one is a generic parameter. A boxed trait object is worth it only
//! where neither works, and this gate makes that a decision somebody argues
//! for rather than one that accumulates.
//!
//! Both are scanned across every Rust file the repository commits, tests
//! included. The code the engine *generates* into somebody else's tree is
//! not scanned as code — it lives here as string literals, and its
//! `#[allow(warnings)]` is deliberate: an expectation that went unfulfilled
//! in a user's build would be our lint noise in their terminal.

use std::fmt;

use syn::visit::Visit;

/// What kind of thing was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Kind {
    /// An `#[allow(…)]` or `#![allow(…)]` attribute.
    AllowAttribute,
    /// A `Box<dyn Trait>` type.
    BoxedTraitObject,
}

impl Kind {
    /// What to write in a report.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::AllowAttribute => "allow-attribute",
            Self::BoxedTraitObject => "boxed-trait-object",
        }
    }

    /// Why it is refused, in the words a person needs to fix it.
    #[must_use]
    pub const fn remedy(self) -> &'static str {
        match self {
            Self::AllowAttribute => {
                "use #[expect(…, reason = \"…\")], which the compiler retires when the lint \
                 stops firing"
            }
            Self::BoxedTraitObject => {
                "use an enum for a closed set of implementations, or a generic parameter for \
                 an open one"
            }
        }
    }
}

/// One thing found in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// What it is.
    pub kind: Kind,
    /// The file, as a repository-relative path.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.file,
            self.line,
            self.kind.label(),
            self.kind.remedy()
        )
    }
}

/// Everything `source` holds that this repository does not write.
///
/// # Errors
///
/// A file that is not Rust this version can parse.
pub fn scan_source(file: &str, source: &str) -> Result<Vec<Finding>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut scan = Scan {
        file: file.to_owned(),
        found: Vec::new(),
    };
    scan.visit_file(&parsed);
    scan.found.sort();
    Ok(scan.found)
}

struct Scan {
    file: String,
    found: Vec<Finding>,
}

impl Scan {
    fn note(&mut self, kind: Kind, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind,
            file: self.file.clone(),
            line: span.start().line,
        });
    }
}

impl Visit<'_> for Scan {
    fn visit_attribute(&mut self, attribute: &syn::Attribute) {
        if attribute.path().is_ident("allow")
            && let Some(segment) = attribute.path().segments.first()
        {
            self.note(Kind::AllowAttribute, segment.ident.span());
        }
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if let Some(segment) = path.path.segments.last()
            && segment.ident == "Box"
            && let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments
            && arguments.args.iter().any(|argument| {
                matches!(
                    argument,
                    syn::GenericArgument::Type(syn::Type::TraitObject(_))
                )
            })
        {
            self.note(Kind::BoxedTraitObject, segment.ident.span());
        }
        syn::visit::visit_type_path(self, path);
    }
}
