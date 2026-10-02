// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Why a sealed invocation could not be made or could not be believed, each with a stable code documented in `docs/errors.md`.

use std::fmt;
use std::time::Duration;

/// A stable, searchable identifier for one failure mode, with what it means and what to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ErrorCode {
    /// The code, e.g. `RS0001`.
    code: &'static str,
    /// One line saying what the code means.
    summary: &'static str,
    /// What to do about it.
    remedy: &'static str,
}

impl ErrorCode {
    /// The code, e.g. `RS0001`.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        self.code
    }

    /// One line saying what the code means.
    #[must_use]
    pub const fn summary(&self) -> &'static str {
        self.summary
    }

    /// What to do about it.
    #[must_use]
    pub const fn remedy(&self) -> &'static str {
        self.remedy
    }
}

/// Every failure mode of the sealed host, one variant per code, in code order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, njutest_macros::AllVariants)]
pub enum SealedCode {
    /// An argument a WASI guest cannot be given.
    ArgumentInvalid,
    /// An environment variable a WASI guest cannot be given.
    EnvironmentInvalid,
    /// A snapshot that cannot be built from what it was given.
    SnapshotInvalid,
    /// A preopened guest path that cannot be given.
    PreopenInvalid,
    /// A directory to start in no tree preopened before the root holds.
    WorkingDirectoryInvalid,
    /// A path to halt at that no tree the invocation is given holds a place for.
    HaltInvalid,
    /// Bytes that are not a WebAssembly binary.
    ModuleMalformed,
    /// A WebAssembly component rather than a core module.
    ModuleComponent,
    /// A memory sealed execution cannot run.
    ModuleMemory,
    /// An import outside the WASI preview1 table.
    ModuleImport,
    /// A module that is not a WASI command.
    ModuleEntry,
    /// A module the host cannot start in a directory.
    ModuleUnstartable,
    /// A module whose functions cannot be answered through the ones it exports.
    ModuleUnredirected,
    /// The owned compilation-cache preparation boundary could not be established.
    PreparationUnavailable,
    /// A wasmtime that cannot be configured deterministically.
    EngineUnavailable,
    /// A module wasmtime refused to compile.
    ModuleUncompiled,
    /// Host functions that could not be linked.
    HostUnlinked,
    /// A watchdog that could not be started.
    WatchdogUnavailable,
    /// A guest the wall-clock watchdog stopped.
    WatchdogExpired,
    /// A runtime that failed outside the guest.
    RuntimeFailed,
    /// A trap this version cannot classify.
    TrapUnclassified,
    /// A guest stopped because whoever ran it stopped.
    Interrupted,
    /// A guest whose own `chdir` refused the directory it was to start in.
    StartRefused,
    /// A host that broke an invariant of its own.
    HostInvariant,
}

