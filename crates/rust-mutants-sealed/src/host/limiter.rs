// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The ceilings on a guest's memory and tables, and the record of what they refused.

use crate::transcript::Denials;

/// The most elements a table may grow to.
pub(crate) const TABLE_ELEMENTS: usize = 1 << 20;

/// The ceilings on the guest's memory and tables, and what they refused.
#[derive(Debug)]
pub(crate) struct Limiter {
    /// The most bytes the linear memory may hold.
    limit: u64,
    /// The largest the linear memory has been allowed to grow.
    pub(crate) peak: u64,
    /// What the ceilings refused.
    pub(crate) denials: Denials,
    /// Whether a growth the ceilings allowed failed on the host.
    pub(crate) reservation_failed: bool,
    /// Whether a count the limiter keeps outgrew its width.
    pub(crate) overflowed: bool,
}

impl Limiter {
    /// A limiter allowing `limit` bytes of linear memory.
    pub(crate) const fn new(limit: u64) -> Self {
        Self {
            limit,
            peak: 0,
            denials: Denials::new(),
            reservation_failed: false,
            overflowed: false,
        }
    }
}

impl wasmtime::ResourceLimiter for Limiter {
    fn memory_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if maximum.is_some_and(|maximum| desired > maximum) {
            return Ok(false);
        }
        let Ok(desired) = u64::try_from(desired) else {
            self.overflowed = true;
            return Err(wasmtime::Error::msg("a memory size outgrew 64 bits"));
        };
        if desired > self.limit {
            if self.denials.memory_refused(desired).is_err() {
                self.overflowed = true;
            }
            return Ok(false);
        }
        self.peak = self.peak.max(desired);
        Ok(true)
    }

    fn memory_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.reservation_failed = true;
        Err(error)
    }

    fn table_growing(
        &mut self,
        _current: usize,
        desired: usize,
        maximum: Option<usize>,
    ) -> wasmtime::Result<bool> {
        if maximum.is_some_and(|maximum| desired > maximum) {
            return Ok(false);
        }
        if desired > TABLE_ELEMENTS {
            if self.denials.table_refused().is_err() {
                self.overflowed = true;
            }
            return Ok(false);
        }
        Ok(true)
    }

    fn table_grow_failed(&mut self, error: wasmtime::Error) -> wasmtime::Result<()> {
        self.reservation_failed = true;
        Err(error)
    }

    fn instances(&self) -> usize {
        1
    }

    fn tables(&self) -> usize {
        16
    }

    fn memories(&self) -> usize {
        1
    }
}
