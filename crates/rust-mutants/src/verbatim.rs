// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Where the build reads a Rust source of the tree as text: each `include_str!` or `include_bytes!` that names one is pointed at a copy of it as it was copied, so a source the engine rewrites is still read as it was written.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use proc_macro2::{Delimiter, TokenStream, TokenTree};

use crate::parsing::{Parsing, ReadingError};

/// The kind of the trace note each pointed include writes.
pub const NOTE: &str = "verbatim";

/// One include of a Rust source of the tree, pointed at the copy of that source as it was copied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Kept {
    /// The source that reads it, relative to the tree.
    pub reader: String,
    /// The line the include is on.
    pub line: usize,
    /// The source it reads, relative to the tree.
    pub read: String,
    /// The copy it reads instead, beside the reader, relative to the tree.
    pub copy: String,
}

/// How an include names the file it reads.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Named {
    /// A path relative to the directory of the file that holds the include.
    Beside(String),
    /// `concat!(env!("CARGO_MANIFEST_DIR"), …)`: a path under the directory of the package that compiles the file.
    Package(String),
}

/// One include found in a text: the bytes its argument takes, how its first and last tokens are spelled, the line it is on, and what it names.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Found {
    start: usize,
    end: usize,
    spelled: (String, String),
    line: usize,
    named: Named,
}

/// Why the includes of a source could not be pointed at the sources as copied.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VerbatimError {
    /// A source of the build could not be read, or the copy of one written.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: String,
        /// What the filesystem said.
        #[source]
        source: std::io::Error,
    },
    /// A source of the build is not Rust tokens.
    #[error("{path}: {source}")]
    Reading {
        /// The file.
        path: String,
        /// What the reading said.
        #[source]
        source: ReadingError,
    },
    /// The bytes an include's argument was read at are not that argument, so pointing it elsewhere would write over something else.
    #[error("{path}:{line}: the include's argument is not where its reading placed it")]
    Misplaced {
        /// The file.
        path: String,
        /// The line of the include.
        line: usize,
    },
    /// The name of the copy does not fit where the include's argument stood, which every position of the file depends on keeping its length.
    #[error(
        "{path}:{line}: the copy's name does not fit in the bytes the include's argument takes on its first line"
    )]
    NoRoom {
        /// The file.
        path: String,
        /// The line of the include.
        line: usize,
    },
}

/// Every include of `text` whose argument names a file by a string: beside the file, or under its package's directory.
fn includes(parsing: &Parsing, text: &str) -> Result<Vec<Found>, ReadingError> {
    let (start, rust) = match text.strip_prefix('\u{feff}') {
        Some(rest) => ('\u{feff}'.len_utf8(), rest),
        None => (0, text),
    };
    let tokens = parsing.tokens(rust)?;
    let mut found = Vec::new();
    collect(tokens, start, &mut found);
    Ok(found)
}

/// Every include in `tokens`, however deep in its groups, with each place moved by `base`.
fn collect(tokens: TokenStream, base: usize, found: &mut Vec<Found>) {
    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        if let TokenTree::Group(group) = tree {
            collect(group.stream(), base, found);
        }
        let TokenTree::Ident(name) = tree else {
            continue;
        };
        if name != "include_str" && name != "include_bytes" {
            continue;
        }
        let bang = at.checked_add(1).and_then(|next| trees.get(next));
        let argument = at.checked_add(2).and_then(|next| trees.get(next));
        if let (Some(TokenTree::Punct(bang)), Some(TokenTree::Group(group))) = (bang, argument)
            && bang.as_char() == '!'
            && group.delimiter() != Delimiter::None
            && let Some(one) = argued(group.stream(), base)
        {
            found.push(one);
        }
    }
}

/// The trees of `tokens` but for the commas between them.
fn uncommaed(tokens: TokenStream) -> Vec<TokenTree> {
    tokens
        .into_iter()
        .filter(|tree| !matches!(tree, TokenTree::Punct(comma) if comma.as_char() == ','))
        .collect()
}

/// What an include's argument names and the bytes it takes, where it is a string or a string under the package's directory.
fn argued(argument: TokenStream, base: usize) -> Option<Found> {
    let trees = uncommaed(argument);
    let (first, last) = (trees.first()?, trees.last()?);
    let named = match trees.as_slice() {
        [TokenTree::Literal(path)] => Named::Beside(text_of(path)?),
        [
            TokenTree::Ident(concat),
            TokenTree::Punct(bang),
            TokenTree::Group(parts),
        ] if concat == "concat" && bang.as_char() == '!' => {
            Named::Package(under_package(parts.stream())?)
        }
        _ => return None,
    };
    let start = base.checked_add(first.span().byte_range().start)?;
    let end = base.checked_add(last.span().byte_range().end)?;
    Some(Found {
        start,
        end,
        spelled: (spelling(first), spelling(last)),
        line: first.span().start().line,
        named,
    })
}

