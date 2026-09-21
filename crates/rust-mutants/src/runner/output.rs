// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Capped capture of a child's combined output, keeping the tail.

use std::sync::Mutex;

/// How many bytes of combined output [`super::run`] keeps when the spec does not say. One mebibyte is far more than a readable test failure needs and far less than an unbounded logger can produce.
pub const DEFAULT_OUTPUT_LIMIT: usize = 1 << 20;

/// The smallest cap honoured. The truncation notice has to fit inside the budget for `output.len() <= limit` to hold.
pub const MIN_OUTPUT_LIMIT: usize = 256;

/// Begins the first line of an output that lost bytes. Stable, because reports quote it.
pub const OUTPUT_TRUNCATED_PREFIX: &str = "[rust-mutants] output truncated";

/// Captures the last `limit` bytes written to it and counts the rest.
#[derive(Debug)]
pub struct TailBuffer {
    limit: usize,
    state: Mutex<TailState>,
}

#[derive(Debug, Default)]
struct TailState {
    buf: Vec<u8>,
    total: u64,
}

/// Why a bounded output capture cannot continue or be read faithfully.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum OutputError {
    /// A capture callback panicked while it was changing the buffer.
    #[error("the output capture state is poisoned")]
    StatePoisoned,
    /// The exact byte count cannot be represented by the durable counter.
    #[error("output byte count overflowed its u64 counter")]
    ByteCountOverflow,
    /// The buffer's internal size relation was violated.
    #[error("the bounded output buffer violated its capacity invariant")]
    CapacityInvariant,
}

impl TailBuffer {
    /// A buffer keeping `limit` bytes, raised to [`MIN_OUTPUT_LIMIT`].
    #[must_use]
    pub fn new(limit: usize) -> Self {
        Self {
            limit: limit.max(MIN_OUTPUT_LIMIT),
            state: Mutex::new(TailState::default()),
        }
    }

    /// The effective limit.
    #[must_use]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Appends `bytes`, keeping only the tail.
    ///
    /// # Errors
    /// Refuses a poisoned capture state or a byte count that no longer fits
    /// the durable counter.
    pub fn write(&self, bytes: &[u8]) -> Result<(), OutputError> {
        let written =
            u64::try_from(bytes.len()).map_err(|_overflow| OutputError::ByteCountOverflow)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| OutputError::StatePoisoned)?;
        state.total = state
            .total
            .checked_add(written)
            .ok_or(OutputError::ByteCountOverflow)?;
        if bytes.len() >= self.limit {
            state.buf.clear();
            let start = bytes
                .len()
                .checked_sub(self.limit)
                .ok_or(OutputError::CapacityInvariant)?;
            let tail = bytes.get(start..).ok_or(OutputError::CapacityInvariant)?;
            state.buf.extend_from_slice(tail);
            drop(state);
            return Ok(());
        }
        state.buf.extend_from_slice(bytes);
        if state.buf.len() > self.limit {
            let drop_count = state
                .buf
                .len()
                .checked_sub(self.limit)
                .ok_or(OutputError::CapacityInvariant)?;
            state.buf.drain(..drop_count);
        }
        drop(state);
        Ok(())
    }

    /// The output as a result carries it: the bytes as written when nothing was lost, otherwise the truncation notice followed by as much of the tail as the remaining budget allows. `capture().len() <= limit` always.
    ///
    /// # Errors
    /// Refuses a poisoned capture state or an internal size relation that no
    /// longer preserves the configured cap.
    pub fn capture(&self) -> Result<Vec<u8>, OutputError> {
        let (total, buf) = {
            let state = self
                .state
                .lock()
                .map_err(|_poisoned| OutputError::StatePoisoned)?;
            (state.total, state.buf.clone())
        };
        let limit =
            u64::try_from(self.limit).map_err(|_overflow| OutputError::CapacityInvariant)?;
        if total <= limit {
            return Ok(buf);
        }
        let notice = truncation_notice(total);
        let keep = self
            .limit
            .checked_sub(notice.len())
            .ok_or(OutputError::CapacityInvariant)?;
        let start = if buf.len() > keep {
            buf.len()
                .checked_sub(keep)
                .ok_or(OutputError::CapacityInvariant)?
        } else {
            0
        };
        let tail = buf.get(start..).ok_or(OutputError::CapacityInvariant)?;
        let capacity = notice
            .len()
            .checked_add(tail.len())
            .ok_or(OutputError::CapacityInvariant)?;
        let mut out = Vec::with_capacity(capacity);
        out.extend_from_slice(notice.as_bytes());
        out.extend_from_slice(tail);
        Ok(out)
    }
}

/// The line prepended to a capped capture. It reports only the total the child produced, which the writer knows before it decides how much to keep.
#[must_use]
pub fn truncation_notice(total: u64) -> String {
    format!(
        "{OUTPUT_TRUNCATED_PREFIX}: the process produced {total} bytes, only the tail is kept\n"
    )
}

/// Keeps the first `limit` bytes of a stream and admits what it cut: the buffer for a child's structured stdout, where the head is the part that parses and a truncated tail would be a truncated document.
#[derive(Debug)]
pub struct HeadBuffer {
    limit: usize,
    state: Mutex<HeadState>,
}

#[derive(Debug, Default)]
struct HeadState {
    buf: Vec<u8>,
    total: u64,
    truncated: bool,
}

impl HeadBuffer {
    /// A buffer keeping at most `limit` bytes.
    #[must_use]
    pub const fn new(limit: usize) -> Self {
        Self {
            limit,
            state: Mutex::new(HeadState {
                buf: Vec::new(),
                total: 0,
                truncated: false,
            }),
        }
    }

    /// Appends `bytes`, keeping what still fits.
    ///
    /// # Errors
    /// Refuses a poisoned capture state, an exhausted byte counter, or a
    /// broken capacity invariant.
    pub fn write(&self, bytes: &[u8]) -> Result<(), OutputError> {
        let written =
            u64::try_from(bytes.len()).map_err(|_overflow| OutputError::ByteCountOverflow)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_poisoned| OutputError::StatePoisoned)?;
        state.total = state
            .total
            .checked_add(written)
            .ok_or(OutputError::ByteCountOverflow)?;
        let room = self
            .limit
            .checked_sub(state.buf.len())
            .ok_or(OutputError::CapacityInvariant)?;
        if bytes.len() > room {
            state.truncated = true;
        }
        let take = room.min(bytes.len());
        let kept = bytes.get(..take).ok_or(OutputError::CapacityInvariant)?;
        state.buf.extend_from_slice(kept);
        drop(state);
        Ok(())
    }

    /// The kept bytes, whether anything was cut, and the total written.
    ///
    /// # Errors
    /// Refuses a poisoned capture state.
    pub fn capture(&self) -> Result<(Vec<u8>, bool, u64), OutputError> {
        let state = self
            .state
            .lock()
            .map_err(|_poisoned| OutputError::StatePoisoned)?;
        Ok((state.buf.clone(), state.truncated, state.total))
    }
}
