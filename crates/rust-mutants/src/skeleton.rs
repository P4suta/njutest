// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What an edit can change without an execution noticing it: which item bodies are sealed, each body's digest, and each unit's skeleton (ADR 0041).

use std::collections::BTreeMap;

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use serde::{Deserialize, Serialize};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};

use crate::touch::{Item, ItemRef};

/// The file a run keeps this evidence in, beside `touched-v1.json`.
pub const FILE: &str = "skeletons-v1.json";

/// Names the shape of [`Skeletons`].
pub const DOCUMENT_TYPE: &str = "rust-mutants/skeletons";

/// The version of that shape.
pub const SCHEMA_VERSION: u32 = 1;

/// The standard macros a sealed body may invoke: each expands to an expression or a statement and declares nothing.
pub const SEALABLE_MACROS: [&str; 28] = [
    "assert",
    "assert_eq",
    "assert_ne",
    "cfg",
    "column",
    "concat",
    "dbg",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "eprint",
    "eprintln",
    "file",
    "format",
    "format_args",
    "line",
    "matches",
    "module_path",
    "panic",
    "print",
    "println",
    "stringify",
    "todo",
    "unimplemented",
    "unreachable",
    "vec",
    "write",
    "writeln",
];

/// The crates a macro path may be qualified with and still name a standard macro, and a glob may import from and still bring no macro in.
pub const STANDARD_ROOTS: [&str; 3] = ["std", "core", "alloc"];

/// The roots a glob may import from and still bring in only what the unit itself declares.
pub const LOCAL_ROOTS: [&str; 3] = ["self", "super", "crate"];

/// The attributes a sealed body, its item, and the items around it may carry.
pub const SEALABLE_ATTRIBUTES: [&str; 16] = [
    "allow",
    "cfg",
    "cfg_attr",
    "cold",
    "deny",
    "deprecated",
    "doc",
    "expect",
    "forbid",
    "ignore",
    "inline",
    "must_use",
    "should_panic",
    "test",
    "track_caller",
    "warn",
];

/// The tool namespaces whose attributes only lints, formatters and diagnostics read.
pub const TOOL_NAMESPACES: [&str; 3] = ["clippy", "diagnostic", "rustfmt"];

/// The words a macro cannot be called, so `if !(…)` is not an invocation of `if`.
const KEYWORDS: [&str; 38] = [
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];

/// The evidence one run keeps for carrying answers across edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skeletons {
    /// [`DOCUMENT_TYPE`].
    pub document_type: String,
    /// [`SCHEMA_VERSION`].
    pub schema_version: u32,
    /// Every cataloged item, by item index.
    pub items: Vec<ItemEvidence>,
    /// Every compiled unit, sorted by its name.
    pub units: Vec<UnitSkeleton>,
}

/// One item's body, and whether an edit inside it can matter to anything that does not enter it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemEvidence {
    /// The item index `touched-v1.json` names it by.
    pub index: u32,
    /// The item as every record that names one names it: its package, its file, and its position among the file's cataloged items.
    pub item: ItemRef,
    /// The item as a reader writes it.
    pub name: String,
    /// The lowercase hex SHA-256 of its body's bytes, braces included.
    pub body_digest: String,
    /// Whether everything the body contributes to the program is its own execution.
    pub sealed: bool,
    /// Why it is not sealed, absent exactly when it is.
    pub unsealed: Option<Unsealing>,
}

/// Why a body is not sealed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "why", rename_all = "kebab-case", deny_unknown_fields)]
pub enum Unsealing {
    /// The item is a `const fn`, a `const` or a `static`, which can be evaluated where nothing enters it.
    Evaluated,
    /// A unit that read the item's file runs in the compiler, as a procedural macro or a build script does, where no test enters it.
    CompileTime,
    /// The function is `async` or returns an `impl` type, whose hidden type the body decides and a caller can observe without entering it.
    OpaqueType,
    /// The body invokes a macro off the list, or one qualified by a path outside the standard library.
    Macro {
        /// The macro's path as written.
        name: String,
    },
    /// The item, an item around it, or something inside it carries an attribute off the list.
    Attribute {
        /// The attribute's path as written.
        name: String,
    },
    /// The body declares an item, which contributes to the program whether or not the body runs.
    DeclaresItem {
        /// Which kind of item.
        kind: String,
    },
    /// The body holds an inline `const` block, which is evaluated where nothing enters it.
    ConstBlock,
    /// A file of the unit declares or imports a macro under a listed name.
    Shadowed {
        /// The name.
        name: String,
    },
    /// A file of the unit imports every name of a path outside the standard library and this crate, or takes every macro of a crate.
    ForeignGlob {
        /// The path imported from.
        path: String,
    },
    /// The body could not be found where the catalog says it is.
    Unlocated,
    /// No unit compiled the item's file.
    Unread,
}

