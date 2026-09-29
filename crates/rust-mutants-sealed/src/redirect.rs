// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A module rewritten so that the functions its name section names answer through a function it exports: how a sealed build gives the standard library an answer its platform layer does not have.

use std::collections::BTreeMap;
use std::ops::Range;

use wasmparser::{ExternalKind, FuncType, KnownCustom, Name, Parser, Payload, TypeRef};

use crate::error::SealedError;

/// The section a module's function bodies are in.
const CODE_SECTION: u8 = 10;

/// One function a module is rewritten to answer through another.
#[derive(Debug, Clone, Copy)]
pub struct Redirect {
    /// Whether a function whose name section name is this one is to be answered through [`Redirect::export`].
    pub names: fn(&str) -> bool,
    /// The function the module exports that answers in its place, with the same arguments and results.
    pub export: &'static str,
}

/// Whether `name` is the symbol of `std::env::<item>`, or of a copy of it the compiler made in another crate, in either of Rust's manglings.
#[must_use]
pub fn names_std_env(name: &str, item: &str) -> bool {
    let tail = format!("3std3env{}{item}", item.len());
    if let Some(rest) = name
        .strip_prefix("_ZN")
        .and_then(|rest| rest.strip_prefix(tail.as_str()))
    {
        return rest
            .strip_prefix("17h")
            .and_then(|rest| rest.strip_suffix('E'))
            .is_some_and(|hash| {
                hash.len() == 16 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            });
    }
    name.strip_prefix("_RNvNtCs")
        .and_then(|rest| rest.split_once('_'))
        .and_then(|(_disambiguator, rest)| rest.strip_prefix(tail.as_str()))
        .is_some_and(|rest| rest.is_empty() || rest.starts_with('C'))
}

/// What a module is before its bodies are rewritten: the functions it imports, each defined function's type, what it exports and what its name section names.
#[derive(Default)]
struct Read<'a> {
    imported: u32,
    types: Vec<FuncType>,
    defined: Vec<u32>,
    exports: BTreeMap<&'a str, u32>,
    names: BTreeMap<u32, &'a str>,
}

impl Read<'_> {
    /// The type of the function at `index`, imports first.
    fn type_of(&self, index: u32) -> Option<&FuncType> {
        let defined = index.checked_sub(self.imported)?;
        let type_index = self.defined.get(place(defined)?)?;
        self.types.get(place(*type_index)?)
    }
}

/// `value` as a place in memory, where it fits one.
fn place(value: u32) -> Option<usize> {
    match usize::try_from(value) {
        Ok(place) => Some(place),
        Err(_wider) => None,
    }
}

/// What `bytes` holds that a rewrite reads: the functions it imports, each defined function's type, its exported functions and the names its name section gives functions.
fn read(bytes: &[u8]) -> Result<Read<'_>, SealedError> {
    let malformed = |source| SealedError::ModuleMalformed { source };
    let mut read = Read::default();
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(malformed)?;
        if let Payload::TypeSection(section) = payload {
            for func_type in section.into_iter_err_on_gc_types() {
                read.types.push(func_type.map_err(malformed)?);
            }
        } else if let Payload::ImportSection(section) = payload {
            for import in section.into_imports() {
                if matches!(
                    import.map_err(malformed)?.ty,
                    TypeRef::Func(_) | TypeRef::FuncExact(_)
                ) {
                    read.imported = read
                        .imported
                        .checked_add(1)
                        .ok_or(SealedError::RedirectUnread)?;
                }
            }
        } else if let Payload::FunctionSection(section) = payload {
            for type_index in section {
                read.defined.push(type_index.map_err(malformed)?);
            }
        } else if let Payload::ExportSection(section) = payload {
            for export in section {
                let export = export.map_err(malformed)?;
                if export.kind == ExternalKind::Func {
                    read.exports.insert(export.name, export.index);
                }
            }
        } else if let Payload::CustomSection(custom) = payload
            && let KnownCustom::Name(names) = custom.as_known()
        {
            for name in names {
                if let Name::Function(map) = name.map_err(malformed)? {
                    for naming in map {
                        let naming = naming.map_err(malformed)?;
                        read.names.insert(naming.index, naming.name);
                    }
                }
            }
        }
    }
    Ok(read)
}

/// A module rewritten by [`redirected`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redirected {
    /// Its bytes.
    pub bytes: Vec<u8>,
    /// How many functions each redirect replaced, in the order given.
    pub counts: Vec<usize>,
}