impl SealedCode {
    /// The code, what it means, and what to do about it.
    #[must_use]
    #[expect(
        clippy::too_many_lines,
        reason = "one arm per code: the table is the function, and splitting it would split the one match that keeps it total"
    )]
    pub const fn error_code(self) -> ErrorCode {
        match self {
            Self::ArgumentInvalid => ErrorCode {
                code: "RS0001",
                summary: "an argument holds a NUL byte, which a WASI argument cannot carry",
                remedy: "pass the argument without the NUL byte: WASI hands every argument to the guest as a C string",
            },
            Self::EnvironmentInvalid => ErrorCode {
                code: "RS0002",
                summary: "an environment variable a WASI guest cannot be given: an empty name, a name holding `=` or NUL, a value holding NUL, or a name given twice",
                remedy: "name each variable once, without `=` or NUL in its name or NUL in its value",
            },
            Self::SnapshotInvalid => ErrorCode {
                code: "RS0003",
                summary: "a snapshot path that is not a relative path of named components, a path the snapshot would hold twice or as both a file and a directory, or a change to a path whose directory the snapshot does not hold",
                remedy: "give each file once, by a relative path of `/`-separated names with no `.`, `..`, empty or NUL-bearing component",
            },
            Self::PreopenInvalid => ErrorCode {
                code: "RS0004",
                summary: "a preopened guest path that is empty, holds a NUL byte, or names the place another preopen names, as wasi-libc reads the two",
                remedy: "preopen each snapshot once, at a nonempty guest path without NUL bytes; wasi-libc reads `/a`, `a` and `a/` as one place, and `/` as `.`, the working directory's",
            },
            Self::WorkingDirectoryInvalid => ErrorCode {
                code: "RS0005",
                summary: "a directory to start in that names no tree preopened before the root, or a directory that tree does not hold",
                remedy: "preopen the tree before the root, and name the directory by `/`-separated names below its root, or by nothing for the root itself",
            },
            Self::HaltInvalid => ErrorCode {
                code: "RS0006",
                summary: "a path to halt at that no tree the invocation is given holds a place for: below no tree's guest path, naming the tree itself, or holding an empty name, `.`, `..` or NUL",
                remedy: "name the halt by a tree's guest path and `/`-separated names below it, where a rename puts the file that ends the guest",
            },
            Self::ModuleMalformed => ErrorCode {
                code: "RS1001",
                summary: "the bytes are not a WebAssembly binary the parser can read",
                remedy: "give the bytes of a `.wasm` file a WebAssembly toolchain wrote; the message says where the parser stopped",
            },
            Self::ModuleComponent => ErrorCode {
                code: "RS1002",
                summary: "the bytes are a WebAssembly component, and only a core module runs sealed",
                remedy: "build the guest for `wasm32-wasip1`, which writes a core module, rather than for a component target",
            },
            Self::ModuleMemory => ErrorCode {
                code: "RS1003",
                summary: "the module's memory is not one sealed execution runs: 64-bit, shared, imported, of a custom page size, or not exactly one",
                remedy: "build the guest for `wasm32-wasip1` without the memory64, threads, or multi-memory features",
            },
            Self::ModuleImport => ErrorCode {
                code: "RS1004",
                summary: "the module imports something outside the WASI preview1 table, or a table function with another signature",
                remedy: "a sealed guest may import only functions of `wasi_snapshot_preview1`; the message names the import to remove",
            },
            Self::ModuleEntry => ErrorCode {
                code: "RS1005",
                summary: "the module is not a WASI command: no `_start` function of type () -> (), no exported memory, or a start section",
                remedy: "build a binary crate or a test harness for `wasm32-wasip1`, which exports `_start` and `memory`",
            },
            Self::ModuleUnredirected => ErrorCode {
                code: "RS1007",
                summary: "the module names a function to answer through one it exports, and does not export it, exports one of another type, or has a code section this version cannot rewrite",
                remedy: "link the guest with the object and the exports the engine's sealed build adds, which answer the standard library's temporary and home directories; the message names what is missing",
            },
            Self::ModuleUnstartable => ErrorCode {
                code: "RS1006",
                summary: "the module does not export the `chdir` and `malloc` of type (i32) -> i32 the host starts a guest in a directory through",
                remedy: "link the guest with `-C link-arg=--undefined=chdir -C link-arg=--export=chdir -C link-arg=--export=malloc`, as the engine's sealed build does",
            },
            Self::PreparationUnavailable => ErrorCode {
                code: "RS1008",
                summary: "the owned compilation-cache directory or preparation lease could not be used",
                remedy: "the message names the refused path; check its access and retain it until its producer process ends",
            },
            Self::EngineUnavailable => ErrorCode {
                code: "RS2001",
                summary: "wasmtime, its compiled-module cache, or the module preparation lock could not be configured or used",
                remedy: "the message names the refused setting, cache path or poisoned lock; check cache access and restart a runner whose preparation lock was poisoned",
            },
            Self::ModuleUncompiled => ErrorCode {
                code: "RS2002",
                summary: "wasmtime refused to compile the module under the deterministic feature set",
                remedy: "the module uses a WebAssembly feature sealed execution leaves off, such as relaxed SIMD; the message names it",
            },
            Self::HostUnlinked => ErrorCode {
                code: "RS2003",
                summary: "the WASI host functions could not be linked to the module",
                remedy: "this is a defect in this tool: every import the validator lets through is one the host links",
            },
            Self::WatchdogUnavailable => ErrorCode {
                code: "RS3001",
                summary: "the thread that advances the watchdog's epochs could not be started",
                remedy: "the operating system refused a thread; check the process and memory limits of this user, and run again",
            },
            Self::WatchdogExpired => ErrorCode {
                code: "RS3002",
                summary: "the wall-clock watchdog stopped the guest; nothing about the guest follows from it",
                remedy: "the fuel budget, not the watchdog, bounds a guest: run it again on a machine less loaded, or with a longer watchdog",
            },
            Self::RuntimeFailed => ErrorCode {
                code: "RS3003",
                summary: "the runtime failed outside the guest: an instantiation, a fuel account, or a memory reservation the host could not complete",
                remedy: "this machine could not give the guest what its limits allow; the message says which, and nothing about the guest follows from it",
            },
            Self::TrapUnclassified => ErrorCode {
                code: "RS3004",
                summary: "the guest stopped with a trap this version cannot classify",
                remedy: "this is a defect in this tool: every trap of the pinned wasmtime has a kind, so report the message",
            },
            Self::Interrupted => ErrorCode {
                code: "RS3005",
                summary: "the guest was stopped because whoever ran it stopped, as an interrupted run does; nothing about the guest follows from it",
                remedy: "nothing is wrong with the guest or the host: run it again to measure it",
            },
            Self::StartRefused => ErrorCode {
                code: "RS3006",
                summary: "the guest's own `chdir` refused the directory it was to start in, so it cannot start where it was asked to",
                remedy: "the directory is not one the guest's C library reaches through the preopens; the message names it, so report it with the preopens it was given",
            },
            Self::HostInvariant => ErrorCode {
                code: "RS9001",
                summary: "the host broke an invariant of its own",
                remedy: "this is a defect in this tool; the message says which invariant, so report it",
            },
        }
    }
}