/// One compiled unit, named without a package id, and the digest of everything outside its sealed bodies.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitSkeleton {
    /// The package's name.
    pub package: String,
    /// The target's name.
    pub target: String,
    /// The target's kinds, comma-separated.
    pub kind: String,
    /// Whether this is the target's test build.
    pub test: bool,
    /// The lowercase hex SHA-256 of the unit's inputs, each sealed body replaced by a placeholder naming its item.
    pub skeleton: String,
    /// Every entry `skeleton` folds, by name, with its digest or value.
    pub entries: BTreeMap<String, String>,
}

/// One compiled unit and everything it read, with every path spelled by its class.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitSource {
    /// The package's name.
    pub package: String,
    /// The target's name.
    pub target: String,
    /// The target's kinds, comma-separated.
    pub kind: String,
    /// Whether this is the target's test build.
    pub test: bool,
    /// Every file its dep-info names, as `$root/<path>` or `$target/<path>`, and its bytes.
    pub files: BTreeMap<String, Vec<u8>>,
    /// Every variable the compilation recorded reading, and its value spelled portably.
    pub env: BTreeMap<String, String>,
    /// What each build script told it, as its closure entry and digest.
    pub emitted: BTreeMap<String, String>,
}

/// The evidence for `units` and the cataloged `items`, each with the reference the catalog gives it.
#[must_use]
pub fn evidence(units: &[UnitSource], items: &[(&Item, &ItemRef)]) -> Skeletons {
    let shadowing: Vec<Option<Unsealing>> = units.iter().map(unit_unsealing).collect();
    let mut verdicts: BTreeMap<&str, BTreeMap<(u32, u32), Option<Unsealing>>> = BTreeMap::new();
    let mut item_evidence = Vec::with_capacity(items.len());
    for (item, reference) in items {
        let name = format!("$root/{}", item.path);
        let reading: Vec<usize> = units
            .iter()
            .enumerate()
            .filter(|(_, unit)| unit.files.contains_key(&name))
            .map(|(position, _)| position)
            .collect();
        let source = reading
            .first()
            .and_then(|position| units.get(*position))
            .and_then(|unit| unit.files.get(&name));
        let body = source.and_then(|bytes| slice(bytes, item.body.start, item.body.end));
        let unsealed = match (source, body) {
            (None, _) => Some(Unsealing::Unread),
            (Some(_), None) => Some(Unsealing::Unlocated),
            (Some(_), Some(_))
                if item.measurable
                    && reading.iter().any(|position| {
                        units.get(*position).is_some_and(|unit| {
                            unit.kind
                                .split(',')
                                .any(|kind| kind == "proc-macro" || kind == "custom-build")
                        })
                    }) =>
            {
                Some(Unsealing::CompileTime)
            }
            (Some(bytes), Some(_)) if item.measurable => {
                let file = verdicts
                    .entry(item.path.as_str())
                    .or_insert_with(|| file_verdicts(bytes));
                match file.get(&(item.body.start, item.body.end)) {
                    None => Some(Unsealing::Unlocated),
                    Some(verdict) => verdict.clone().or_else(|| {
                        reading
                            .iter()
                            .find_map(|position| shadowing.get(*position).and_then(Clone::clone))
                    }),
                }
            }
            (Some(_), Some(_)) => Some(Unsealing::Evaluated),
        };
        item_evidence.push(ItemEvidence {
            index: item.index,
            item: (*reference).clone(),
            name: item.name.clone(),
            body_digest: crate::id::digest(body.unwrap_or_default()),
            sealed: unsealed.is_none(),
            unsealed,
        });
    }
    let mut unit_skeletons: Vec<UnitSkeleton> = units
        .iter()
        .map(|unit| {
            let entries = entries(unit, items, &item_evidence);
            UnitSkeleton {
                package: unit.package.clone(),
                target: unit.target.clone(),
                kind: unit.kind.clone(),
                test: unit.test,
                skeleton: folded(&entries),
                entries,
            }
        })
        .collect();
    unit_skeletons.sort();
    Skeletons {
        document_type: DOCUMENT_TYPE.to_owned(),
        schema_version: SCHEMA_VERSION,
        items: item_evidence,
        units: unit_skeletons,
    }
}

