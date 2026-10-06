// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guest's random bytes: SHA-256 of the seed and a block counter, so the stream is a function of the seed and of how much was read.

use crate::digest::Encoder;

/// The bytes one block of the stream holds.
const BLOCK: usize = 32;

/// A deterministic stream of bytes that looks random to the guest, drawn in order.
#[derive(Debug, Clone)]
pub(crate) struct RandomStream {
    /// The invocation's seed.
    seed: u64,
    /// The index of the next block to make.
    next: u64,
    /// The block being drawn from.
    block: [u8; BLOCK],
    /// How many bytes of `block` are drawn already.
    drawn: usize,
}

/// The stream drew more blocks than a counter can number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Exhausted;

impl RandomStream {
    /// The stream `seed` names, nothing drawn yet.
    pub(crate) const fn new(seed: u64) -> Self {
        Self {
            seed,
            next: 0,
            block: [0; BLOCK],
            drawn: BLOCK,
        }
    }

    /// Fills `out` with the next bytes of the stream.
    pub(crate) fn fill(&mut self, out: &mut [u8]) -> Result<(), Exhausted> {
        for byte in out {
            if self.drawn == BLOCK {
                self.refill()?;
            }
            let Some(drawn) = self.block.get(self.drawn) else {
                return Err(Exhausted);
            };
            *byte = *drawn;
            self.drawn = self.drawn.checked_add(1).ok_or(Exhausted)?;
        }
        Ok(())
    }

    /// Makes the next block.
    fn refill(&mut self) -> Result<(), Exhausted> {
        let mut encoder = Encoder::new("rust-mutants-sealed/random/v1");
        encoder.number(self.seed).number(self.next);
        self.block = *encoder.finish().as_bytes();
        self.next = self.next.checked_add(1).ok_or(Exhausted)?;
        self.drawn = 0;
        Ok(())
    }
}