/// Every code the sealed host reports, in code order.
#[must_use]
pub const fn error_codes() -> &'static [ErrorCode] {
    &ERROR_CODES
}

/// Every code, made once from [`SealedCode::ALL`].
#[expect(
    clippy::indexing_slicing,
    reason = "a const loop cannot iterate, and the bound is the length of the array it indexes"
)]
const ERROR_CODES: [ErrorCode; SealedCode::ALL.len()] = {
    let mut codes = [SealedCode::ALL[0].error_code(); SealedCode::ALL.len()];
    let mut at = 0;
    while at < codes.len() {
        codes[at] = SealedCode::ALL[at].error_code();
        at += 1;
    }
    codes
};

/// Why an invocation could not be made, or why what it did cannot be read as an answer about the guest.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum SealedError {
    /// An argument holds a NUL byte.
    #[error("argument {index} holds a NUL byte, which a WASI argument cannot carry")]
    ArgumentHoldsNul {
        /// Where it stands among the arguments, the program name at zero.
        index: usize,
    },
    /// An environment variable a guest cannot be given.
    #[error("the environment variable {name:?} cannot be given to a WASI guest: {fault}")]
    EnvironmentVariable {
        /// The variable's name.
        name: String,
        /// What is wrong with it.
        fault: EnvironmentFault,
    },
    /// A snapshot path that is not a path a snapshot can hold.
    #[error("the snapshot path {path:?} cannot be held: {fault}")]
    SnapshotPath {
        /// The path as given.
        path: String,
        /// What is wrong with it.
        fault: SnapshotFault,
    },
    /// A guest path that cannot be preopened.
    #[error("the guest path {path:?} cannot be preopened: {fault}")]
    Preopen {
        /// The guest path as given.
        path: String,
        /// What is wrong with it.
        fault: PreopenFault,
    },
    /// A directory to start in that cannot be started in.
    #[error("the directory {directory:?} of the tree at {tree:?} cannot be started in: {fault}")]
    WorkingDirectory {
        /// The path of the tree it names.
        tree: String,
        /// The directory as given.
        directory: String,
        /// What is wrong with it.
        fault: WorkingFault,
    },
    /// A path to halt at that no tree holds a place for.
    #[error("the guest path {path:?} to halt at is in no tree the invocation is given")]
    Halt {
        /// The path as given.
        path: String,
    },
    /// Bytes the WebAssembly parser could not read.
    #[error("the bytes are not a WebAssembly binary: {source}")]
    ModuleMalformed {
        /// Where and why the parser stopped.
        #[source]
        source: wasmparser::BinaryReaderError,
    },
    /// A component rather than a core module.
    #[error("the bytes are a WebAssembly component, and only a core module runs sealed")]
    ModuleComponent,
    /// A memory sealed execution does not run.
    #[error("the module's memory cannot run sealed: {fault}")]
    ModuleMemory {
        /// What is wrong with it.
        fault: MemoryFault,
    },
    /// An import outside the table.
    #[error("the module imports {module}::{name}, which {fault}")]
    ModuleImport {
        /// The module the import names.
        module: String,
        /// The name the import names.
        name: String,
        /// What is wrong with it.
        fault: ImportFault,
    },
    /// A module that is not a WASI command.
    #[error("the module is not a WASI command: {fault}")]
    ModuleEntry {
        /// What is missing.
        fault: EntryFault,
    },
    /// A module that names a function to redirect and does not export the one that answers it.
    #[error(
        "the module names a function to answer through its export `{export}`, which it does not have"
    )]
    RedirectUnexported {
        /// The export it does not have.
        export: &'static str,
    },
    /// A function whose type is not that of the export that is to answer it.
    #[error(
        "the module's {function} cannot be answered through its export `{export}`, whose type is another"
    )]
    RedirectMismatched {
        /// The function, as the module's name section names it.
        function: String,
        /// The export.
        export: &'static str,
    },
    /// A code section this version cannot rewrite.
    #[error("the module's code section cannot be rewritten to answer a function through an export")]
    RedirectUnread,
    /// A module that does not export what the host starts a guest in a directory through.
    #[error(
        "the module exports no `{export}` of type (i32) -> i32, which the host starts a guest in a directory through"
    )]
    StartUnexported {
        /// The export it does not have.
        export: &'static str,
    },
    /// The owned compilation-cache directory or preparation lease could not be used.
    #[error("the owned module preparation at {path:?} failed: {source}")]
    Preparation {
        /// The directory or keyed lease whose ownership could not be established.
        path: std::path::PathBuf,
        /// The operating-system failure.
        #[source]
        source: std::io::Error,
    },
    /// Wasmtime refused the deterministic configuration.
    #[error("wasmtime could not be configured deterministically: {source}")]
    Engine {
        /// What wasmtime said.
        #[source]
        source: wasmtime::Error,
    },
    /// Wasmtime refused to compile the module.
    #[error("wasmtime refused to compile the module: {source}")]
    Compile {
        /// What wasmtime said.
        #[source]
        source: wasmtime::Error,
    },
    /// The host functions could not be linked.
    #[error("the WASI host could not be linked to the module: {source}")]
    Link {
        /// What wasmtime said.
        #[source]
        source: wasmtime::Error,
    },
    /// The watchdog thread could not be started.
    #[error("the watchdog thread could not be started: {source}")]
    WatchdogUnavailable {
        /// What the operating system said.
        #[source]
        source: std::io::Error,
    },
    /// The wall-clock watchdog stopped the guest.
    #[error(
        "the wall-clock watchdog stopped the guest after {limit:?}; nothing about the guest follows from it"
    )]
    WatchdogExpired {
        /// How long the guest was allowed.
        limit: Duration,
    },
    /// The guest was stopped because whoever ran it stopped.
    #[error("the guest was interrupted; nothing about the guest follows from it")]
    Interrupted,
    /// The guest's own `chdir` refused the directory it was to start in.
    #[error("the guest's own chdir refused {path:?}, the directory it was to start in")]
    StartRefused {
        /// The path its `chdir` was given.
        path: String,
    },
    /// The runtime failed outside the guest.
    #[error("the runtime failed outside the guest while {during}: {source}")]
    Runtime {
        /// What the host was doing.
        during: RuntimeStep,
        /// What wasmtime or the operating system said.
        #[source]
        source: wasmtime::Error,
    },
    /// A trap this version cannot classify.
    #[error("the guest stopped with a trap this version cannot classify: {trap}")]
    TrapUnclassified {
        /// How wasmtime described it.
        trap: String,
    },
    /// The host broke an invariant of its own.
    #[error("the sealed host broke an invariant of its own: {invariant}")]
    HostInvariant {
        /// Which one.
        invariant: Invariant,
    },
}