/// The bytes `[start, end)` of `bytes`, when they are there; a position no `usize` holds names no bytes.
fn slice(bytes: &[u8], start: u32, end: u32) -> Option<&[u8]> {
    match (usize::try_from(start), usize::try_from(end)) {
        (Ok(start), Ok(end)) => bytes.get(start..end),
        (Err(_does_not_fit), _) | (_, Err(_does_not_fit)) => None,
    }
}

/// A file as the Rust it holds, or nothing when it is not a whole Rust file, which is what a data file or an included expression is.
fn parsed(bytes: &[u8]) -> Option<(u32, syn::File)> {
    let text = match std::str::from_utf8(bytes) {
        Ok(text) => text,
        Err(_not_text) => return None,
    };
    let (base, rest) = match crate::syntax::strip_prefix(text) {
        Ok(split) => split,
        Err(_does_not_fit) => return None,
    };
    match syn::parse_str::<syn::File>(rest) {
        Ok(file) => Some((base, file)),
        Err(_not_a_rust_file) => None,
    }
}

/// What one unit's skeleton folds: its files with each sealed body replaced, its variables, and what its build scripts emitted.
fn entries(
    unit: &UnitSource,
    items: &[(&Item, &ItemRef)],
    evidence: &[ItemEvidence],
) -> BTreeMap<String, String> {
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    for (name, bytes) in &unit.files {
        let sealed: Vec<(u32, &Item)> = name
            .strip_prefix("$root/")
            .map(|path| {
                items
                    .iter()
                    .zip(evidence)
                    .filter(|((_, reference), said)| reference.path == path && said.sealed)
                    .map(|((item, reference), _)| (reference.ordinal, *item))
                    .collect()
            })
            .unwrap_or_default();
        entries.insert(
            name.clone(),
            crate::id::digest(&with_placeholders(bytes, name, &sealed)),
        );
    }
    for (name, value) in &unit.env {
        entries.insert(format!("$env/{name}"), value.clone());
    }
    for (name, digest) in &unit.emitted {
        entries.insert(name.clone(), digest.clone());
    }
    entries
}

/// One digest over every entry, as `<name>\0<digest>\n` in name order.
fn folded(entries: &BTreeMap<String, String>) -> String {
    let mut text = String::new();
    for (name, digest) in entries {
        text.push_str(name);
        text.push('\0');
        text.push_str(digest);
        text.push('\n');
    }
    crate::id::digest(text.as_bytes())
}