/// How a tree is spelled at its edge in the source: a group by its delimiter, which is where it ends, and anything else as it is written.
fn spelling(tree: &TokenTree) -> String {
    match tree {
        TokenTree::Group(group) => match group.delimiter() {
            Delimiter::Parenthesis => ")".to_owned(),
            Delimiter::Bracket => "]".to_owned(),
            Delimiter::Brace => "}".to_owned(),
            Delimiter::None => String::new(),
        },
        TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => tree.to_string(),
    }
}

/// The path `concat!(env!("CARGO_MANIFEST_DIR"), "…")` names under the package's directory.
fn under_package(parts: TokenStream) -> Option<String> {
    match uncommaed(parts).as_slice() {
        [
            TokenTree::Ident(env),
            TokenTree::Punct(bang),
            TokenTree::Group(variable),
            TokenTree::Literal(rest),
        ] if env == "env"
            && bang.as_char() == '!'
            && variable.stream().to_string() == "\"CARGO_MANIFEST_DIR\"" =>
        {
            text_of(rest)
        }
        _ => None,
    }
}

/// The value of a string literal.
fn text_of(literal: &proc_macro2::Literal) -> Option<String> {
    if let syn::Lit::Str(text) = syn::Lit::new(literal.clone()) {
        Some(text.value())
    } else {
        None
    }
}

