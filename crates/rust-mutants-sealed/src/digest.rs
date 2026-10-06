// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Content digests, and the one canonical encoding every digest of this crate is taken over.

use std::fmt;

use sha2::{Digest as _, Sha256};

/// A SHA-256 digest of content this crate addressed.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct SealedDigest([u8; 32]);

impl SealedDigest {
    /// The digest of `bytes` as they are.
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        Self(Sha256::digest(bytes).into())
    }

    /// The thirty-two bytes of the digest.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The first eight bytes of the digest as a little-endian number.
    pub(crate) const fn leading_number(&self) -> u64 {
        let [
            first,
            second,
            third,
            fourth,
            fifth,
            sixth,
            seventh,
            eighth,
            ..,
        ] = self.0;
        u64::from_le_bytes([first, second, third, fourth, fifth, sixth, seventh, eighth])
    }
}

impl fmt::Display for SealedDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex::encode(self.0))
    }
}

/// The physical preparation identity, shared by leases and observed module work.
/// Operational cache directories select no bytes or semantic engine settings.
#[must_use]
pub fn preparation_key(module: &SealedDigest, configuration: &SealedDigest) -> SealedDigest {
    let mut key = Encoder::new("rust-mutants-sealed/preparation/v1");
    key.bytes(configuration.as_bytes()).bytes(module.as_bytes());
    key.finish()
}

/// A canonical encoding under a domain tag: every byte string as a netstring, every number at a fixed width.
pub(crate) struct Encoder(Sha256);

impl Encoder {
    /// Starts an encoding whose digest can collide with no other domain's.
    pub(crate) fn new(domain: &str) -> Self {
        let mut encoder = Self(Sha256::new());
        encoder.text(domain);
        encoder
    }

    /// Adds a byte string, its length spelled in decimal so no width of `usize` changes the encoding.
    pub(crate) fn bytes(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(bytes.len().to_string().as_bytes());
        self.0.update(b":");
        self.0.update(bytes);
        self.0.update(b",");
        self
    }

    /// Adds a text as its UTF-8 bytes.
    pub(crate) fn text(&mut self, text: &str) -> &mut Self {
        self.bytes(text.as_bytes())
    }

    /// Adds a number at eight bytes, little-endian.
    pub(crate) fn number(&mut self, value: u64) -> &mut Self {
        self.0.update(value.to_le_bytes());
        self
    }

    /// Adds a one-byte tag naming which case of a closed set follows.
    pub(crate) fn tag(&mut self, tag: u8) -> &mut Self {
        self.0.update([tag]);
        self
    }

    /// Adds a count of the items that follow, so two lists cannot be read as one.
    pub(crate) fn count(&mut self, count: usize) -> &mut Self {
        self.text(&count.to_string())
    }

    /// Adds a digest taken elsewhere.
    pub(crate) fn digest(&mut self, digest: &SealedDigest) -> &mut Self {
        self.0.update(digest.0);
        self
    }

    /// The digest of everything added so far, the encoding left open.
    pub(crate) fn peek(&self) -> SealedDigest {
        SealedDigest(self.0.clone().finalize().into())
    }

    /// The digest of everything added.
    pub(crate) fn finish(self) -> SealedDigest {
        SealedDigest(self.0.finalize().into())
    }
}