/// `bytes` with every sealed body replaced by `{sealed:<file>#<ordinal>}`.
fn with_placeholders(bytes: &[u8], name: &str, sealed: &[(u32, &Item)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut from = 0_usize;
    let mut ordered: Vec<&(u32, &Item)> = sealed.iter().collect();
    ordered.sort_by_key(|(_, item)| item.body.start);
    for (ordinal, item) in ordered {
        let (Ok(start), Ok(end)) = (
            usize::try_from(item.body.start),
            usize::try_from(item.body.end),
        ) else {
            continue;
        };
        let (Some(before), Some(body)) = (bytes.get(from..start), bytes.get(start..end)) else {
            continue;
        };
        out.extend_from_slice(before);
        out.extend_from_slice(format!("{{sealed:{name}#{ordinal}/{}}}", shape(body)).as_bytes());
        from = end;
    }
    out.extend_from_slice(bytes.get(from..).unwrap_or_default());
    out
}

/// Where a body leaves what follows it: its line breaks, and its last line's length in bytes and in characters.
fn shape(body: &[u8]) -> String {
    let mut lines = body.split(|byte| *byte == b'\n');
    let last = lines.next_back().unwrap_or_default();
    let newlines = lines.count();
    let characters = match std::str::from_utf8(last) {
        Ok(text) => text.chars().count().to_string(),
        Err(_not_text) => "bytes".to_owned(),
    };
    format!("{newlines}:{}:{characters}", last.len())
}

/// Why no body of this unit is sealed, when a file of it can rename a listed macro.
fn unit_unsealing(unit: &UnitSource) -> Option<Unsealing> {
    unit.files.values().find_map(|bytes| {
        let (_, file) = parsed(bytes)?;
        let mut declarations = Declarations::default();
        declarations.visit_file(&file);
        declarations.found
    })
}

/// What one file of a unit declares or imports that can rename a listed macro.
#[derive(Debug, Default)]
struct Declarations {
    found: Option<Unsealing>,
}

impl Declarations {
    fn found(&mut self, why: Unsealing) {
        if self.found.is_none() {
            self.found = Some(why);
        }
    }

    fn uses(&mut self, prefix: &mut Vec<String>, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                self.uses(prefix, &path.tree);
                prefix.pop();
            }
            syn::UseTree::Name(name) => self.imported(prefix, &name.ident.to_string()),
            syn::UseTree::Rename(rename) => self.imported(prefix, &rename.rename.to_string()),
            syn::UseTree::Glob(_) => {
                let root = prefix.first().map(String::as_str).unwrap_or_default();
                if !STANDARD_ROOTS.contains(&root) && !LOCAL_ROOTS.contains(&root) {
                    self.found(Unsealing::ForeignGlob {
                        path: prefix.join("::"),
                    });
                }
            }
            syn::UseTree::Group(group) => {
                for tree in &group.items {
                    self.uses(prefix, tree);
                }
            }
        }
    }

    fn imported(&mut self, prefix: &[String], visible: &str) {
        let root = prefix.first().map(String::as_str).unwrap_or_default();
        if SEALABLE_MACROS.contains(&visible) && !STANDARD_ROOTS.contains(&root) {
            self.found(Unsealing::Shadowed {
                name: visible.to_owned(),
            });
        }
    }
}

impl<'ast> Visit<'ast> for Declarations {
    fn visit_item_macro(&mut self, node: &'ast syn::ItemMacro) {
        if let Some(ident) = &node.ident {
            let name = ident.to_string();
            if SEALABLE_MACROS.contains(&name.as_str()) {
                self.found(Unsealing::Shadowed { name });
            }
        }
        visit::visit_item_macro(self, node);
    }

    fn visit_item_use(&mut self, node: &'ast syn::ItemUse) {
        let mut prefix = Vec::new();
        self.uses(&mut prefix, &node.tree);
    }

    fn visit_item_extern_crate(&mut self, node: &'ast syn::ItemExternCrate) {
        let name = node.ident.to_string();
        if node
            .attrs
            .iter()
            .any(|attribute| attribute.path().is_ident("macro_use"))
            && !STANDARD_ROOTS.contains(&name.as_str())
        {
            self.found(Unsealing::ForeignGlob { path: name });
        }
    }
}

/// Every function body of one file, by its byte span, and why it is not sealed, or nothing when it is.
fn file_verdicts(bytes: &[u8]) -> BTreeMap<(u32, u32), Option<Unsealing>> {
    let Some((base, file)) = parsed(bytes) else {
        return BTreeMap::new();
    };
    let mut bodies = Bodies {
        base,
        around: attributes(&file.attrs),
        verdicts: BTreeMap::new(),
    };
    bodies.visit_file(&file);
    bodies.verdicts
}

/// The first attribute off the list among `attrs`.
fn attributes(attrs: &[syn::Attribute]) -> Option<Unsealing> {
    attrs.iter().find_map(attribute)
}

/// Why this attribute unseals, or nothing when it is on the list.
fn attribute(attribute: &syn::Attribute) -> Option<Unsealing> {
    meta_path(attribute.path(), Some(&attribute.meta))
}

/// Why an attribute with this path unseals, looking inside a `cfg_attr` at what it would apply.
fn meta_path(path: &syn::Path, meta: Option<&syn::Meta>) -> Option<Unsealing> {
    let name = path_name(path);
    let first = path
        .segments
        .first()
        .map(|segment| segment.ident.to_string())
        .unwrap_or_default();
    let listed = path.segments.len() == 1 && SEALABLE_ATTRIBUTES.contains(&name.as_str());
    let tool = path.segments.len() > 1 && TOOL_NAMESPACES.contains(&first.as_str());
    if !listed && !tool {
        return Some(Unsealing::Attribute { name });
    }
    if name != "cfg_attr" {
        return None;
    }
    let Some(syn::Meta::List(list)) = meta else {
        return Some(Unsealing::Attribute { name });
    };
    let applied = match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(applied) => applied,
        Err(_not_attributes) => return Some(Unsealing::Attribute { name }),
    };
    applied
        .iter()
        .skip(1)
        .find_map(|inner| meta_path(inner.path(), Some(inner)))
}

