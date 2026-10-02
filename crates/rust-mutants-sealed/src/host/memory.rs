// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The guest's linear memory as a host call reads and writes it: every access bounds-checked, and none past what the guest's fuel pays for.

use std::ops::Range;

use crate::abi::IOVEC_SIZE;

/// Why an access to the guest's memory was not made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Access {
    /// The range lies outside the memory, or its address wraps.
    Fault,
    /// The call has already touched as many bytes as the guest's fuel pays for.
    Unpaid,
}

/// The guest's memory for the length of one host call, and how many of its bytes the call has touched.
pub(crate) struct GuestMemory<'memory> {
    /// The linear memory.
    bytes: &'memory mut [u8],
    /// How many bytes the call has read or written.
    touched: u64,
    /// How many bytes the guest's fuel pays for.
    allowance: u64,
}

impl<'memory> GuestMemory<'memory> {
    /// The memory `bytes`, of which the call may touch `allowance`, nothing touched yet.
    pub(crate) const fn new(bytes: &'memory mut [u8], allowance: u64) -> Self {
        Self {
            bytes,
            touched: 0,
            allowance,
        }
    }

    /// How many bytes the call has read or written.
    pub(crate) const fn touched(&self) -> u64 {
        self.touched
    }

    /// The address `offset` bytes past `at`, refusing one that wraps.
    pub(crate) fn offset(at: u32, offset: usize) -> Result<u32, Access> {
        let offset = u32::try_from(offset).map_err(|_wide| Access::Fault)?;
        at.checked_add(offset).ok_or(Access::Fault)
    }

    /// The indices of `len` bytes at `at`, paid for and counted as touched, refusing a range outside the memory.
    fn range(&mut self, at: u32, len: usize) -> Result<Range<usize>, Access> {
        let start = usize::try_from(at).map_err(|_wide| Access::Fault)?;
        let end = start.checked_add(len).ok_or(Access::Fault)?;
        if end > self.bytes.len() {
            return Err(Access::Fault);
        }
        let len = u64::try_from(len).map_err(|_wide| Access::Fault)?;
        let touched = self.touched.checked_add(len).ok_or(Access::Unpaid)?;
        if touched > self.allowance {
            return Err(Access::Unpaid);
        }
        self.touched = touched;
        Ok(start..end)
    }

    /// The `len` bytes at `at`.
    pub(crate) fn read(&mut self, at: u32, len: usize) -> Result<&[u8], Access> {
        let range = self.range(at, len)?;
        self.bytes.get(range).ok_or(Access::Fault)
    }

    /// The `len` bytes at `at`, to be written.
    pub(crate) fn window(&mut self, at: u32, len: usize) -> Result<&mut [u8], Access> {
        let range = self.range(at, len)?;
        self.bytes.get_mut(range).ok_or(Access::Fault)
    }

    /// Writes `data` at `at`.
    pub(crate) fn write(&mut self, at: u32, data: &[u8]) -> Result<(), Access> {
        self.window(at, data.len())?.copy_from_slice(data);
        Ok(())
    }

    /// The fixed-width value at `at`.
    fn read_array<const WIDTH: usize>(&mut self, at: u32) -> Result<[u8; WIDTH], Access> {
        let mut value = [0; WIDTH];
        value.copy_from_slice(self.read(at, WIDTH)?);
        Ok(value)
    }

    /// The byte at `at`.
    pub(crate) fn read_u8(&mut self, at: u32) -> Result<u8, Access> {
        let [value] = self.read_array::<1>(at)?;
        Ok(value)
    }

    /// The little-endian `u16` at `at`.
    pub(crate) fn read_u16(&mut self, at: u32) -> Result<u16, Access> {
        self.read_array(at).map(u16::from_le_bytes)
    }

    /// The little-endian `u32` at `at`.
    pub(crate) fn read_u32(&mut self, at: u32) -> Result<u32, Access> {
        self.read_array(at).map(u32::from_le_bytes)
    }

    /// The little-endian `u64` at `at`.
    pub(crate) fn read_u64(&mut self, at: u32) -> Result<u64, Access> {
        self.read_array(at).map(u64::from_le_bytes)
    }

