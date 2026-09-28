// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The import table: every function of `wasi_snapshot_preview1`, by name and by signature.

use wasmtime::ValType;

/// The module every import of a sealed guest comes from.
pub const IMPORT_MODULE: &str = "wasi_snapshot_preview1";

/// One function of WASI preview1; the whole set is everything a sealed guest may import.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum WasiFunction {
    /// `args_get`: the arguments, as C strings.
    ArgsGet,
    /// `args_sizes_get`: how many arguments, and how many bytes they take.
    ArgsSizesGet,
    /// `environ_get`: the environment, as `NAME=value` C strings.
    EnvironGet,
    /// `environ_sizes_get`: how many variables, and how many bytes they take.
    EnvironSizesGet,
    /// `clock_res_get`: a clock's resolution.
    ClockResGet,
    /// `clock_time_get`: a clock's reading.
    ClockTimeGet,
    /// `fd_advise`: how a file will be read.
    FdAdvise,
    /// `fd_allocate`: space in a file.
    FdAllocate,
    /// `fd_close`: closes a descriptor.
    FdClose,
    /// `fd_datasync`: flushes a file's data.
    FdDatasync,
    /// `fd_fdstat_get`: a descriptor's type, flags and rights.
    FdFdstatGet,
    /// `fd_fdstat_set_flags`: a descriptor's flags.
    FdFdstatSetFlags,
    /// `fd_fdstat_set_rights`: drops rights from a descriptor.
    FdFdstatSetRights,
    /// `fd_filestat_get`: what a descriptor's file is.
    FdFilestatGet,
    /// `fd_filestat_set_size`: truncates or extends a file.
    FdFilestatSetSize,
    /// `fd_filestat_set_times`: a file's times, through its descriptor.
    FdFilestatSetTimes,
    /// `fd_pread`: reads at an offset.
    FdPread,
    /// `fd_prestat_get`: what a preopened descriptor is.
    FdPrestatGet,
    /// `fd_prestat_dir_name`: the guest path of a preopened directory.
    FdPrestatDirName,
    /// `fd_pwrite`: writes at an offset.
    FdPwrite,
    /// `fd_read`: reads at the position.
    FdRead,
    /// `fd_readdir`: a directory's entries.
    FdReaddir,
    /// `fd_renumber`: moves a descriptor onto another number.
    FdRenumber,
    /// `fd_seek`: moves the position.
    FdSeek,
    /// `fd_sync`: flushes a file.
    FdSync,
    /// `fd_tell`: the position.
    FdTell,
    /// `fd_write`: writes at the position.
    FdWrite,
    /// `path_create_directory`: makes a directory.
    PathCreateDirectory,
    /// `path_filestat_get`: what a path names.
    PathFilestatGet,
    /// `path_filestat_set_times`: a file's times, through its path.
    PathFilestatSetTimes,
    /// `path_link`: a hard link.
    PathLink,
    /// `path_open`: opens or creates a file or directory.
    PathOpen,
    /// `path_readlink`: a symbolic link's target.
    PathReadlink,
    /// `path_remove_directory`: removes an empty directory.
    PathRemoveDirectory,
    /// `path_rename`: moves a file or directory.
    PathRename,
    /// `path_symlink`: a symbolic link.
    PathSymlink,
    /// `path_unlink_file`: removes a file.
    PathUnlinkFile,
    /// `poll_oneoff`: waits for clocks and descriptors.
    PollOneoff,
    /// `proc_exit`: ends the process with a code.
    ProcExit,
    /// `proc_raise`: sends the process a signal.
    ProcRaise,
    /// `sched_yield`: lets another thread run.
    SchedYield,
    /// `random_get`: random bytes.
    RandomGet,
    /// `sock_accept`: accepts a connection.
    SockAccept,
    /// `sock_recv`: receives from a socket.
    SockRecv,
    /// `sock_send`: sends on a socket.
    SockSend,
    /// `sock_shutdown`: shuts a socket down.
    SockShutdown,
}

/// A WebAssembly number type a WASI signature is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scalar {
    /// A 32-bit integer: a pointer, a length, a descriptor, a small enumeration.
    I32,
    /// A 64-bit integer: an offset, a size, a timestamp, a set of rights.
    I64,
}

/// A 32-bit parameter, spelled short so the table reads as the specification does.
const W: Scalar = Scalar::I32;
/// A 64-bit parameter.
const L: Scalar = Scalar::I64;

impl Scalar {
    /// The wasmtime type this is.
    pub(crate) const fn value_type(self) -> ValType {
        match self {
            Self::I32 => ValType::I32,
            Self::I64 => ValType::I64,
        }
    }

    /// The scalar a wasmtime type is, where it is one.
    pub(crate) const fn of(value_type: &ValType) -> Option<Self> {
        match value_type {
            ValType::I32 => Some(Self::I32),
            ValType::I64 => Some(Self::I64),
            ValType::F32 | ValType::F64 | ValType::V128 | ValType::Ref(_) => None,
        }
    }
}