/// A path as written, segments joined by `::`.
fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<String>>()
        .join("::")
}

/// The walk over one file that judges every function body it meets.
struct Bodies {
    base: u32,
    around: Option<Unsealing>,
    verdicts: BTreeMap<(u32, u32), Option<Unsealing>>,
}

impl Bodies {
    fn within(&mut self, attrs: &[syn::Attribute], walk: impl FnOnce(&mut Self)) {
        let outer = self.around.clone();
        if self.around.is_none() {
            self.around = attributes(attrs);
        }
        walk(self);
        self.around = outer;
    }

    fn function(
        &mut self,
        attrs: &[syn::Attribute],
        signature: &syn::Signature,
        block: &syn::Block,
    ) {
        self.within(attrs, |bodies| {
            let range = block.span().byte_range();
            let span = match (u32::try_from(range.start), u32::try_from(range.end)) {
                (Ok(start), Ok(end)) => bodies
                    .base
                    .checked_add(start)
                    .zip(bodies.base.checked_add(end)),
                (Err(_does_not_fit), _) | (_, Err(_does_not_fit)) => None,
            };
            let verdict = bodies
                .around
                .clone()
                .or_else(|| opaque(signature))
                .or_else(|| {
                    let mut body = Body::default();
                    body.visit_block(block);
                    body.found
                });
            if let Some(span) = span {
                bodies.verdicts.insert(span, verdict);
            }
            visit::visit_block(bodies, block);
        });
    }
}

impl<'ast> Visit<'ast> for Bodies {
    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        self.function(&node.attrs, &node.sig, &node.block);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        self.function(&node.attrs, &node.sig, &node.block);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if let Some(block) = &node.default {
            self.function(&node.attrs, &node.sig, block);
        }
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        self.within(&node.attrs, |bodies| visit::visit_item_impl(bodies, node));
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        self.within(&node.attrs, |bodies| visit::visit_item_trait(bodies, node));
    }

    fn visit_item_mod(&mut self, node: &'ast syn::ItemMod) {
        self.within(&node.attrs, |bodies| visit::visit_item_mod(bodies, node));
    }
}

/// Why a function with this signature has a type its body decides, or nothing when it has none.
fn opaque(signature: &syn::Signature) -> Option<Unsealing> {
    let mut returned = Returned::default();
    returned.visit_return_type(&signature.output);
    (signature.asyncness.is_some() || returned.opaque).then_some(Unsealing::OpaqueType)
}

/// The walk over a return type that finds an `impl` type in it.
#[derive(Debug, Default)]
struct Returned {
    opaque: bool,
}

impl<'ast> Visit<'ast> for Returned {
    fn visit_type_impl_trait(&mut self, _node: &'ast syn::TypeImplTrait) {
        self.opaque = true;
    }
}

/// The walk inside one body that finds the first thing it contributes beyond its own execution.
#[derive(Debug, Default)]
struct Body {
    found: Option<Unsealing>,
}

impl Body {
    fn found(&mut self, why: Unsealing) {
        if self.found.is_none() {
            self.found = Some(why);
        }
    }

    fn declares(&mut self, kind: &str) {
        self.found(Unsealing::DeclaresItem {
            kind: kind.to_owned(),
        });
    }
}

impl<'ast> Visit<'ast> for Body {
    fn visit_item(&mut self, node: &'ast syn::Item) {
        if let syn::Item::Verbatim(_) = node {
            self.declares("item");
        } else {
            visit::visit_item(self, node);
        }
    }

    fn visit_attribute(&mut self, node: &'ast syn::Attribute) {
        if let Some(why) = attribute(node) {
            self.found(why);
        }
    }

    fn visit_macro(&mut self, node: &'ast syn::Macro) {
        if let Some(why) = invoked(&node.path, &node.tokens) {
            self.found(why);
        }
    }

    fn visit_expr_const(&mut self, _node: &'ast syn::ExprConst) {
        self.found(Unsealing::ConstBlock);
    }

