// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The host calls that move bytes and records between the filesystem and the guest's memory.

use crate::abi::{DIRENT_SIZE, Errno, FDSTAT_SIZE, FILESTAT_SIZE};

use super::fs::{Filestat, Filesystem, OpenRequest};
use super::memory::GuestMemory;
use super::{Failure, Params};

/// `args_get` and `environ_get`: a pointer to each string at `pointers`, the strings themselves from `buffer`.
pub(crate) fn strings_get(
    memory: &mut GuestMemory<'_>,
    strings: &[Vec<u8>],
    pointers: u32,
    buffer: u32,
) -> Result<(), Failure> {
    let mut at = buffer;
    for (index, string) in strings.iter().enumerate() {
        let slot = GuestMemory::offset(pointers, index.checked_mul(4).ok_or(Errno::Fault)?)?;
        memory.write_u32(slot, at)?;
        memory.write(at, string)?;
        at = GuestMemory::offset(at, string.len())?;
    }
    Ok(())
}

/// `args_sizes_get` and `environ_sizes_get`: how many strings, and how many bytes they take with their NULs.
pub(crate) fn strings_sizes(
    memory: &mut GuestMemory<'_>,
    strings: &[Vec<u8>],
    count: u32,
    size: u32,
) -> Result<(), Failure> {
    let bytes = strings
        .iter()
        .try_fold(0_usize, |total, string| total.checked_add(string.len()))
        .ok_or(Errno::Overflow)?;
    memory.write_u32(
        count,
        u32::try_from(strings.len()).map_err(|_wide| Errno::Overflow)?,
    )?;
    memory.write_u32(size, u32::try_from(bytes).map_err(|_wide| Errno::Overflow)?)?;
    Ok(())
}

/// A path of `len` bytes at `at`, refusing one that is not UTF-8.
pub(crate) fn path(memory: &mut GuestMemory<'_>, at: u32, len: u32) -> Result<String, Failure> {
    let len = usize::try_from(len).map_err(|_wide| Errno::Fault)?;
    let bytes = memory.read(at, len)?.to_vec();
    String::from_utf8(bytes).map_err(|_not_utf8| Failure::Errno(Errno::Ilseq))
}

/// `fd_fdstat_get`: the descriptor's type, flags and rights.
pub(crate) fn fdstat(
    files: &Filesystem,
    memory: &mut GuestMemory<'_>,
    fd: u32,
    at: u32,
) -> Result<(), Failure> {
    let stat = files.fdstat(fd)?;
    let mut record = [0_u8; FDSTAT_SIZE];
    put(&mut record, 0, &[stat.filetype])?;
    put(&mut record, 2, &stat.flags.to_le_bytes())?;
    put(&mut record, 8, &stat.rights.to_le_bytes())?;
    put(&mut record, 16, &stat.inheriting.to_le_bytes())?;
    memory.write(at, &record)?;
    Ok(())
}

/// Writes a `filestat` record at `at`.
pub(crate) fn write_filestat(
    memory: &mut GuestMemory<'_>,
    at: u32,
    stat: &Filestat,
) -> Result<(), Failure> {
    let mut record = [0_u8; FILESTAT_SIZE];
    put(&mut record, 0, &stat.device.to_le_bytes())?;
    put(&mut record, 8, &stat.inode.to_le_bytes())?;
    put(&mut record, 16, &[stat.filetype])?;
    put(&mut record, 24, &1_u64.to_le_bytes())?;
    put(&mut record, 32, &stat.size.to_le_bytes())?;
    put(&mut record, 40, &stat.accessed.to_le_bytes())?;
    put(&mut record, 48, &stat.modified.to_le_bytes())?;
    put(&mut record, 56, &stat.modified.to_le_bytes())?;
    memory.write(at, &record)?;
    Ok(())
}

/// Copies `field` into `record` at `at`.
fn put(record: &mut [u8], at: usize, field: &[u8]) -> Result<(), Failure> {
    let end = at.checked_add(field.len()).ok_or(Errno::Fault)?;
    record
        .get_mut(at..end)
        .ok_or(Errno::Fault)?
        .copy_from_slice(field);
    Ok(())
}

/// `fd_prestat_get`: a preopened directory, and the length of its guest path.
pub(crate) fn prestat(
    files: &Filesystem,
    memory: &mut GuestMemory<'_>,
    fd: u32,
    at: u32,
) -> Result<(), Failure> {
    let name = files.preopened(fd)?;
    let len = u32::try_from(name.len()).map_err(|_wide| Errno::Overflow)?;
    let mut record = [0_u8; 8];
    put(&mut record, 4, &len.to_le_bytes())?;
    memory.write(at, &record)?;
    Ok(())
}

