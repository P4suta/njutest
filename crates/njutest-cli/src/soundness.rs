// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The soundness inventory: every place a crate steps outside what the compiler guarantees.

use std::path::{Path, PathBuf};

use syn::spanned::Spanned as _;
use syn::visit::Visit;

/// What kind of place the compiler stops vouching for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, njutest_macros::AllVariants)]
pub enum Kind {
    /// An `unsafe { … }` block.
    Block,
    /// An `unsafe fn`.
    Function,
    /// An `unsafe trait`.
    Trait,
    /// An `unsafe impl`.
    Implementation,
    /// A `static mut`.
    StaticMut,
    /// An `extern` block, whose declarations the compiler takes on trust.
    ForeignBlock,
}

impl Kind {
    /// The canonical wire name.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Block => "unsafe-block",
            Self::Function => "unsafe-fn",
            Self::Trait => "unsafe-trait",
            Self::Implementation => "unsafe-impl",
            Self::StaticMut => "static-mut",
            Self::ForeignBlock => "extern-block",
        }
    }

    /// The kind with the given wire name, if any.
    #[must_use]
    #[cfg(feature = "testkit")]
    pub fn parse(name: &str) -> Option<Self> {
        #[cfg(feature = "testkit")]
        Self::ALL.into_iter().find(|kind| kind.name() == name)
    }
}

/// One place in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Item {
    /// The package that owns the file.
    pub package: String,
    /// The workspace-relative path.
    pub path: String,
    /// The 1-based line.
    pub line: u32,
    /// What kind of place it is.
    pub kind: Kind,
}

/// What one walk of a tree found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inventory {
    /// Every place found, in a fixed order.
    pub items: Vec<Item>,
    /// The packages that hold at least one, sorted.
    pub packages: Vec<String>,
    /// The files that could not be read as Rust this release understands. A file that was not read is not a file with nothing in it.
    pub unreadable: Vec<String>,
}

impl Inventory {
    /// Whether the compiler vouches for everything the inventory looked at.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

/// Why the inventory could not be taken.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SoundnessError {
    /// A file could not be read as Rust.
    #[error("{path}:{line}:{column}: {message}")]
    Unparsable {
        /// The file.
        path: String,
        /// The 1-based line.
        line: u32,
        /// The 1-based column.
        column: u32,
        /// What the parser said.
        message: String,
    },
    /// A directory could not be listed, or a file could not be read.
    #[error("reading {}: {source}", path.display())]
    Unreadable {
        /// What could not be read.
        path: PathBuf,
        /// The operating system's reason.
        #[source]
        source: std::io::Error,
    },
    /// A filesystem path cannot be represented in the report without changing it.
    #[error("filesystem path {path:?} contains the non-UTF-8 component {spelling:?}")]
    NonUtf8Path {
        /// The path whose bytes cannot be represented exactly.
        path: PathBuf,
        /// The exact platform component that failed conversion.
        spelling: std::ffi::OsString,
    },
    /// A discovered source escaped the root it was meant to be relative to.
    #[error("{path:?} is outside {root:?}: {source}")]
    OutsideRoot {
        /// The discovered source.
        path: PathBuf,
        /// The root it was expected to be under.
        root: PathBuf,
        /// The exact prefix mismatch.
        #[source]
        source: std::path::StripPrefixError,
    },
    /// A parser coordinate cannot fit the report's numeric domain.
    #[error("{path}: the {coordinate} coordinate does not fit u32: {source}")]
    CoordinateOutsideWire {
        /// The source whose coordinate failed.
        path: String,
        /// Which coordinate failed.
        coordinate: &'static str,
        /// The exact integer conversion failure.
        #[source]
        source: std::num::TryFromIntError,
    },
    /// Turning a zero-based parser coordinate into a one-based coordinate overflowed.
    #[error("{path}: the parser column cannot be converted to a one-based coordinate")]
    CoordinateArithmeticOverflow {
        /// The source whose coordinate failed.
        path: String,
    },
}

/// Every place in one file, in source order.
///
/// # Errors
/// Returns [`SoundnessError::Unparsable`] for a file this release cannot read
/// as Rust, which is not the same as a file with nothing in it.
pub fn of_source(path: &str, source: &str) -> Result<Vec<Item>, SoundnessError> {
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => {
            let span = error.span().start();
            let line = u32::try_from(span.line).map_err(|source| {
                SoundnessError::CoordinateOutsideWire {
                    path: path.to_owned(),
                    coordinate: "parser line",
                    source,
                }
            })?;
            let one_based_column = span.column.checked_add(1).ok_or_else(|| {
                SoundnessError::CoordinateArithmeticOverflow {
                    path: path.to_owned(),
                }
            })?;
            let column = u32::try_from(one_based_column).map_err(|source| {
                SoundnessError::CoordinateOutsideWire {
                    path: path.to_owned(),
                    coordinate: "parser column",
                    source,
                }
            })?;
            return Err(SoundnessError::Unparsable {
                path: path.to_owned(),
                line,
                column,
                message: error.to_string(),
            });
        }
    };
    let mut walker = Walker {
        path: path.to_owned(),
        found: Vec::new(),
        failure: None,
    };
    walker.visit_file(&file);
    if let Some(failure) = walker.failure {
        return Err(failure);
    }
    walker.found.sort();
    walker.found.dedup();
    Ok(walker.found)
}

