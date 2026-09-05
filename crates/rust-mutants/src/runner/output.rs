// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Capped capture of a child's combined output, keeping the tail.

use std::sync::{Mutex, PoisonError};

/// How many bytes of combined output [`super::run`] keeps when the spec does not say. One mebibyte is far more than a readable test failure needs and far less than a runaway logger can produce.
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

    /// Appends `bytes`, keeping only the tail. Never fails: a capture that could error would make the child's own writes fail.
    pub fn write(&self, bytes: &[u8]) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.total = state
            .total
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if bytes.len() >= self.limit {
            state.buf.clear();
            state.buf.extend_from_slice(
                bytes
                    .get(bytes.len().saturating_sub(self.limit)..)
                    .unwrap_or(bytes),
            );
            return;
        }
        state.buf.extend_from_slice(bytes);
        if state.buf.len() > self.limit.saturating_mul(2) {
            let drop_count = state.buf.len().saturating_sub(self.limit);
            state.buf.drain(..drop_count);
        }
    }

    /// The output as a result carries it: the bytes as written when nothing was lost, otherwise the truncation notice followed by as much of the tail as the remaining budget allows. `capture().len() <= limit` always.
    #[must_use]
    pub fn capture(&self) -> Vec<u8> {
        let (total, buf) = {
            let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            (state.total, state.buf.clone())
        };
        if total <= u64::try_from(self.limit).unwrap_or(u64::MAX) {
            return buf;
        }
        let notice = truncation_notice(total);
        let keep = self.limit.saturating_sub(notice.len());
        let tail = buf.get(buf.len().saturating_sub(keep)..).unwrap_or(&buf);
        let mut out = Vec::with_capacity(notice.len().saturating_add(tail.len()));
        out.extend_from_slice(notice.as_bytes());
        out.extend_from_slice(tail);
        out
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
    pub fn write(&self, bytes: &[u8]) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.total = state
            .total
            .saturating_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        let room = self.limit.saturating_sub(state.buf.len());
        if bytes.len() > room {
            state.truncated = true;
        }
        state
            .buf
            .extend_from_slice(bytes.get(..room.min(bytes.len())).unwrap_or_default());
    }

    /// The kept bytes, whether anything was cut, and the total written.
    #[must_use]
    pub fn capture(&self) -> (Vec<u8>, bool, u64) {
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        (state.buf.clone(), state.truncated, state.total)
    }
}