    /// Writes `value` little-endian at `at`.
    pub(crate) fn write_u32(&mut self, at: u32, value: u32) -> Result<(), Access> {
        self.write(at, &value.to_le_bytes())
    }

    /// Writes `value` little-endian at `at`.
    pub(crate) fn write_u64(&mut self, at: u32, value: u64) -> Result<(), Access> {
        self.write(at, &value.to_le_bytes())
    }

    /// The `count` buffers of the `iovec` array at `at`, each as its address and length.
    pub(crate) fn buffers(&mut self, at: u32, count: u32) -> Result<Vec<(u32, u32)>, Access> {
        let mut buffers = Vec::new();
        for index in 0..count {
            let index = usize::try_from(index).map_err(|_wide| Access::Fault)?;
            let offset = index.checked_mul(IOVEC_SIZE).ok_or(Access::Fault)?;
            let record = Self::offset(at, offset)?;
            let address = self.read_u32(record)?;
            let len = self.read_u32(Self::offset(record, 4)?)?;
            buffers.push((address, len));
        }
        Ok(buffers)
    }

    /// Every byte the buffers name, in order.
    pub(crate) fn gather(&mut self, buffers: &[(u32, u32)]) -> Result<Vec<u8>, Access> {
        let mut gathered = Vec::new();
        for (address, len) in buffers {
            let len = usize::try_from(*len).map_err(|_wide| Access::Fault)?;
            gathered.extend_from_slice(self.read(*address, len)?);
        }
        Ok(gathered)
    }

    /// Writes `data` across the buffers in order, as far as they reach, and says how much was written.
    pub(crate) fn scatter(&mut self, buffers: &[(u32, u32)], data: &[u8]) -> Result<usize, Access> {
        let mut rest = data;
        let mut written = 0_usize;
        for (address, len) in buffers {
            if rest.is_empty() {
                break;
            }
            let len = usize::try_from(*len).map_err(|_wide| Access::Fault)?;
            let (now, later) = rest.split_at(len.min(rest.len()));
            self.write(*address, now)?;
            written = written.checked_add(now.len()).ok_or(Access::Fault)?;
            rest = later;
        }
        Ok(written)
    }

    /// The total length the buffers name.
    pub(crate) fn capacity(buffers: &[(u32, u32)]) -> Result<usize, Access> {
        buffers.iter().try_fold(0_usize, |total, (_address, len)| {
            let len = usize::try_from(*len).map_err(|_wide| Access::Fault)?;
            total.checked_add(len).ok_or(Access::Fault)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Access, GuestMemory};

    #[test]
    fn an_access_past_the_allowance_is_refused_before_it_is_made() {
        let mut bytes = vec![0_u8; 64];
        let mut memory = GuestMemory::new(&mut bytes, 10);
        assert_eq!(memory.write(0, &[1; 8]), Ok(()));
        assert_eq!(memory.write(8, &[2; 8]), Err(Access::Unpaid));
        assert_eq!(memory.touched(), 8, "the refused write touched nothing");
        assert_eq!(memory.read(8, 2).map(<[u8]>::to_vec), Ok(vec![0, 0]));
        assert_eq!(memory.touched(), 10);
        assert_eq!(memory.read_u8(0), Err(Access::Unpaid));
        assert_eq!(bytes.get(..10), Some(&[1, 1, 1, 1, 1, 1, 1, 1, 0, 0][..]));
    }

    #[test]
    fn an_access_outside_the_memory_is_a_fault_whatever_the_allowance() {
        let mut bytes = vec![0_u8; 16];
        let mut memory = GuestMemory::new(&mut bytes, u64::MAX);
        assert_eq!(memory.read(12, 8).map(<[u8]>::to_vec), Err(Access::Fault));
        assert_eq!(memory.read_u32(u32::MAX), Err(Access::Fault));
        assert_eq!(GuestMemory::offset(u32::MAX, 1), Err(Access::Fault));
        assert_eq!(memory.touched(), 0);
    }
}