/// Every place in every Rust file under `root` that belongs to one of `packages`, given as name and manifest directory.
///
/// # Errors
/// Returns [`SoundnessError::Unreadable`] when the tree itself cannot be
/// walked. A file that does not parse is recorded in
/// [`Inventory::unreadable`] rather than ending the walk, because one file
/// this release cannot read must not hide what every other file says.
pub fn inventory(root: &Path, packages: &[(String, PathBuf)]) -> Result<Inventory, SoundnessError> {
    let root = match std::fs::canonicalize(root) {
        Ok(canonical) => canonical,
        Err(_absent) => root.to_path_buf(),
    };
    let mut inventory = Inventory::default();
    for (name, directory) in packages {
        for (relative, source) in sources(&root, directory)? {
            match of_source(&relative, &source) {
                Ok(found) => inventory.items.extend(found.into_iter().map(|item| Item {
                    package: name.clone(),
                    ..item
                })),
                Err(_unparsable) => inventory.unreadable.push(relative),
            }
        }
    }
    inventory.items.sort();
    inventory.items.dedup();
    inventory.unreadable.sort();
    inventory.unreadable.dedup();
    inventory.packages = {
        let mut packages: Vec<String> = inventory
            .items
            .iter()
            .map(|item| item.package.clone())
            .collect();
        packages.sort();
        packages.dedup();
        packages
    };
    Ok(inventory)
}

/// Every `.rs` file under `directory`, as a workspace-relative path and its text.
fn sources(root: &Path, directory: &Path) -> Result<Vec<(String, String)>, SoundnessError> {
    let mut found = Vec::new();
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(SoundnessError::Unreadable {
                    path: current,
                    source,
                });
            }
        };
        for entry in entries {
            let entry = entry.map_err(|source| SoundnessError::Unreadable {
                path: current.clone(),
                source,
            })?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|source| SoundnessError::Unreadable {
                    path: path.clone(),
                    source,
                })?;
            let name = match entry.file_name().into_string() {
                Ok(name) => name,
                Err(spelling) => {
                    return Err(SoundnessError::NonUtf8Path { path, spelling });
                }
            };
            if kind.is_dir() {
                if !crate::evidence::tree::EXCLUDED_DIRECTORIES.contains(&name.as_str()) {
                    pending.push(path);
                }
                continue;
            }
            if !kind.is_file() || path.extension() != Some(std::ffi::OsStr::new("rs")) {
                continue;
            }
            let text =
                std::fs::read_to_string(&path).map_err(|source| SoundnessError::Unreadable {
                    path: path.clone(),
                    source,
                })?;
            let stripped =
                path.strip_prefix(root)
                    .map_err(|source| SoundnessError::OutsideRoot {
                        path: path.clone(),
                        root: root.to_path_buf(),
                        source,
                    })?;
            let mut parts = Vec::new();
            for part in stripped.components() {
                let text =
                    part.as_os_str()
                        .to_str()
                        .ok_or_else(|| SoundnessError::NonUtf8Path {
                            path: path.clone(),
                            spelling: part.as_os_str().to_owned(),
                        })?;
                parts.push(text.to_owned());
            }
            let relative = parts.join("/");
            found.push((relative, text));
        }
    }
    found.sort();
    Ok(found)
}

struct Walker {
    path: String,
    found: Vec<Item>,
    failure: Option<SoundnessError>,
}

impl Walker {
    fn record(&mut self, kind: Kind, line: usize) {
        match u32::try_from(line) {
            Ok(line) => self.found.push(Item {
                package: String::new(),
                path: self.path.clone(),
                line,
                kind,
            }),
            Err(source) => {
                if self.failure.is_none() {
                    self.failure = Some(SoundnessError::CoordinateOutsideWire {
                        path: self.path.clone(),
                        coordinate: "source line",
                        source,
                    });
                }
            }
        }
    }
}

impl<'ast> Visit<'ast> for Walker {
    fn visit_expr_unsafe(&mut self, node: &'ast syn::ExprUnsafe) {
        self.record(Kind::Block, node.unsafe_token.span().start().line);
        syn::visit::visit_expr_unsafe(self, node);
    }

    fn visit_item_fn(&mut self, node: &'ast syn::ItemFn) {
        if let syn::Safety::Unsafe(token) = node.sig.safety {
            self.record(Kind::Function, token.span().start().line);
        }
        syn::visit::visit_item_fn(self, node);
    }

    fn visit_impl_item_fn(&mut self, node: &'ast syn::ImplItemFn) {
        if let syn::Safety::Unsafe(token) = node.sig.safety {
            self.record(Kind::Function, token.span().start().line);
        }
        syn::visit::visit_impl_item_fn(self, node);
    }

    fn visit_trait_item_fn(&mut self, node: &'ast syn::TraitItemFn) {
        if let syn::Safety::Unsafe(token) = node.sig.safety {
            self.record(Kind::Function, token.span().start().line);
        }
        syn::visit::visit_trait_item_fn(self, node);
    }

    fn visit_item_trait(&mut self, node: &'ast syn::ItemTrait) {
        if let Some(token) = node.unsafety {
            self.record(Kind::Trait, token.span().start().line);
        }
        syn::visit::visit_item_trait(self, node);
    }

    fn visit_item_impl(&mut self, node: &'ast syn::ItemImpl) {
        if let Some(token) = node.unsafety {
            self.record(Kind::Implementation, token.span().start().line);
        }
        syn::visit::visit_item_impl(self, node);
    }

    fn visit_item_static(&mut self, node: &'ast syn::ItemStatic) {
        if matches!(node.mutability, syn::StaticMutability::Mut(_)) {
            self.record(Kind::StaticMut, node.static_token.span().start().line);
        }
        syn::visit::visit_item_static(self, node);
    }

    fn visit_item_foreign_mod(&mut self, node: &'ast syn::ItemForeignMod) {
        self.record(Kind::ForeignBlock, node.abi.span().start().line);
        syn::visit::visit_item_foreign_mod(self, node);
    }
}