/// `bytes` with the body of every function a redirect names replaced by a call to that redirect's export with the same arguments.
///
/// # Errors
/// [`SealedError::ModuleMalformed`] for bytes the parser cannot read, [`SealedError::RedirectUnexported`] where the module names a function to redirect and does not export what answers it, and [`SealedError::RedirectMismatched`] where the two have different types.
pub fn redirected(bytes: &[u8], redirects: &[Redirect]) -> Result<Redirected, SealedError> {
    let read = read(bytes)?;
    let mut bodies: BTreeMap<u32, Vec<u8>> = BTreeMap::new();
    let mut counts = Vec::with_capacity(redirects.len());
    for redirect in redirects {
        let targets: Vec<u32> = read
            .names
            .iter()
            .filter(|(index, name)| **index >= read.imported && (redirect.names)(name))
            .map(|(index, _name)| *index)
            .collect();
        counts.push(targets.len());
        if targets.is_empty() {
            continue;
        }
        let answering =
            *read
                .exports
                .get(redirect.export)
                .ok_or(SealedError::RedirectUnexported {
                    export: redirect.export,
                })?;
        for target in targets {
            let mismatched = || SealedError::RedirectMismatched {
                function: read
                    .names
                    .get(&target)
                    .map_or_else(String::new, |name| (*name).to_owned()),
                export: redirect.export,
            };
            let (Some(own), Some(answer)) = (read.type_of(target), read.type_of(answering)) else {
                return Err(mismatched());
            };
            if own.params() != answer.params()
                || own.results() != answer.results()
                || target == answering
            {
                return Err(mismatched());
            }
            let defined = target.checked_sub(read.imported).ok_or_else(mismatched)?;
            let arguments = u32::try_from(own.params().len()).map_err(|_wide| mismatched())?;
            bodies.insert(defined, calling(answering, arguments));
        }
    }
    let bytes = if bodies.is_empty() {
        bytes.to_vec()
    } else {
        rewritten(bytes, &bodies)?
    };
    Ok(Redirected { bytes, counts })
}

/// A function body that passes its `arguments` on to the function at `index` and answers what it answers.
fn calling(index: u32, arguments: u32) -> Vec<u8> {
    let mut body = vec![0_u8];
    for argument in 0..arguments {
        body.push(0x20);
        leb(&mut body, argument);
    }
    body.push(0x10);
    leb(&mut body, index);
    body.push(0x0b);
    body
}

/// `bytes` with the code section's bodies at the defined indices `bodies` names replaced.
fn rewritten(bytes: &[u8], bodies: &BTreeMap<u32, Vec<u8>>) -> Result<Vec<u8>, SealedError> {
    let unread = || SealedError::RedirectUnread;
    let (header, contents) = section(bytes, CODE_SECTION).ok_or_else(unread)?;
    let code = bytes.get(contents.clone()).ok_or_else(unread)?;
    let (count, mut at) = read_leb(code, 0).ok_or_else(unread)?;
    let mut section_contents = Vec::with_capacity(code.len());
    leb(&mut section_contents, count);
    for defined in 0..count {
        let (size, body_at) = read_leb(code, at).ok_or_else(unread)?;
        let end = body_at
            .checked_add(usize::try_from(size).map_err(|_wide| unread())?)
            .ok_or_else(unread)?;
        let body = code.get(body_at..end).ok_or_else(unread)?;
        let chosen = bodies.get(&defined).map_or(body, Vec::as_slice);
        leb(
            &mut section_contents,
            u32::try_from(chosen.len()).map_err(|_wide| unread())?,
        );
        section_contents.extend_from_slice(chosen);
        at = end;
    }
    if at != code.len() {
        return Err(unread());
    }
    let mut out = Vec::with_capacity(bytes.len());
    out.extend_from_slice(bytes.get(..header.start).ok_or_else(unread)?);
    out.push(CODE_SECTION);
    leb(
        &mut out,
        u32::try_from(section_contents.len()).map_err(|_wide| unread())?,
    );
    out.extend_from_slice(&section_contents);
    out.extend_from_slice(bytes.get(contents.end..).ok_or_else(unread)?);
    Ok(out)
}

/// Where the first section with id `id` is: the range from its id byte to its end, and the range of its contents.
fn section(bytes: &[u8], id: u8) -> Option<(Range<usize>, Range<usize>)> {
    let mut at = 8;
    while at < bytes.len() {
        let this = *bytes.get(at)?;
        let (size, contents_at) = read_leb(bytes, at.checked_add(1)?)?;
        let end = contents_at.checked_add(place(size)?)?;
        if this == id {
            return Some((at..end, contents_at..end));
        }
        at = end;
    }
    None
}

/// The unsigned LEB128 number at `at` in `bytes`, and where it ends, of at most the five bytes a `u32` takes.
fn read_leb(bytes: &[u8], at: usize) -> Option<(u32, usize)> {
    let mut value: u32 = 0;
    let mut shift: u32 = 0;
    let mut position = at;
    loop {
        let byte = *bytes.get(position)?;
        value |= u32::from(byte & 0x7f).checked_shl(shift)?;
        position = position.checked_add(1)?;
        if byte & 0x80 == 0 {
            return Some((value, position));
        }
        shift = shift.checked_add(7).filter(|next| *next < 35)?;
    }
}

/// Appends `value` to `out` as an unsigned LEB128 number.
fn leb(out: &mut Vec<u8>, value: u32) {
    let mut rest = value;
    loop {
        let [low, ..] = rest.to_le_bytes();
        let byte = low & 0x7f;
        rest >>= 7;
        if rest == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}