    fn visit_item_fn(&mut self, _node: &'ast syn::ItemFn) {
        self.declares("fn");
    }

    fn visit_item_struct(&mut self, _node: &'ast syn::ItemStruct) {
        self.declares("struct");
    }

    fn visit_item_enum(&mut self, _node: &'ast syn::ItemEnum) {
        self.declares("enum");
    }

    fn visit_item_union(&mut self, _node: &'ast syn::ItemUnion) {
        self.declares("union");
    }

    fn visit_item_impl(&mut self, _node: &'ast syn::ItemImpl) {
        self.declares("impl");
    }

    fn visit_item_trait(&mut self, _node: &'ast syn::ItemTrait) {
        self.declares("trait");
    }

    fn visit_item_trait_alias(&mut self, _node: &'ast syn::ItemTraitAlias) {
        self.declares("trait");
    }

    fn visit_item_type(&mut self, _node: &'ast syn::ItemType) {
        self.declares("type");
    }

    fn visit_item_use(&mut self, _node: &'ast syn::ItemUse) {
        self.declares("use");
    }

    fn visit_item_mod(&mut self, _node: &'ast syn::ItemMod) {
        self.declares("mod");
    }

    fn visit_item_macro(&mut self, _node: &'ast syn::ItemMacro) {
        self.declares("macro");
    }

    fn visit_item_const(&mut self, _node: &'ast syn::ItemConst) {
        self.declares("const");
    }

    fn visit_item_static(&mut self, _node: &'ast syn::ItemStatic) {
        self.declares("static");
    }

    fn visit_item_extern_crate(&mut self, _node: &'ast syn::ItemExternCrate) {
        self.declares("extern crate");
    }

    fn visit_item_foreign_mod(&mut self, _node: &'ast syn::ItemForeignMod) {
        self.declares("extern");
    }
}

/// Why invoking the macro at `path` with `tokens` unseals, or nothing when it and every macro inside its arguments are on the list.
fn invoked(path: &syn::Path, tokens: &TokenStream) -> Option<Unsealing> {
    let segments: Vec<String> = path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    sealable(&segments).map_or_else(|| scanned(tokens.clone()), Some)
}

/// Why a macro with these path segments unseals, or nothing when it names a listed standard macro.
fn sealable(segments: &[String]) -> Option<Unsealing> {
    let listed = match segments {
        [name] => SEALABLE_MACROS.contains(&name.as_str()),
        [root, name] => {
            STANDARD_ROOTS.contains(&root.as_str()) && SEALABLE_MACROS.contains(&name.as_str())
        }
        _ => false,
    };
    (!listed).then(|| Unsealing::Macro {
        name: segments.join("::"),
    })
}

/// Why a macro invoked anywhere inside these tokens unseals, or nothing when there is none that does.
fn scanned(tokens: TokenStream) -> Option<Unsealing> {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut path: Vec<String> = Vec::new();
    let mut joining = false;
    let mut position = 0_usize;
    while let Some(tree) = trees.get(position) {
        let next = trees.get(position.saturating_add(1));
        match tree {
            TokenTree::Ident(ident) => {
                let word = ident.to_string();
                if joining {
                    path.push(word);
                } else {
                    path = vec![word];
                }
                joining = false;
            }
            TokenTree::Punct(punct)
                if punct.as_char() == ':' && punct.spacing() == Spacing::Joint =>
            {
                joining = true;
                position = position.saturating_add(1);
            }
            TokenTree::Punct(punct) if punct.as_char() == '!' => {
                if let Some(TokenTree::Group(group)) = next
                    && group.delimiter() != Delimiter::None
                    && path
                        .last()
                        .is_some_and(|word| !KEYWORDS.contains(&word.as_str()))
                {
                    if let Some(why) = sealable(&path) {
                        return Some(why);
                    }
                    if let Some(why) = scanned(group.stream()) {
                        return Some(why);
                    }
                    position = position.saturating_add(1);
                }
                path.clear();
                joining = false;
            }
            TokenTree::Group(group) => {
                if let Some(why) = scanned(group.stream()) {
                    return Some(why);
                }
                path.clear();
                joining = false;
            }
            TokenTree::Punct(_) | TokenTree::Literal(_) => {
                path.clear();
                joining = false;
            }
        }
        position = position.saturating_add(1);
    }
    None
}
