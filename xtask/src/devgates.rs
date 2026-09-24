// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The seam ratchet (ADR 0001).

use std::collections::BTreeSet;
use std::fmt;

use syn::visit::Visit;

/// What kind of seam a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum SeamKind {
    /// `static mut`.
    StaticMut,
    /// A `static` whose type carries interior mutability.
    StaticInteriorMutability,
    /// A `thread_local!` declaration.
    ThreadLocal,
    /// `#[cfg(test)]` or `cfg!(test)` outside a `mod tests`.
    CfgTestOutsideTestsModule,
    /// A read of the process environment outside the composition root.
    ProcessEnvironmentRead,
    /// `std::process::exit` outside the composition root.
    ProcessExit,
    /// An import of test support from production code.
    TestkitImport,
}

impl SeamKind {
    /// What the ledger and a refusal call it.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::StaticMut => "static-mut",
            Self::StaticInteriorMutability => "static-interior-mutability",
            Self::ThreadLocal => "thread-local",
            Self::CfgTestOutsideTestsModule => "cfg-test-outside-tests-module",
            Self::ProcessEnvironmentRead => "process-environment-read",
            Self::ProcessExit => "process-exit",
            Self::TestkitImport => "testkit-import",
        }
    }

    /// The examples a scan must find as this kind before its silence about a tree is believed.
    #[must_use]
    pub const fn planted(self) -> &'static str {
        match self {
            Self::StaticMut => include_str!("../sentinels/seams/static-mut.planted"),
            Self::StaticInteriorMutability => {
                include_str!("../sentinels/seams/static-interior-mutability.planted")
            }
            Self::ThreadLocal => include_str!("../sentinels/seams/thread-local.planted"),
            Self::CfgTestOutsideTestsModule => {
                include_str!("../sentinels/seams/cfg-test-outside-tests-module.planted")
            }
            Self::ProcessEnvironmentRead => {
                include_str!("../sentinels/seams/process-environment-read.planted")
            }
            Self::ProcessExit => include_str!("../sentinels/seams/process-exit.planted"),
            Self::TestkitImport => include_str!("../sentinels/seams/testkit-import.planted"),
        }
    }

    fn parse(label: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.label() == label)
    }
}

impl fmt::Display for SeamKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// One seam the scan found.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Seam {
    /// The file, slash-separated and relative to the workspace root.
    pub path: String,
    /// What kind of seam it is.
    pub kind: SeamKind,
    /// The name of the item, or the spelling of the expression.
    pub name: String,
}

impl fmt::Display for Seam {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.path, self.kind, self.name)
    }
}

/// Scans one production source file.
///
/// # Errors
/// Returns the parse error when `source` is not Rust.
pub fn scan_source(path: &str, source: &str) -> Result<Vec<Seam>, syn::Error> {
    let file = syn::parse_file(source)?;
    let composition_root = path.rsplit('/').next() == Some("main.rs");
    let mut scanner = Scanner {
        path,
        composition_root,
        seams: BTreeSet::new(),
    };
    scanner.visit_file(&file);
    Ok(scanner.seams.into_iter().collect())
}

struct Scanner<'a> {
    path: &'a str,
    composition_root: bool,
    seams: BTreeSet<Seam>,
}

const INTERIOR_MUTABILITY: [&str; 8] = [
    "Mutex",
    "RwLock",
    "OnceLock",
    "LazyLock",
    "Cell",
    "RefCell",
    "UnsafeCell",
    "OnceCell",
];

const ENVIRONMENT_READS: [&str; 10] = [
    "var",
    "var_os",
    "vars",
    "vars_os",
    "args",
    "args_os",
    "current_dir",
    "current_exe",
    "home_dir",
    "temp_dir",
];

impl Scanner<'_> {
    fn record(&mut self, kind: SeamKind, name: impl Into<String>) {
        self.seams.insert(Seam {
            path: self.path.to_owned(),
            kind,
            name: name.into(),
        });
    }

    fn record_path_expression(&mut self, segments: &[String]) {
        if self.composition_root || segments.len() < 2 {
            return;
        }
        let Some(module_index) = segments.len().checked_sub(2) else {
            return;
        };
        let (Some(last), Some(module)) = (segments.last(), segments.get(module_index)) else {
            return;
        };
        if module == "env" && ENVIRONMENT_READS.contains(&last.as_str()) {
            self.record(SeamKind::ProcessEnvironmentRead, segments.join("::"));
        } else if module == "process" && last == "exit" {
            self.record(SeamKind::ProcessExit, segments.join("::"));
        }
    }
}