impl SealedError {
    /// The stable code of this failure.
    #[must_use]
    pub const fn code(&self) -> ErrorCode {
        self.sealed_code().error_code()
    }

    /// Which failure mode this is.
    #[must_use]
    pub const fn sealed_code(&self) -> SealedCode {
        match self {
            Self::ArgumentHoldsNul { .. } => SealedCode::ArgumentInvalid,
            Self::EnvironmentVariable { .. } => SealedCode::EnvironmentInvalid,
            Self::SnapshotPath { .. } => SealedCode::SnapshotInvalid,
            Self::Preopen { .. } => SealedCode::PreopenInvalid,
            Self::WorkingDirectory { .. } => SealedCode::WorkingDirectoryInvalid,
            Self::Halt { .. } => SealedCode::HaltInvalid,
            Self::ModuleMalformed { .. } => SealedCode::ModuleMalformed,
            Self::ModuleComponent => SealedCode::ModuleComponent,
            Self::ModuleMemory { .. } => SealedCode::ModuleMemory,
            Self::ModuleImport { .. } => SealedCode::ModuleImport,
            Self::ModuleEntry { .. } => SealedCode::ModuleEntry,
            Self::StartUnexported { .. } => SealedCode::ModuleUnstartable,
            Self::RedirectUnexported { .. }
            | Self::RedirectMismatched { .. }
            | Self::RedirectUnread => SealedCode::ModuleUnredirected,
            Self::Preparation { .. } => SealedCode::PreparationUnavailable,
            Self::Engine { .. } => SealedCode::EngineUnavailable,
            Self::Compile { .. } => SealedCode::ModuleUncompiled,
            Self::Link { .. } => SealedCode::HostUnlinked,
            Self::WatchdogUnavailable { .. } => SealedCode::WatchdogUnavailable,
            Self::WatchdogExpired { .. } => SealedCode::WatchdogExpired,
            Self::Interrupted => SealedCode::Interrupted,
            Self::StartRefused { .. } => SealedCode::StartRefused,
            Self::Runtime { .. } => SealedCode::RuntimeFailed,
            Self::TrapUnclassified { .. } => SealedCode::TrapUnclassified,
            Self::HostInvariant { .. } => SealedCode::HostInvariant,
        }
    }
}

