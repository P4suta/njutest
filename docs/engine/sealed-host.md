<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The sealed host

**Status: implemented, and not yet read by the engine.** `crates/rust-mutants-sealed` runs a WebAssembly guest compiled for `wasm32-wasip1`; a later change has the engine run each test of an instrumented snapshot through it.

A sealed invocation is a pure function of content-addressed inputs.
The same module, run with the same invocation under the same configuration, gives a transcript that is the same byte for byte, down to its digest.
Everything a guest could otherwise learn from the machine it runs on — the time, the environment, the files, randomness, how much memory there is — is an input it was handed, and everything it does is recorded rather than done to the machine.

## One invocation

`SealedRunner::new` configures one wasmtime engine and links the host once.
`SealedRunner::prepare` validates a module, compiles it in memory, and links it; `SealedModule::invoke` runs `_start` in a fresh store and a fresh instance every time.
Compiled code never leaves memory: there is no `Module::deserialize`, which would trust bytes on disk to be code.

An `Invocation` is one value holding everything the run is a function of:

| Input | What the guest sees |
| --- | --- |
| `arguments` | `args_get`, the program name first |
| `environment` | `environ_get`, in the order of the names |
| `preopens` | each `Preopen` as descriptors 3, 4, … in the order given: a read-only `Snapshot` at its guest path, which may be an absolute host path string, or the working directory `.` inside a tree given before it, as [the working directory](#the-working-directory) says |
| `seed` | the stream `random_get` draws from |
| `fuel` | the budget every WebAssembly instruction and every host call spends |
| `limits` | the most memory, standard output, standard error, and overlay the guest may hold |
| `clock` | the origins of the clocks and how many nanoseconds a unit of fuel moves them |
| `halt` | nothing, or an absolute guest path inside a tree given before, at which a rename that puts a file there ends the guest in that call, as [`Halted`](#how-a-run-ends) |

An invocation can start where another stopped.
`Preopens::after` gives each tree as a transcript's overlay, or any part of it, left it: an entry changes the tree whose guest path is the longest that holds it, in the order the overlay lists them, times included, so a snapshot after an overlay is exactly the snapshot built with what the overlay left, down to its digest.
An entry below no tree, or inside a directory its tree does not hold by then, is refused (`RS0003`).

The runner's own watchdog is not an input.
It is a wall-clock backstop, and when it fires the invocation ends in `RS3002`, an error, never in a stop a caller could read as an answer about the guest.
Neither is the `Interrupt` a caller passes beside the invocation: raised, it ends the invocation in `RS3005` at the guest's next epoch or host call, and one raised before the invocation starts is refused without starting it.

## What a module must be

A module that is not one of these is refused with a typed error before it is compiled, or before it is linked:

- a core module, not a component (`RS1002`);
- exactly one memory, defined rather than imported, not 64-bit, not shared, with the ordinary page size (`RS1003`);
- every import a function of `wasi_snapshot_preview1` with the specification's own signature (`RS1004`);
- a WASI command: a `_start` of type `() -> ()`, a memory exported as `memory`, and no start section (`RS1005`).

The engine leaves off every proposal that is nondeterministic or that the host does not run: relaxed SIMD, memory64, multi-memory, threads, custom page sizes, wide arithmetic, stack switching, GC, and exceptions.
A module that uses one of them is refused by the compiler (`RS2002`).
Floating-point NaNs are canonicalized.

## The import table

`WasiFunction` is one closed enum of all forty-six functions of WASI preview1, and the host carries every one of them out through one exhaustive match.

| Functions | What the host does |
| --- | --- |
| `args_get`, `args_sizes_get`, `environ_get`, `environ_sizes_get` | the invocation's arguments and environment, as C strings |
| `clock_time_get` | the clock's origin, plus the fuel spent times `nanos_per_fuel`, plus every wait; the two CPU-time clocks read the fuel alone |
| `clock_res_get` | one nanosecond, for every clock |
| `poll_oneoff` | every descriptor is ready at once; where no descriptor is asked about, the clocks move straight to the earliest deadline and the call returns |
| `random_get` | SHA-256 of the seed and a block counter, so the bytes are a function of the seed and of how many were read before |
| `fd_write`, `fd_read` on 1, 2, 0 | standard output and standard error are kept as far as their caps and counted past them; standard input is at its end |
| `path_open`, `fd_read`, `fd_pread`, `fd_seek`, `fd_tell`, `fd_filestat_get`, `path_filestat_get`, `fd_readdir` | the snapshot, as the overlay has changed it; a name its directory holds only in another case is refused with `notcapable` and recorded, `case-only` |
| `fd_write`, `fd_pwrite`, `fd_filestat_set_size`, `fd_allocate`, `path_create_directory`, `path_rename`, `path_unlink_file`, `path_remove_directory`, `fd_filestat_set_times`, `path_filestat_set_times` | written into the invocation's own overlay, which later calls in the same invocation see and no other invocation does; a name made, renamed to or removed that its directory holds only in another case is refused, `case-only` |
| `fd_close`, `fd_renumber`, `fd_fdstat_get`, `fd_fdstat_set_flags`, `fd_fdstat_set_rights`, `fd_prestat_get`, `fd_prestat_dir_name`, `fd_advise`, `fd_sync`, `fd_datasync` | the descriptor table, which gives out the lowest free number |
| `path_readlink` | nothing is a symbolic link: `inval` for a path that exists, `noent` for one that does not |
| `proc_exit` | ends the invocation as `Exited` |
| `sched_yield` | returns |
| `sock_accept`, `sock_recv`, `sock_send`, `sock_shutdown` | refused with `notsup` and recorded |
| `proc_raise` | refused with `nosys` and recorded |
| `path_link`, `path_symlink` | refused with `notsup` and recorded |

A path that leaves what the descriptor it is resolved from may reach, or that is absolute and names no place inside it, is refused with `notcapable` and recorded.
A name a directory does not hold, where it holds one that differs from it only in case, is refused with `notcapable` and recorded as `case-only`, whether the path looks it up, makes it, renames to it or removes it.
A snapshot holds names exactly as they were read, while the file system a build ran on may compare them in either case, as NTFS and APFS do by default, and then the name would have reached the other.
The host does not choose between the two: it answers neither, and says so, on every machine alike, so a transcript does not depend on the file system of the machine that made it.
Two names are compared as NTFS compares them, character by character, each in either case where its case is one other character.
A write the overlay has no room left for is refused with `nospc` and recorded.
A wait on a CPU-time clock, which moves only while the guest runs, is answered with `notsup` in its event and recorded.
A wait for a deadline past the end of virtual time is a wait nothing ends, so the guest spends its whole budget and stops as `FuelExhausted`.

A directory lists `.` and `..` and then its entries in the order of their names.
Every file and directory reads the same metadata in every invocation: its times are zero until the guest sets them, its inode is a digest of its path, its device is the number of its preopen, and it has one link.

## The working directory

A `Preopen::Working` names a tree preopened before it and a directory inside that tree, and the guest is given it as `.`.
wasi-libc, which every Rust guest resolves a path through, reads a preopen named `.` as the empty prefix, and gives a path to the preopen whose name is the longest prefix of it, so a path no other preopen names reaches the working directory.
wasi-libc takes the leading `/` off a path before it matches one, so an absolute path no other preopen names reaches it too, as the relative path it becomes: a sealed guest's root directory is its working directory.
Two preopens wasi-libc reads as one prefix, such as `/` and `.`, or `/a` and `a/`, are refused (`RS0004`).

A relative path starts at the working directory and climbs with `..` as far as its tree's root, never past it.
The working directory and its tree are one tree: what a path through one wrote, a path through the other reads, and the transcript reports it under the tree's guest path.

A tree's guest path says how the build that made the guest spells a path into it.
A guest path spelled from a drive's root, `X:\…` or `X:/…`, is a Windows tree: in every path into it `\` and `/` both separate names, and a path is read as Windows reads one, empty names and `.` dropped and each `..` taking back the name before it.
Any other guest path is a POSIX tree, where `/` alone separates names and `..` climbs the directories walked.
A Windows build bakes paths such as `env!("CARGO_MANIFEST_DIR")` into the guest as it spells them, and the guest's standard library, which knows `/` alone, takes them for relative ones and joins them with `/`.
wasi-libc matches a preopen's name only where `/` or nothing follows it, so `C:\tree\pkg/data.txt` never reaches the tree preopened at `C:\tree`, and neither does `C:\tree/data.txt` where the tree was preopened at `C:\tree\`.
The working directory of a Windows tree answers such a path: one whose names begin with the names of the tree's root resolves from the tree's root.
The drive letter and every name of the tree's root are compared in either case, as NTFS compares a name: character by character, each in either case where its case is one other character, so a build that spelled the root in another case than the tree was read at still reaches it.
A name below the root is walked exactly as spelled, as the snapshot holds it, and one it holds only in another case is refused, as in every tree.
Any other absolute path, on another drive, below a root it does not spell, rooted without a drive, or on a drive without its root, names no place in the tree and is refused.

## What a run costs

Every WebAssembly instruction spends the fuel wasmtime assigns it.
Every host call spends 64 units before it does anything, and one more for every byte of guest memory it reads or writes.
The bytes are paid for as they are touched, so a call the fuel left cannot pay for stops the guest before it reads or writes past what was paid, however much memory it names.
Instantiation spends fuel too: wasmtime copies data segments into memory and fills tables with a compiled startup function, and the guest's budget pays for it.
The copy-on-write memory images wasmtime can use instead exist on Linux alone, where they skip that function, so they are turned off: with them on, the same guest would spend differently on each platform.

## How a run ends

`SealedStop` is closed:

| Stop | When |
| --- | --- |
| `Returned` | `_start` returned, which wasi-libc does only when `main` returned zero |
| `Exited { code }` | the guest called `proc_exit` |
| `Trapped { kind }` | the guest trapped; `TrapKind` names every trap of the pinned wasmtime but running out of fuel and being interrupted |
| `FuelExhausted` | the budget ran out, in WebAssembly, in a host call, in instantiation, or in a wait nothing ends |
| `MemoryExhausted` | the memory limit refused a growth, or the memory a module starts with, and the guest then trapped, as a Rust guest does when its allocator fails |
| `Halted` | a `path_rename` put a file at the invocation's `halt` path; the host ended the guest in that call, so the overlay is what it held when the rename was done and nothing the guest would have run after it ran: no destructor, exit handler or flush |

A halt path names a place below a tree's guest path by `/`-separated names; one below no tree, naming a tree itself, or holding an empty name, `.`, `..` or NUL is refused before the guest starts (`RS0006`).
Only a rename halts: a file made at the path by opening it is still being written, and a rename is how a writer says it is whole.

A panicking Rust guest traps `Unreachable` with its message on standard error, because `wasm32-wasip1` aborts on a panic.
A libtest harness run with `--exact <name> --test-threads=1 --nocapture` returns for a passing test, traps for a panicking one, and exits 101 for one that returns `Err`.

## The transcript

A `Transcript` holds the stop, the fuel spent, the largest the memory grew, standard output and standard error with how much was cut from each, every refusal by function and reason, the growths the limits refused, how far waits moved the clocks, and every path of every preopened tree whose final state differs from its snapshot.
It carries two digests.
The invocation digest is taken over the module's digest, this crate's version, the wasmtime version, the configuration, and every input of the invocation.
The configuration includes wasmtime's own account of how it compiles — the target, the CPU features it compiles for, its tunables, and the proposals it leaves on — and the host's own constants.
The transcript digest is taken over everything the transcript holds.

## What one machine and another may differ in

Stack depth.
The WebAssembly stack is bounded in bytes of the native stack, and how many calls fit depends on how large the compiled frames are, which differs between architectures and can differ between CPU features.
A guest that overflows the stack traps `StackOverflow` wherever it runs, but a recursion that only nearly overflows can trap on one machine and not another, and the fuel spent before the trap differs.
That is why the configuration digest names the target and its CPU features: two transcripts are claimed to agree only under one configuration.

## Testing it

`cargo nextest run -p rust-mutants-sealed` runs the suite: hand-written WebAssembly commands, one for each host call, and the laws over generated inputs.
`toolchain_guests` compiles Rust guests with the toolchain `rust-toolchain.toml` pins, for `wasm32-wasip1`, which the same file installs; a test that finds the target missing fails and names `rustup target add wasm32-wasip1 --toolchain 1.98.0`.
It keeps compiled guests under `target/tmp/sealed-guests`, each under the digest of its sources, the compiler, and its flags.
