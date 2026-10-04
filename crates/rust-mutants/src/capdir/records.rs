// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Owned, initialized FFI storage, borrowed, bounds-checked Windows variable-length records, and the kind an entry's attributes name.

use std::ffi::c_void;
use std::io;

use super::Kind;

/// `FILE_ATTRIBUTE_DIRECTORY`.
pub(super) const DIRECTORY_ATTRIBUTE: u32 = 0x10;

/// `FILE_ATTRIBUTE_REPARSE_POINT`.
pub(super) const REPARSE_POINT_ATTRIBUTE: u32 = 0x400;

/// `IO_REPARSE_TAG_APPEXECLINK`, the tag of an app execution alias.
pub(super) const EXECUTION_ALIAS_TAG: u32 = 0x8000_001b;

/// What an entry is by the attributes and reparse tag `FILE_ATTRIBUTE_TAG_INFO` reports, its tag read only where its attributes say it has one.
pub(super) const fn kind(attributes: u32, reparse_tag: u32) -> Kind {
    if attributes & REPARSE_POINT_ATTRIBUTE != 0 {
        if reparse_tag == EXECUTION_ALIAS_TAG {
            Kind::ExecutionAlias
        } else {
            Kind::Other
        }
    } else if attributes & DIRECTORY_ATTRIBUTE != 0 {
        Kind::Directory
    } else {
        Kind::File
    }
}

/// Initialized storage with the alignment every Windows record here requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Buffer(Vec<u64>);

impl Buffer {
    pub(super) fn sized(length: usize) -> Self {
        Self(vec![0; length.div_ceil(size_of::<u64>())])
    }

    pub(super) fn from_bytes(bytes: &[u8]) -> Self {
        Self(
            bytes
                .chunks(8)
                .map(|chunk| {
                    let mut word = [0; 8];
                    for (target, byte) in word.iter_mut().zip(chunk) {
                        *target = *byte;
                    }
                    u64::from_ne_bytes(word)
                })
                .collect(),
        )
    }

    pub(super) fn capacity(&self) -> io::Result<u32> {
        u32::try_from(size_of_val(self.0.as_slice())).map_err(|_outside| broken())
    }

    pub(super) fn bytes(&self, used: usize) -> io::Result<Vec<u8>> {
        if used > size_of_val(self.0.as_slice()) {
            return Err(broken());
        }
        Ok(self
            .0
            .iter()
            .flat_map(|word| word.to_ne_bytes())
            .take(used)
            .collect())
    }

    pub(super) const fn pointer(&self) -> *mut c_void {
        self.0.as_ptr().cast_mut().cast()
    }

    pub(super) const fn output(&mut self) -> *mut c_void {
        self.0.as_mut_ptr().cast()
    }
}

/// A borrowed byte range whose fields and child records cannot escape its bounds or lifetime.
#[derive(Debug, Clone, Copy)]
pub(super) struct Record<'a>(&'a [u8]);

impl<'a> Record<'a> {
    pub(super) const fn new(bytes: &'a [u8]) -> Self {
        Self(bytes)
    }

    pub(super) fn range(self, at: usize, length: usize) -> io::Result<Self> {
        let end = at.checked_add(length).ok_or_else(broken)?;
        self.0.get(at..end).map(Self).ok_or_else(broken)
    }

    fn tail(self, at: usize) -> io::Result<Self> {
        self.0.get(at..).map(Self).ok_or_else(broken)
    }

    fn byte(self, at: usize) -> io::Result<u8> {
        self.0.get(at).copied().ok_or_else(broken)
    }

    fn array<const N: usize>(self, at: usize) -> io::Result<[u8; N]> {
        self.range(at, N)?.0.try_into().map_err(|_short| broken())
    }

    pub(super) fn word(self, at: usize) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.array(at)?))
    }

    fn short(self, at: usize) -> io::Result<u16> {
        Ok(u16::from_le_bytes(self.array(at)?))
    }

    pub(super) fn sid(self) -> io::Result<Self> {
        if self.byte(0)? != 1 {
            return Err(broken());
        }
        let count = self.byte(1)?;
        if count > 15 {
            return Err(broken());
        }
        let length = usize::from(count)
            .checked_mul(4)
            .and_then(|count| count.checked_add(8))
            .ok_or_else(broken)?;
        self.range(0, length)
    }

    pub(super) fn sid_words(self) -> io::Result<Vec<u32>> {
        let sid = self.sid()?;
        let (words, remainder) = sid.0.as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(broken());
        }
        Ok(words.iter().map(|word| u32::from_le_bytes(*word)).collect())
    }

    pub(super) fn token_sid(self, base: usize) -> io::Result<Self> {
        let address = usize::from_ne_bytes(self.array(0)?);
        let offset = address.checked_sub(base).ok_or_else(broken)?;
        if offset < size_of::<usize>() {
            return Err(broken());
        }
        self.tail(offset)?.sid()
    }
}

