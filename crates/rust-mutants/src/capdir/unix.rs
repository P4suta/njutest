// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The capability directory on the POSIX `*at` calls.

use std::fs::File;
use std::io;
use std::path::Path;

use rustix::fs::{AtFlags, FileType, Mode, OFlags, RenameFlags};

use super::{Identity, Kind, Name, Privacy, Status};

const DIRECTORY: OFlags = OFlags::RDONLY
    .union(OFlags::CLOEXEC)
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW);

pub(super) fn open(path: &Path) -> io::Result<File> {
    rustix::fs::open(path, DIRECTORY, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
}

pub(super) fn open_dir(dir: &File, name: Name<'_>) -> io::Result<File> {
    rustix::fs::openat(dir, name.as_str(), DIRECTORY, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
}

pub(super) fn open_file(dir: &File, name: Name<'_>) -> io::Result<File> {
    rustix::fs::openat(
        dir,
        name.as_str(),
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(io::Error::from)
}

pub(super) fn create_file(dir: &File, name: Name<'_>) -> io::Result<File> {
    rustix::fs::openat(
        dir,
        name.as_str(),
        OFlags::WRONLY | OFlags::CLOEXEC | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(io::Error::from)
}

pub(super) fn create_private_dir(dir: &File, name: Name<'_>) -> io::Result<File> {
    rustix::fs::mkdirat(dir, name.as_str(), Mode::RWXU).map_err(io::Error::from)?;
    let made = open_dir(dir, name)?;
    let identity = file_status(&made)?.identity;
    match status_at(dir, name)? {
        Some(named) if named.identity == identity && named.kind == Kind::Directory => Ok(made),
        Some(_) | None => Err(io::Error::other(
            "the directory just made is not the one its name now holds",
        )),
    }
}

fn status_of(stat: &rustix::fs::Stat) -> io::Result<Status> {
    let kind = match FileType::from_raw_mode(stat.st_mode) {
        FileType::RegularFile => Kind::File,
        FileType::Directory => Kind::Directory,
        FileType::Symlink
        | FileType::Fifo
        | FileType::Socket
        | FileType::CharacterDevice
        | FileType::BlockDevice
        | FileType::Unknown => Kind::Other,
    };
    let volume = u64::try_from(stat.st_dev)
        .map_err(|_negative| io::Error::other("a device number is negative"))?;
    let object = u128::from(stat.st_ino);
    let len = u64::try_from(stat.st_size)
        .map_err(|_negative| io::Error::other("a file size is negative"))?;
    Ok(Status {
        identity: Identity { volume, object },
        kind,
        len,
    })
}

pub(super) fn file_status(file: &File) -> io::Result<Status> {
    let stat = rustix::fs::fstat(file).map_err(io::Error::from)?;
    status_of(&stat)
}

pub(super) fn status_at(dir: &File, name: Name<'_>) -> io::Result<Option<Status>> {
    match rustix::fs::statat(dir, name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => status_of(&stat).map(Some),
        Err(rustix::io::Errno::NOENT) => Ok(None),
        Err(errno) => Err(io::Error::from(errno)),
    }
}

pub(super) fn rename_noreplace(
    from_dir: &File,
    from: Name<'_>,
    to_dir: &File,
    to: Name<'_>,
) -> io::Result<()> {
    rustix::fs::renameat_with(
        from_dir,
        from.as_str(),
        to_dir,
        to.as_str(),
        RenameFlags::NOREPLACE,
    )
    .map_err(io::Error::from)
}

pub(super) fn rename_replace(
    from_dir: &File,
    from: Name<'_>,
    to_dir: &File,
    to: Name<'_>,
) -> io::Result<()> {
    rustix::fs::renameat(from_dir, from.as_str(), to_dir, to.as_str()).map_err(io::Error::from)
}

pub(super) fn remove(dir: &File, name: Name<'_>, directory: bool) -> io::Result<()> {
    let flags = if directory {
        AtFlags::REMOVEDIR
    } else {
        AtFlags::empty()
    };
    rustix::fs::unlinkat(dir, name.as_str(), flags).map_err(io::Error::from)
}

pub(super) fn sync(dir: &File) -> io::Result<()> {
    rustix::fs::fsync(dir).map_err(io::Error::from)
}

pub(super) fn entries(dir: &File) -> io::Result<Vec<String>> {
    let scan = open_dir_at_self(dir)?;
    let mut listing = rustix::fs::Dir::read_from(&scan).map_err(io::Error::from)?;
    let mut names = Vec::new();
    while let Some(entry) = listing.read() {
        let entry = entry.map_err(io::Error::from)?;
        let bytes = entry.file_name().to_bytes();
        if matches!(bytes, b"." | b"..") {
            continue;
        }
        let name = std::str::from_utf8(bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("a directory entry is not UTF-8: {error}"),
            )
        })?;
        names.push(name.to_owned());
    }
    Ok(names)
}

fn open_dir_at_self(dir: &File) -> io::Result<File> {
    rustix::fs::openat(dir, ".", DIRECTORY, Mode::empty())
        .map(File::from)
        .map_err(io::Error::from)
}

pub(super) fn privacy(dir: &File) -> io::Result<Privacy> {
    let stat = rustix::fs::fstat(dir).map_err(io::Error::from)?;
    if stat.st_uid != rustix::process::geteuid().as_raw() {
        return Ok(Privacy::ForeignOwner);
    }
    let others = Mode::RWXG | Mode::RWXO;
    if Mode::from_raw_mode(stat.st_mode).intersects(others) {
        Ok(Privacy::Wider)
    } else {
        Ok(Privacy::OwnerOnly)
    }
}

pub(super) fn restrict_to_owner(dir: &File) -> io::Result<()> {
    rustix::fs::fchmod(dir, Mode::RWXU).map_err(io::Error::from)
}