fn path_segments(path: &syn::Path) -> Vec<String> {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect()
}

fn is_cfg_test(attribute: &syn::Attribute) -> bool {
    attribute.path().is_ident("cfg")
        && matches!(&attribute.meta, syn::Meta::List(list) if list.tokens.to_string() == "test")
}

fn is_tests_module(item: &syn::Item) -> bool {
    matches!(item, syn::Item::Mod(module) if module.ident == "tests" || module.ident == "test")
}

fn item_name(item: &syn::Item) -> String {
    match item {
        syn::Item::Fn(item) => item.sig.ident.to_string(),
        syn::Item::Static(item) => item.ident.to_string(),
        syn::Item::Const(item) => item.ident.to_string(),
        syn::Item::Struct(item) => item.ident.to_string(),
        syn::Item::Enum(item) => item.ident.to_string(),
        syn::Item::Type(item) => item.ident.to_string(),
        syn::Item::Trait(item) => item.ident.to_string(),
        syn::Item::Mod(item) => item.ident.to_string(),
        syn::Item::Macro(item) => item
            .ident
            .as_ref()
            .map_or_else(|| "macro".to_owned(), ToString::to_string),
        _ => "item".to_owned(),
    }
}

fn type_has_interior_mutability(ty: &syn::Type) -> bool {
    struct Finder(bool);
    impl Visit<'_> for Finder {
        fn visit_path_segment(&mut self, segment: &syn::PathSegment) {
            let ident = segment.ident.to_string();
            if INTERIOR_MUTABILITY.contains(&ident.as_str()) || ident.starts_with("Atomic") {
                self.0 = true;
            }
            syn::visit::visit_path_segment(self, segment);
        }
    }
    let mut finder = Finder(false);
    finder.visit_type(ty);
    finder.0
}

fn thread_local_names(tokens: &proc_macro2::TokenStream) -> Vec<String> {
    let mut names = Vec::new();
    let mut previous_was_static = false;
    for token in tokens.clone() {
        if let proc_macro2::TokenTree::Ident(ident) = &token {
            if previous_was_static {
                names.push(ident.to_string());
            }
            previous_was_static = *ident == "static";
        } else {
            previous_was_static = false;
        }
    }
    names
}

fn use_paths(tree: &syn::UseTree, prefix: &mut Vec<String>, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => {
            prefix.push(path.ident.to_string());
            use_paths(&path.tree, prefix, out);
            prefix.pop();
        }
        syn::UseTree::Name(name) => {
            prefix.push(name.ident.to_string());
            out.push(prefix.join("::"));
            prefix.pop();
        }
        syn::UseTree::Rename(rename) => {
            prefix.push(rename.ident.to_string());
            out.push(prefix.join("::"));
            prefix.pop();
        }
        syn::UseTree::Glob(_) => {
            prefix.push("*".to_owned());
            out.push(prefix.join("::"));
            prefix.pop();
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                use_paths(item, prefix, out);
            }
        }
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        let attributes: &[syn::Attribute] = match item {
            syn::Item::Fn(i) => &i.attrs,
            syn::Item::Static(i) => &i.attrs,
            syn::Item::Const(i) => &i.attrs,
            syn::Item::Struct(i) => &i.attrs,
            syn::Item::Enum(i) => &i.attrs,
            syn::Item::Type(i) => &i.attrs,
            syn::Item::Trait(i) => &i.attrs,
            syn::Item::Impl(i) => &i.attrs,
            syn::Item::Mod(i) => &i.attrs,
            syn::Item::Use(i) => &i.attrs,
            syn::Item::Macro(i) => &i.attrs,
            _ => &[],
        };
        if attributes.iter().any(is_cfg_test) {
            if !is_tests_module(item) {
                self.record(SeamKind::CfgTestOutsideTestsModule, item_name(item));
            }
            return;
        }
        syn::visit::visit_item(self, item);
    }

    fn visit_item_static(&mut self, item: &'ast syn::ItemStatic) {
        if matches!(item.mutability, syn::StaticMutability::Mut(_)) {
            self.record(SeamKind::StaticMut, item.ident.to_string());
        } else if type_has_interior_mutability(&item.ty) {
            self.record(SeamKind::StaticInteriorMutability, item.ident.to_string());
        }
        syn::visit::visit_item_static(self, item);
    }

    fn visit_item_macro(&mut self, item: &'ast syn::ItemMacro) {
        if item.mac.path.is_ident("thread_local") {
            for name in thread_local_names(&item.mac.tokens) {
                self.record(SeamKind::ThreadLocal, name);
            }
        }
        syn::visit::visit_item_macro(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        let mut paths = Vec::new();
        use_paths(&item.tree, &mut Vec::new(), &mut paths);
        for path in paths {
            let segments: Vec<&str> = path.split("::").collect();
            if segments.first() == Some(&"njutest_devkit") || segments.contains(&"testkit") {
                self.record(SeamKind::TestkitImport, path);
            }
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        self.record_path_expression(&path_segments(&expression.path));
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_macro(&mut self, mac: &'ast syn::Macro) {
        if mac.path.is_ident("cfg") && mac.tokens.to_string() == "test" {
            self.record(SeamKind::CfgTestOutsideTestsModule, "cfg!(test)");
        } else if mac.path.is_ident("option_env") && !self.composition_root {
            self.record(SeamKind::ProcessEnvironmentRead, "option_env!");
        }
        syn::visit::visit_macro(self, mac);
    }
}

/// The gate's verdict when the scan and the ledger disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disagreement {
    /// Seams the tree has and the ledger does not name.
    pub unrecorded: Vec<Seam>,
    /// Ledger lines the tree no longer has.
    pub stale: Vec<Seam>,
}

impl fmt::Display for Disagreement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "devgates: the seam scan and xtask/seam_allowlist.txt disagree."
        )?;
        if !self.unrecorded.is_empty() {
            writeln!(f, "seams the ledger does not name:")?;
            for seam in &self.unrecorded {
                writeln!(f, "  {seam}")?;
            }
            writeln!(
                f,
                "A package-level variable a test overwrites forces every test in the package to \
                 run alone, and a read of the process environment or an exit outside the \
                 composition root hides a decision from the options that should carry it. Move the \
                 behaviour into an argument — an options field, a hooks value, or a trait object — \
                 and have the composition root (main.rs) read the environment. Adding a line to the \
                 ledger is a reviewed exception, not the fix (docs/adr/0001-seam-policy.md)."
            )?;
        }
        if !self.stale.is_empty() {
            writeln!(
                f,
                "ledger lines the tree no longer has (delete them so the ledger keeps shrinking):"
            )?;
            for seam in &self.stale {
                writeln!(f, "  {seam}")?;
            }
        }
        Ok(())
    }
}