/// `path` relative to the tree, with every `.` dropped and every `..` folded, or nothing where it leaves the tree or is not relative.
fn folded(path: &Path) -> Option<String> {
    let mut parts: Vec<String> = Vec::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::Normal(name) => parts.push(name.to_str()?.to_owned()),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The file an include of `reader` names, relative to the tree, where it is a Rust source inside it.
fn read_by(reader: &str, package: Option<&str>, named: &Named) -> Option<String> {
    let joined = match named {
        Named::Beside(path) => Path::new(reader).parent()?.join(path),
        Named::Package(path) => Path::new(package?).join(match path.strip_prefix('/') {
            Some(under) => under,
            None => path,
        }),
    };
    folded(&joined).filter(|read| {
        Path::new(read)
            .extension()
            .is_some_and(|extension| extension == "rs")
    })
}

/// `argument`'s bytes with `literal` in place of its first ones and a space in place of every other byte but a line's end, so the file keeps its length and every line where it was.
fn padded(argument: &str, literal: &str) -> Option<String> {
    let room = match argument.find(['\n', '\r']) {
        Some(ending) => ending,
        None => argument.len(),
    };
    if literal.len() > room {
        return None;
    }
    let mut padded = String::with_capacity(argument.len());
    padded.push_str(literal);
    for (at, character) in argument.char_indices() {
        let end = at.checked_add(character.len_utf8())?;
        if end <= literal.len() {
            continue;
        }
        if at < literal.len() {
            padded.push_str(&" ".repeat(end.checked_sub(literal.len())?));
        } else if character == '\n' || character == '\r' {
            padded.push(character);
        } else {
            padded.push_str(&" ".repeat(character.len_utf8()));
        }
    }
    (padded.len() == argument.len()).then_some(padded)
}

/// What the build's sources read of the tree as text: each reader's text with the includes it holds and the file each reads, and every file read, as it was copied.
type Planned = (
    BTreeMap<String, (String, Vec<(Found, String)>)>,
    BTreeMap<String, Vec<u8>>,
);

/// Where `sources` read a Rust source of the tree under `root` as text, and what each file they read holds now.
///
/// # Errors
/// A source or a file it reads that cannot be read, and a source that is not Rust tokens.
fn planned(
    root: &Path,
    sources: &BTreeMap<String, Option<String>>,
) -> Result<Planned, VerbatimError> {
    let mut planned = BTreeMap::new();
    let mut copies: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    let mut absent: BTreeSet<String> = BTreeSet::new();
    for (reader, package) in sources {
        let bytes = std::fs::read(root.join(reader)).map_err(|source| VerbatimError::Io {
            path: reader.clone(),
            source,
        })?;
        if !mentions_an_include(&bytes) {
            continue;
        }
        let Ok(text) = String::from_utf8(bytes) else {
            continue;
        };
        let found = crate::parsing::apart(|parsing| includes(parsing, &text))
            .and_then(|found| found)
            .map_err(|source| VerbatimError::Reading {
                path: reader.clone(),
                source,
            })?;
        let mut reading = Vec::new();
        for one in found {
            let Some(target) = read_by(reader, package.as_deref(), &one.named) else {
                continue;
            };
            if absent.contains(&target) {
                continue;
            }
            if !copies.contains_key(&target) {
                match std::fs::read(root.join(&target)) {
                    Ok(bytes) => {
                        copies.insert(target.clone(), bytes);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        absent.insert(target);
                        continue;
                    }
                    Err(source) => {
                        return Err(VerbatimError::Io {
                            path: target,
                            source,
                        });
                    }
                }
            }
            reading.push((one, target));
        }
        if !reading.is_empty() {
            planned.insert(reader.clone(), (text, reading));
        }
    }
    Ok((planned, copies))
}

/// The name of the copy of `read` beside `reader`, which is a short hidden file nothing in the reader's directory holds yet, allocated once for each directory and file read.
///
/// # Errors
/// A name that cannot be looked at.
fn copy_beside(
    root: &Path,
    (reader, read): (&str, &str),
    named: &mut BTreeMap<(String, String), String>,
) -> Result<String, VerbatimError> {
    let directory = match reader.rsplit_once('/') {
        Some((directory, _file)) => format!("{directory}/"),
        None => String::new(),
    };
    if let Some(copy) = named.get(&(directory.clone(), read.to_owned())) {
        return Ok(copy.clone());
    }
    let taken: BTreeSet<&String> = named.values().collect();
    let mut number = 0_u32;
    loop {
        let copy = format!("{directory}.{number}");
        let free = match std::fs::symlink_metadata(root.join(&copy)) {
            Ok(_held) => false,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
            Err(source) => return Err(VerbatimError::Io { path: copy, source }),
        };
        if free && !taken.contains(&copy) {
            named.insert((directory, read.to_owned()), copy.clone());
            return Ok(copy);
        }
        number = number.checked_add(1).ok_or_else(|| VerbatimError::NoRoom {
            path: reader.to_owned(),
            line: 0,
        })?;
    }
}

/// Points every include of a Rust source of the tree at a copy of that source as it was copied, and says each one.
///
/// The copy stands beside the reader, and the reader keeps its length and its lines.
/// `sources` are the build's Rust sources relative to `root`, each with its package's directory relative to `root` where the package is inside it.
///
/// # Errors
/// A source that cannot be read or written, one that is not Rust tokens, an include whose argument is not where its reading placed it, and one whose argument has no room for the copy's name.
pub fn keep(
    root: &Path,
    sources: &BTreeMap<String, Option<String>>,
) -> Result<Vec<Kept>, VerbatimError> {
    let (planned, copies) = planned(root, sources)?;
    let mut named: BTreeMap<(String, String), String> = BTreeMap::new();
    let mut kept = Vec::new();
    for (reader, (mut text, mut found)) in planned {
        found.sort_by_key(|(one, _)| std::cmp::Reverse(one.start));
        for (one, read) in &found {
            let misplaced = || VerbatimError::Misplaced {
                path: reader.clone(),
                line: one.line,
            };
            let argument = text.get(one.start..one.end).ok_or_else(misplaced)?;
            if !(argument.starts_with(&one.spelled.0) && argument.ends_with(&one.spelled.1)) {
                return Err(misplaced());
            }
            let copy = copy_beside(root, (&reader, read), &mut named)?;
            let name = match copy.rsplit_once('/') {
                Some((_directory, name)) => name,
                None => copy.as_str(),
            };
            let pointer =
                padded(argument, &format!("{name:?}")).ok_or_else(|| VerbatimError::NoRoom {
                    path: reader.clone(),
                    line: one.line,
                })?;
            text.replace_range(one.start..one.end, &pointer);
            kept.push(Kept {
                reader: reader.clone(),
                line: one.line,
                read: read.clone(),
                copy,
            });
        }
        std::fs::write(root.join(&reader), text).map_err(|source| VerbatimError::Io {
            path: reader.clone(),
            source,
        })?;
    }
    for one in &kept {
        let bytes = copies
            .get(&one.read)
            .ok_or_else(|| VerbatimError::Misplaced {
                path: one.reader.clone(),
                line: one.line,
            })?;
        std::fs::write(root.join(&one.copy), bytes).map_err(|source| VerbatimError::Io {
            path: one.copy.clone(),
            source,
        })?;
    }
    kept.sort_by(|one, other| (&one.reader, one.line).cmp(&(&other.reader, other.line)));
    Ok(kept)
}

/// Whether `bytes` could hold an include, which is what reading a source as tokens is spent on.
fn mentions_an_include(bytes: &[u8]) -> bool {
    [b"include_str".as_slice(), b"include_bytes".as_slice()]
        .iter()
        .any(|name| bytes.windows(name.len()).any(|window| window == *name))
}

#[cfg(test)]
mod tests;