impl WasiFunction {
    /// The name the guest imports it by.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::ArgsGet => "args_get",
            Self::ArgsSizesGet => "args_sizes_get",
            Self::EnvironGet => "environ_get",
            Self::EnvironSizesGet => "environ_sizes_get",
            Self::ClockResGet => "clock_res_get",
            Self::ClockTimeGet => "clock_time_get",
            Self::FdAdvise => "fd_advise",
            Self::FdAllocate => "fd_allocate",
            Self::FdClose => "fd_close",
            Self::FdDatasync => "fd_datasync",
            Self::FdFdstatGet => "fd_fdstat_get",
            Self::FdFdstatSetFlags => "fd_fdstat_set_flags",
            Self::FdFdstatSetRights => "fd_fdstat_set_rights",
            Self::FdFilestatGet => "fd_filestat_get",
            Self::FdFilestatSetSize => "fd_filestat_set_size",
            Self::FdFilestatSetTimes => "fd_filestat_set_times",
            Self::FdPread => "fd_pread",
            Self::FdPrestatGet => "fd_prestat_get",
            Self::FdPrestatDirName => "fd_prestat_dir_name",
            Self::FdPwrite => "fd_pwrite",
            Self::FdRead => "fd_read",
            Self::FdReaddir => "fd_readdir",
            Self::FdRenumber => "fd_renumber",
            Self::FdSeek => "fd_seek",
            Self::FdSync => "fd_sync",
            Self::FdTell => "fd_tell",
            Self::FdWrite => "fd_write",
            Self::PathCreateDirectory => "path_create_directory",
            Self::PathFilestatGet => "path_filestat_get",
            Self::PathFilestatSetTimes => "path_filestat_set_times",
            Self::PathLink => "path_link",
            Self::PathOpen => "path_open",
            Self::PathReadlink => "path_readlink",
            Self::PathRemoveDirectory => "path_remove_directory",
            Self::PathRename => "path_rename",
            Self::PathSymlink => "path_symlink",
            Self::PathUnlinkFile => "path_unlink_file",
            Self::PollOneoff => "poll_oneoff",
            Self::ProcExit => "proc_exit",
            Self::ProcRaise => "proc_raise",
            Self::SchedYield => "sched_yield",
            Self::RandomGet => "random_get",
            Self::SockAccept => "sock_accept",
            Self::SockRecv => "sock_recv",
            Self::SockSend => "sock_send",
            Self::SockShutdown => "sock_shutdown",
        }
    }

    /// The function a guest imports by `name`, where the table holds one.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|function| function.name() == name)
    }

    /// The parameter types, in order, as the specification lays them out in core WebAssembly.
    pub(crate) const fn parameters(self) -> &'static [Scalar] {
        match self {
            Self::ArgsGet
            | Self::ArgsSizesGet
            | Self::EnvironGet
            | Self::EnvironSizesGet
            | Self::ClockResGet
            | Self::FdFdstatGet
            | Self::FdFdstatSetFlags
            | Self::FdFilestatGet
            | Self::FdPrestatGet
            | Self::FdRenumber
            | Self::FdTell
            | Self::RandomGet
            | Self::SockShutdown => &[W, W],
            Self::ClockTimeGet => &[W, L, W],
            Self::FdAdvise | Self::FdFilestatSetTimes => &[W, L, L, W],
            Self::FdAllocate | Self::FdFdstatSetRights => &[W, L, L],
            Self::FdClose | Self::FdDatasync | Self::FdSync | Self::ProcExit | Self::ProcRaise => {
                &[W]
            }
            Self::FdFilestatSetSize => &[W, L],
            Self::FdPread | Self::FdPwrite | Self::FdReaddir => &[W, W, W, L, W],
            Self::FdPrestatDirName
            | Self::PathCreateDirectory
            | Self::PathRemoveDirectory
            | Self::PathUnlinkFile
            | Self::SockAccept => &[W, W, W],
            Self::FdRead | Self::FdWrite | Self::PollOneoff => &[W, W, W, W],
            Self::FdSeek => &[W, L, W, W],
            Self::PathFilestatGet | Self::PathSymlink | Self::SockSend => &[W, W, W, W, W],
            Self::PathFilestatSetTimes => &[W, W, W, W, L, L, W],
            Self::PathLink => &[W, W, W, W, W, W, W],
            Self::PathOpen => &[W, W, W, W, W, L, L, W, W],
            Self::PathReadlink | Self::PathRename | Self::SockRecv => &[W, W, W, W, W, W],
            Self::SchedYield => &[],
        }
    }

    /// Whether the function answers with an error number; only `proc_exit` does not return at all.
    pub(crate) const fn answers(self) -> bool {
        !matches!(self, Self::ProcExit)
    }

    /// The parameter types as wasmtime types.
    pub(crate) fn parameter_types(self) -> impl Iterator<Item = ValType> {
        self.parameters().iter().map(|scalar| scalar.value_type())
    }

    /// The result types as wasmtime types: one error number, or nothing.
    pub(crate) fn result_types(self) -> impl Iterator<Item = ValType> {
        self.answers().then_some(ValType::I32).into_iter()
    }
}