/// `fd_prestat_dir_name`: the guest path of a preopened directory.
pub(crate) fn prestat_name(
    files: &Filesystem,
    memory: &mut GuestMemory<'_>,
    params: &Params<'_>,
) -> Result<(), Failure> {
    let name = files.preopened(params.w(0)?)?;
    let room = usize::try_from(params.w(2)?).map_err(|_wide| Errno::Fault)?;
    if room < name.len() {
        return Err(Errno::Nametoolong.into());
    }
    memory.write(params.w(1)?, name.as_bytes())?;
    Ok(())
}

/// `fd_read`: bytes at the position, into the guest's buffers.
pub(crate) fn read(
    files: &mut Filesystem,
    memory: &mut GuestMemory<'_>,
    params: &Params<'_>,
) -> Result<(), Failure> {
    let buffers = memory.buffers(params.w(1)?, params.w(2)?)?;
    let capacity = GuestMemory::capacity(&buffers)?;
    let bytes = files.read(params.w(0)?, capacity)?;
    let read = memory.scatter(&buffers, &bytes)?;
    memory.write_u32(
        params.w(3)?,
        u32::try_from(read).map_err(|_wide| Errno::Overflow)?,
    )?;
    Ok(())
}

/// `fd_pread`: bytes at an offset, into the guest's buffers.
pub(crate) fn pread(
    files: &Filesystem,
    memory: &mut GuestMemory<'_>,
    params: &Params<'_>,
) -> Result<(), Failure> {
    let buffers = memory.buffers(params.w(1)?, params.w(2)?)?;
    let capacity = GuestMemory::capacity(&buffers)?;
    let bytes = files.pread(params.w(0)?, capacity, params.l(3)?)?;
    let read = memory.scatter(&buffers, &bytes)?;
    memory.write_u32(
        params.w(4)?,
        u32::try_from(read).map_err(|_wide| Errno::Overflow)?,
    )?;
    Ok(())
}

/// `fd_pwrite`: the guest's buffers, written at an offset.
pub(crate) fn pwrite(
    files: &mut Filesystem,
    memory: &mut GuestMemory<'_>,
    params: &Params<'_>,
) -> Result<(), Failure> {
    let buffers = memory.buffers(params.w(1)?, params.w(2)?)?;
    let data = memory.gather(&buffers)?;
    let written = files.pwrite(params.w(0)?, &data, params.l(3)?)?;
    memory.write_u32(
        params.w(4)?,
        u32::try_from(written).map_err(|_wide| Errno::Overflow)?,
    )?;
    Ok(())
}

/// `fd_readdir`: the entries from a cookie on, as many as the buffer holds, the last one cut where it runs out.
pub(crate) fn readdir(
    files: &Filesystem,
    memory: &mut GuestMemory<'_>,
    params: &Params<'_>,
) -> Result<(), Failure> {
    let room = usize::try_from(params.w(2)?).map_err(|_wide| Errno::Fault)?;
    let mut laid = Vec::new();
    for dirent in files.readdir(params.w(0)?, params.l(3)?)? {
        if laid.len() >= room {
            break;
        }
        let mut header = [0_u8; DIRENT_SIZE];
        put(&mut header, 0, &dirent.next.to_le_bytes())?;
        put(&mut header, 8, &dirent.inode.to_le_bytes())?;
        let len = u32::try_from(dirent.name.len()).map_err(|_wide| Errno::Overflow)?;
        put(&mut header, 16, &len.to_le_bytes())?;
        put(&mut header, 20, &[dirent.filetype])?;
        laid.extend_from_slice(&header);
        laid.extend_from_slice(dirent.name.as_bytes());
    }
    laid.truncate(room);
    memory.write(params.w(1)?, &laid)?;
    memory.write_u32(
        params.w(4)?,
        u32::try_from(laid.len()).map_err(|_wide| Errno::Overflow)?,
    )?;
    Ok(())
}

/// `path_open`: a new descriptor for what a path names, created where asked.
pub(crate) fn open(
    files: &mut Filesystem,
    memory: &mut GuestMemory<'_>,
    params: &Params<'_>,
) -> Result<(), Failure> {
    let path = path(memory, params.w(2)?, params.w(3)?)?;
    let request = OpenRequest {
        oflags: params.w(4)?,
        rights: params.l(5)?,
        inheriting: params.l(6)?,
        fdflags: params.w(7)?,
    };
    let fd = files.open(params.w(0)?, &path, request)?;
    memory.write_u32(params.w(8)?, fd)?;
    Ok(())
}