/// The only access-control entry privacy can admit, with its SID borrowed from that entry alone.
#[derive(Debug, Clone, Copy)]
pub(super) enum Ace<'a> {
    Allows { sid: Record<'a>, mask: u32 },
    Other,
}

/// A self-relative security descriptor whose owner and grants have already been bounded and validated.
#[derive(Debug)]
pub(super) struct Security<'a> {
    pub(super) owner: Record<'a>,
    pub(super) protected: bool,
    pub(super) grants: Option<Vec<Ace<'a>>>,
}

pub(super) fn security(bytes: &[u8]) -> io::Result<Security<'_>> {
    let record = Record::new(bytes);
    record.range(0, 20)?;
    let control = record.short(2)?;
    if record.byte(0)? != 1 || control & 0x8000 == 0 {
        return Err(broken());
    }
    let owner_at = usize::try_from(record.word(4)?).map_err(|_outside| broken())?;
    if owner_at < 20 {
        return Err(broken());
    }
    let owner = record.tail(owner_at)?.sid()?;
    let dacl_at = usize::try_from(record.word(16)?).map_err(|_outside| broken())?;
    let grants = if control & 4 == 0 || dacl_at == 0 {
        None
    } else {
        if dacl_at < 20 {
            return Err(broken());
        }
        Some(acl(record.tail(dacl_at)?)?)
    };
    Ok(Security {
        owner,
        protected: control & 0x1000 != 0,
        grants,
    })
}

fn acl(record: Record<'_>) -> io::Result<Vec<Ace<'_>>> {
    record.range(0, 8)?;
    if !matches!(record.byte(0)?, 2 | 4) {
        return Err(broken());
    }
    let length = usize::from(record.short(2)?);
    if length < 8 {
        return Err(broken());
    }
    let record = record.range(0, length)?;
    let mut at = 8_usize;
    let mut grants = Vec::new();
    for _entry in 0..record.short(4)? {
        let header = record.range(at, 4)?;
        let size = usize::from(header.short(2)?);
        if size < 4 || !size.is_multiple_of(4) {
            return Err(broken());
        }
        let ace = record.range(at, size)?;
        let grant = if header.byte(0)? == 0 {
            Ace::Allows {
                sid: ace.tail(8)?.sid()?,
                mask: ace.word(4)?,
            }
        } else {
            Ace::Other
        };
        grants.push(grant);
        at = at.checked_add(size).ok_or_else(broken)?;
    }
    Ok(grants)
}

pub(super) fn directory_names(bytes: &[u8]) -> io::Result<Vec<Vec<u16>>> {
    const NAME_AT: usize = 68;
    let mut batch = Record::new(bytes);
    let mut names = Vec::new();
    loop {
        let next = usize::try_from(batch.word(0)?).map_err(|_outside| broken())?;
        let record = if next == 0 {
            batch
        } else {
            if next < NAME_AT || !next.is_multiple_of(8) {
                return Err(broken());
            }
            batch.range(0, next)?
        };
        let length = usize::try_from(record.word(60)?).map_err(|_outside| broken())?;
        let (units, remainder) = record.range(NAME_AT, length)?.0.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err(broken());
        }
        names.push(units.iter().map(|unit| u16::from_le_bytes(*unit)).collect());
        if next == 0 {
            return Ok(names);
        }
        batch = batch.tail(next)?;
    }
}

pub(super) fn put(bytes: &mut [u8], at: usize, value: &[u8]) -> io::Result<()> {
    let end = at.checked_add(value.len()).ok_or_else(broken)?;
    bytes
        .get_mut(at..end)
        .ok_or_else(broken)?
        .copy_from_slice(value);
    Ok(())
}

fn broken() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "an OS record runs outside its owner or contradicts its layout",
    )
}

#[cfg(test)]
mod tests;
