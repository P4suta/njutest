// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The numbers WASI preview1 is written in: error numbers, file types, flags, rights, clocks and record layouts.

/// An error number a WASI call answers the guest with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum Errno {
    /// The call did what it was asked.
    Success,
    /// A descriptor that is not open, or not open for what was asked of it.
    Badf,
    /// A path that already names something.
    Exist,
    /// A pointer or a length outside the guest's memory.
    Fault,
    /// A file that would grow past what an offset can address.
    Fbig,
    /// A path that is not UTF-8.
    Ilseq,
    /// An argument the call cannot take.
    Inval,
    /// A directory where the call needs something else.
    Isdir,
    /// No descriptor number is left to give.
    Mfile,
    /// A buffer too short for the name it is asked to hold.
    Nametoolong,
    /// A path that names nothing.
    Noent,
    /// The overlay has no room left for what the guest would write.
    Nospc,
    /// A function this host does not carry out.
    Nosys,
    /// Something other than a directory where the call needs one.
    Notdir,
    /// A directory that still holds entries.
    Notempty,
    /// An operation a sealed host refuses to perform.
    Notsup,
    /// A value too large for the type it is returned in.
    Overflow,
    /// A position asked of a stream that has none.
    Spipe,
    /// A rename from one preopened tree into another.
    Xdev,
    /// A path that leaves the directory it is resolved from.
    Notcapable,
}

impl Errno {
    /// The number the guest receives.
    #[must_use]
    pub const fn number(self) -> u16 {
        match self {
            Self::Success => 0,
            Self::Badf => 8,
            Self::Exist => 20,
            Self::Fault => 21,
            Self::Fbig => 22,
            Self::Ilseq => 25,
            Self::Inval => 28,
            Self::Isdir => 31,
            Self::Mfile => 33,
            Self::Nametoolong => 37,
            Self::Noent => 44,
            Self::Nospc => 51,
            Self::Nosys => 52,
            Self::Notdir => 54,
            Self::Notempty => 55,
            Self::Notsup => 58,
            Self::Overflow => 61,
            Self::Spipe => 70,
            Self::Xdev => 75,
            Self::Notcapable => 76,
        }
    }

    /// The name WASI gives it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Success => "success",
            Self::Badf => "badf",
            Self::Exist => "exist",
            Self::Fault => "fault",
            Self::Fbig => "fbig",
            Self::Ilseq => "ilseq",
            Self::Inval => "inval",
            Self::Isdir => "isdir",
            Self::Mfile => "mfile",
            Self::Nametoolong => "nametoolong",
            Self::Noent => "noent",
            Self::Nospc => "nospc",
            Self::Nosys => "nosys",
            Self::Notdir => "notdir",
            Self::Notempty => "notempty",
            Self::Notsup => "notsup",
            Self::Overflow => "overflow",
            Self::Spipe => "spipe",
            Self::Xdev => "xdev",
            Self::Notcapable => "notcapable",
        }
    }
}

/// A file type this host never names more precisely: a stream that is neither a file nor a terminal.
pub(crate) const FILETYPE_UNKNOWN: u8 = 0;
/// A directory.
pub(crate) const FILETYPE_DIRECTORY: u8 = 3;
/// A regular file.
pub(crate) const FILETYPE_REGULAR_FILE: u8 = 4;

/// The realtime clock.
pub(crate) const CLOCK_REALTIME: u32 = 0;
/// The monotonic clock.
pub(crate) const CLOCK_MONOTONIC: u32 = 1;
/// The clock of the time the process has run.
pub(crate) const CLOCK_PROCESS_CPUTIME: u32 = 2;
/// The clock of the time the thread has run.
pub(crate) const CLOCK_THREAD_CPUTIME: u32 = 3;

/// Seek from the start of the file.
pub(crate) const WHENCE_SET: u32 = 0;
/// Seek from the current position.
pub(crate) const WHENCE_CUR: u32 = 1;
/// Seek from the end of the file.
pub(crate) const WHENCE_END: u32 = 2;

/// `path_open`: create the file when it is absent.
pub(crate) const OFLAGS_CREAT: u32 = 1;
/// `path_open`: fail unless the path is a directory.
pub(crate) const OFLAGS_DIRECTORY: u32 = 2;
/// `path_open`: fail when the path already names something.
pub(crate) const OFLAGS_EXCL: u32 = 4;
/// `path_open`: truncate the file to nothing.
pub(crate) const OFLAGS_TRUNC: u32 = 8;
/// Every `path_open` flag WASI defines.
pub(crate) const OFLAGS_ALL: u32 = OFLAGS_CREAT | OFLAGS_DIRECTORY | OFLAGS_EXCL | OFLAGS_TRUNC;