/// What makes an environment variable one a guest cannot be given.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum EnvironmentFault {
    /// The name is empty.
    EmptyName,
    /// The name holds `=`, which would end the name early.
    NameHoldsEquals,
    /// The name or the value holds a NUL byte, which would end the C string early.
    HoldsNul,
    /// The name is given twice.
    Repeated,
}

impl fmt::Display for EnvironmentFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::EmptyName => "its name is empty",
            Self::NameHoldsEquals => "its name holds `=`",
            Self::HoldsNul => "it holds a NUL byte",
            Self::Repeated => "it is given twice",
        })
    }
}

/// What makes a snapshot path one a snapshot cannot hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum SnapshotFault {
    /// The path is empty, absolute, or has an empty, `.`, `..` or NUL-bearing component.
    NotRelative,
    /// The path is given twice.
    Repeated,
    /// The path is a file in one place and a directory in another.
    FileAndDirectory,
    /// A change to the path came before the directory it is in was one the snapshot holds.
    NoParent,
}

impl fmt::Display for SnapshotFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotRelative => "it is not a relative path of named components",
            Self::Repeated => "it is given twice",
            Self::FileAndDirectory => "it is both a file and a directory",
            Self::NoParent => "the directory it is in is not one the snapshot holds",
        })
    }
}

/// What makes a guest path one that cannot be preopened.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum PreopenFault {
    /// The path is empty.
    Empty,
    /// The path holds a NUL byte.
    HoldsNul,
    /// The path names a place a preopen before it names, as wasi-libc reads a name.
    Repeated,
}