/// Compares the scan against the ledger.
/// `Ok(())` means exact agreement.
///
/// # Errors
/// Returns every seam missing from either side.
pub fn compare(found: &[Seam], ledger: &[Seam]) -> Result<(), Disagreement> {
    let found: BTreeSet<&Seam> = found.iter().collect();
    let recorded: BTreeSet<&Seam> = ledger.iter().collect();
    let unrecorded: Vec<Seam> = found
        .difference(&recorded)
        .map(|seam| (*seam).clone())
        .collect();
    let stale: Vec<Seam> = recorded
        .difference(&found)
        .map(|seam| (*seam).clone())
        .collect();
    if unrecorded.is_empty() && stale.is_empty() {
        Ok(())
    } else {
        Err(Disagreement { unrecorded, stale })
    }
}

/// Parses the ledger: one `path:kind:name` per line, sorted, `#` comments and blank lines ignored.
///
/// # Errors
/// Returns the first malformed or out-of-order line.
pub fn parse_ledger(text: &str) -> Result<Vec<Seam>, LedgerError> {
    let mut seams: Vec<Seam> = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index.saturating_add(1);
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let mut parts = trimmed.splitn(3, ':');
        let (Some(path), Some(kind), Some(name)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(LedgerError {
                line,
                reason: format!("expected `path:kind:name`, found `{trimmed}`"),
            });
        };
        let Some(kind) = SeamKind::parse(kind) else {
            return Err(LedgerError {
                line,
                reason: format!("unknown seam kind `{kind}`"),
            });
        };
        let seam = Seam {
            path: path.to_owned(),
            kind,
            name: name.to_owned(),
        };
        if let Some(previous) = seams.last()
            && *previous >= seam
        {
            return Err(LedgerError {
                line,
                reason: format!("lines must be sorted and unique; `{seam}` follows `{previous}`"),
            });
        }
        seams.push(seam);
    }
    Ok(seams)
}

/// A ledger line that could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("seam ledger line {line}: {reason}")]
pub struct LedgerError {
    /// The 1-based line number.
    pub line: usize,
    /// What was wrong with it.
    pub reason: String,
}

impl crate::error::Coded for LedgerError {
    fn code(&self) -> crate::error::ErrorCode {
        crate::error::SEAM_LEDGER
    }
}