/// A descriptor whose writes go to the end of the file.
pub(crate) const FDFLAGS_APPEND: u32 = 1;
/// Every descriptor flag WASI defines: append, dsync, nonblock, rsync, sync.
pub(crate) const FDFLAGS_ALL: u32 = 0x1f;

/// `fst_flags`: set the access time to the value given.
pub(crate) const FSTFLAGS_ATIM: u32 = 1;
/// `fst_flags`: set the access time to now.
pub(crate) const FSTFLAGS_ATIM_NOW: u32 = 2;
/// `fst_flags`: set the modification time to the value given.
pub(crate) const FSTFLAGS_MTIM: u32 = 4;
/// `fst_flags`: set the modification time to now.
pub(crate) const FSTFLAGS_MTIM_NOW: u32 = 8;

/// A subscription's deadline is absolute rather than relative to now.
pub(crate) const SUBCLOCKFLAGS_ABSTIME: u32 = 1;
/// A subscription or event about a clock.
pub(crate) const EVENTTYPE_CLOCK: u8 = 0;
/// A subscription or event about a descriptor becoming readable.
pub(crate) const EVENTTYPE_FD_READ: u8 = 1;
/// A subscription or event about a descriptor becoming writable.
pub(crate) const EVENTTYPE_FD_WRITE: u8 = 2;
/// An event whose stream has reached its end.
pub(crate) const EVENTRWFLAGS_HANGUP: u16 = 1;

/// The right to read.
pub(crate) const RIGHTS_FD_READ: u64 = 1 << 1;
/// The right to move the position.
const RIGHTS_FD_SEEK: u64 = 1 << 2;
/// The right to ask the position.
const RIGHTS_FD_TELL: u64 = 1 << 5;
/// The right to write.
pub(crate) const RIGHTS_FD_WRITE: u64 = 1 << 6;
/// The right to advise how a file will be used.
const RIGHTS_FD_ADVISE: u64 = 1 << 7;
/// The right to allocate space in a file.
const RIGHTS_FD_ALLOCATE: u64 = 1 << 8;
/// The right to change a file's size through its descriptor.
const RIGHTS_FD_FILESTAT_SET_SIZE: u64 = 1 << 22;
/// The right to be polled for reading or writing.
const RIGHTS_POLL_FD_READWRITE: u64 = 1 << 27;
/// The rights only a socket carries.
const RIGHTS_SOCKET: u64 = (1 << 28) | (1 << 29);
/// Every right WASI defines.
pub(crate) const RIGHTS_ALL: u64 = (1 << 30) - 1;
/// The rights only a directory carries: every one that names a path, and reading entries.
const RIGHTS_PATHS: u64 = 0x071f_fe00;
/// The rights a regular file can carry.
pub(crate) const RIGHTS_FILE: u64 = RIGHTS_ALL & !RIGHTS_PATHS & !RIGHTS_SOCKET;
/// The rights a directory can carry: none of the ones that move bytes.
pub(crate) const RIGHTS_DIRECTORY: u64 = RIGHTS_ALL
    & !RIGHTS_SOCKET
    & !(RIGHTS_FD_READ
        | RIGHTS_FD_SEEK
        | RIGHTS_FD_TELL
        | RIGHTS_FD_WRITE
        | RIGHTS_FD_ADVISE
        | RIGHTS_FD_ALLOCATE
        | RIGHTS_FD_FILESTAT_SET_SIZE);
/// The rights standard input carries: reading, and polling for it.
pub(crate) const RIGHTS_STDIN: u64 = RIGHTS_FD_READ | RIGHTS_POLL_FD_READWRITE;
/// The rights standard output and standard error carry: writing, and polling for it.
pub(crate) const RIGHTS_STDOUT: u64 = RIGHTS_FD_WRITE | RIGHTS_POLL_FD_READWRITE;

/// The size of a `filestat` record.
pub(crate) const FILESTAT_SIZE: usize = 64;
/// The size of an `fdstat` record.
pub(crate) const FDSTAT_SIZE: usize = 24;
/// The size of the fixed part of a `dirent` record, before its name.
pub(crate) const DIRENT_SIZE: usize = 24;
/// The size of a `subscription` record.
pub(crate) const SUBSCRIPTION_SIZE: usize = 48;
/// The size of an `event` record.
pub(crate) const EVENT_SIZE: usize = 32;
/// The size of an `iovec` or `ciovec` record.
pub(crate) const IOVEC_SIZE: usize = 8;