impl fmt::Display for PreopenFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "it is empty",
            Self::HoldsNul => "it holds a NUL byte",
            Self::Repeated => "it names a place a preopen before it names",
        })
    }
}

/// What makes a directory to start in one the guest cannot start in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum WorkingFault {
    /// No tree is preopened at the path it names before the root.
    NoTree,
    /// The directory is not `/`-separated names, each neither empty, `.`, `..`, nor holding NUL.
    NotNames,
    /// The tree holds no directory at those names.
    NotADirectory,
}

impl fmt::Display for WorkingFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoTree => "no tree is preopened at that path before the root",
            Self::NotNames => "it is not a relative path of `/`-separated names",
            Self::NotADirectory => "the tree holds no directory there",
        })
    }
}

/// What makes a module's memory one sealed execution does not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryFault {
    /// A memory indexed by 64-bit addresses.
    Memory64,
    /// A memory shared between threads.
    Shared,
    /// A memory with a page size other than 64 KiB.
    CustomPageSize,
    /// A memory the module imports rather than defines.
    Imported,
    /// Not exactly one memory.
    Count {
        /// How many the module defines.
        count: u32,
    },
}

impl fmt::Display for MemoryFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Memory64 => f.write_str("it is a 64-bit memory"),
            Self::Shared => f.write_str("it is a shared memory"),
            Self::CustomPageSize => f.write_str("it has a custom page size"),
            Self::Imported => f.write_str("it is imported rather than defined"),
            Self::Count { count } => {
                write!(
                    f,
                    "the module defines {count} memories where it needs exactly one"
                )
            }
        }
    }
}

/// What makes an import one sealed execution does not satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum ImportFault {
    /// The import names a module other than `wasi_snapshot_preview1`.
    OtherModule,
    /// The name is not a function of WASI preview1.
    NotInTable,
    /// The import is not a function.
    NotAFunction,
    /// The function is imported with another signature than the specification's.
    Signature,
}

impl fmt::Display for ImportFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::OtherModule => "is not from wasi_snapshot_preview1",
            Self::NotInTable => "is not a function of WASI preview1",
            Self::NotAFunction => "is not imported as a function",
            Self::Signature => "is imported with another signature than WASI preview1 gives it",
        })
    }
}

/// What makes a module something other than a WASI command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum EntryFault {
    /// No `_start` export of type () -> ().
    NoStart,
    /// No exported memory named `memory`.
    NoMemory,
    /// A start section, which would run guest code before `_start` is called.
    StartSection,
}

impl fmt::Display for EntryFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NoStart => "it exports no `_start` function of type () -> ()",
            Self::NoMemory => "it exports no memory named `memory`",
            Self::StartSection => "it has a start section, which runs guest code before `_start`",
        })
    }
}

/// What the host was doing when the runtime failed outside the guest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum RuntimeStep {
    /// Setting or reading the fuel account.
    Fuel,
    /// Making the instance.
    Instantiation,
    /// Reserving memory the limits allowed.
    Memory,
    /// Entering the directory the guest starts in.
    Start,
    /// Calling the entry point.
    Entry,
}

impl fmt::Display for RuntimeStep {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Fuel => "keeping the fuel account",
            Self::Instantiation => "instantiating the module",
            Self::Memory => "reserving memory the limits allowed",
            Self::Start => "entering the directory the guest starts in",
            Self::Entry => "calling `_start`",
        })
    }
}

/// An invariant of the host that did not hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum Invariant {
    /// A host function was called with parameters of other types than the table declares.
    Signature,
    /// The guest's exported memory was not there when a host function needed it.
    Memory,
    /// A count or a size did not fit the width it is recorded at.
    Width,
    /// A module owner's synchronization state was interrupted while locked.
    ModuleOwnerPoisoned,
}

impl fmt::Display for Invariant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Signature => {
                "a host function was called with parameters of other types than the import table declares"
            }
            Self::Memory => "the guest's memory was not there when a host function needed it",
            Self::Width => "a count or a size did not fit the width it is recorded at",
            Self::ModuleOwnerPoisoned => "a module owner lock was poisoned",
        })
    }
}
