<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# The sealed host

**Status: implemented, and held to the official WASI preview1 conformance suite.** `crates/rust-mutants-sealed` runs a WebAssembly guest compiled for `wasm32-wasip1`, and the engine runs each test of an instrumented snapshot through it, as [sealed execution](sealed.md) says.

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
| `preopens` | each `Preopen` as descriptors 3, 4, … in the order given: a read-only `Snapshot` at its guest path, which may be an absolute host path string, or the root directory `/` and the directory the guest starts in, as [the root and the start](#the-root-and-the-start) say |
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

Every descriptor carries the rights WASI gives it, and a call a right governs asks the descriptor it names for that right before it does anything.
A descriptor without it is answered `notcapable`, or `badf` where the right is reading or writing, as POSIX answers a descriptor not open for them, and nothing is recorded: a right the guest gave up is its own narrowing, not the sandbox refusing.
A preopen carries every right a directory can and passes every right on; a descriptor `path_open` gives out carries what it asked for of what its directory passes on and of what its kind can carry, never more; and `fd_fdstat_set_rights` narrows, never widens.
`path_open` needs `path_open`, with `path_create_file` to make the file and `path_filestat_set_size` to truncate it; `path_rename` needs `path_rename_source` of the one descriptor and `path_rename_target` of the other; `fd_pread` and `fd_pwrite` need `fd_seek` besides reading or writing; asking the position without moving it needs `fd_tell` or `fd_seek`; and a descriptor waited on in `poll_oneoff` needs `poll_fd_readwrite`.
Every other call a right is named after needs that right: `fd_advise`, `fd_allocate`, `fd_datasync`, `fd_fdstat_set_flags`, `fd_filestat_get`, `fd_filestat_set_size`, `fd_filestat_set_times`, `fd_readdir`, `fd_seek`, `fd_sync`, `fd_tell`, `path_create_directory`, `path_filestat_get`, `path_filestat_set_times`, `path_readlink`, `path_remove_directory` and `path_unlink_file`.
The standard streams carry reading or writing, polling, asking their metadata and setting their flags.

A path that leaves what the descriptor it is resolved from may reach, or that is absolute and names no place inside it, is refused with `notcapable` and recorded.
A name a directory does not hold, where it holds one that differs from it only in case, is refused with `notcapable` and recorded as `case-only`, whether the path looks it up, makes it, renames to it or removes it.
A snapshot holds names exactly as they were read, while the file system a build ran on may compare them in either case, as NTFS and APFS do by default, and then the name would have reached the other.
The host does not choose between the two: it answers neither, and says so, on every machine alike, so a transcript does not depend on the file system of the machine that made it.
Two names are compared as NTFS compares them, character by character, each in either case where its case is one other character.
A write the overlay has no room left for is refused with `nospc` and recorded.
A wait on a CPU-time clock, which moves only while the guest runs, is answered with `notsup` in its event and recorded.
A wait for a deadline past the end of virtual time is a wait nothing ends, so the guest spends its whole budget and stops as `FuelExhausted`.

A directory lists `.` and `..` and then its entries in the order of their names.
Every file and directory reads the same metadata in every invocation.
Its times are what the realtime clock reads when the invocation starts, a file or directory the guest makes as much as the snapshot's, until the guest sets them, so no time is one the guest's own clock could never have read.
Its inode is a digest of its path, its device is the number of its preopen, and it has one link.

## The root and the start

A `Preopen::Root` gives the guest `/`.
wasi-libc, which every Rust guest resolves a path through, gives a path to the preopen whose name is the longest prefix of it, after it takes the leading `/` off both, so the root, whose name is the empty prefix, receives every absolute path no tree's own preopen names, without its `/`.
From the root, a path reaches a tree whose own spelling of an absolute path reads it as one below the tree's root, and of several, the one whose root is deepest.
Every other path is refused with `notcapable` and recorded as an escape, and so is the root itself, which lists nothing and has no metadata: nothing a guest reaches through `/` is the machine's.
Two preopens wasi-libc reads as one prefix, such as the root and a tree at `/` or `.`, or `/a` and `a/`, are refused (`RS0004`).

A tree's guest path says how the build that made the guest spells a path into it.
A guest path spelled from a drive's root, `X:\…` or `X:/…`, is a Windows tree: in every path into it `\` and `/` both separate names, and a path is read as Windows reads one, empty names and `.` dropped and each `..` taking back the name before it.
Any other guest path is a POSIX tree, where `/` alone separates names and `..` climbs the directories walked; from the root, a `..` would climb the machine's own directories, which decide natively where it lands, so it is refused.
A Windows build bakes paths such as `env!("CARGO_MANIFEST_DIR")` into the guest as it spells them, and the guest's standard library, which knows `/` alone, takes them for relative ones and joins them with `/`.
wasi-libc matches a preopen's name only where `/` or nothing follows it, so `C:\tree\pkg/data.txt` never reaches the tree preopened at `C:\tree`, and it reaches the root.
The root reads it as the Windows tree spells it: a path whose names begin with the names of the tree's root resolves from the tree's root, and it is read from the last of its names that begins a drive's root, since wasi-libc joins a path it took for a relative one onto the directory the guest started in, as Windows replaces one absolute path with another.
The drive letter and every name of the tree's root are compared in either case, as NTFS compares a name: character by character, each in either case where its case is one other character, so a build that spelled the root in another case than the tree was read at still reaches it.
A name below the root is walked exactly as spelled, as the snapshot holds it, and one it holds only in another case is refused, as in every tree.
Any other absolute path, on another drive, below a root it does not spell, rooted without a drive, or on a drive without its root, names no place in the tree and is refused.

The root's `start` names a directory of a tree preopened before it, and the host enters it before `_start`, through the guest's own C library, as cargo starts a test in its package's directory: it asks the guest's `malloc` for room, writes the directory there as the tree's build spells a path, and calls the guest's `chdir`.
The calls run the guest's own code and spend its fuel, so a stop in them is the guest's stop, and a `chdir` that refuses the directory leaves the guest where it was not asked to start, which is no answer about it (`RS3006`).
wasi-libc then makes every relative path absolute from that directory, so a relative path reaches its tree from the start and climbs with `..` as far as the tree's root, never past it, and never reaches the root: a relative path and an absolute one are two calls the host tells apart.
A module that does not export `chdir` and `malloc`, both of type (i32) -> i32, cannot be started in a directory (`RS1006`); `START_LINK_ARGS`, `--undefined=chdir --export=chdir --export=malloc`, are the linker arguments that keep the C library's `chdir` and export both.
Without a start, the guest's C library starts at `/`, so every relative path reaches the root and is refused.
`std::env::current_dir()` reads the start as the C library keeps it: a POSIX directory as given, and a Windows one after a `/` of wasi-libc's own.

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

### The official conformance suite

`cargo xtask wasi-testsuite` holds the host to [WebAssembly/wasi-testsuite](https://github.com/WebAssembly/wasi-testsuite): every one of its preview1 tests, prebuilt from AssemblyScript, C and Rust, at the commit `crates/rust-mutants/tests/wasi-testsuite.toml` pins.
The command fetches that commit once into a cache, `target/wasi-testsuite` of the workspace or the directory `--cache` names, and verifies it by its id every time it runs: its `HEAD` is the commit, and nothing of its tree is changed, added or ignored beside it.
It then runs the harness, the module `wasi_testsuite` of `rust-mutants`' suite, which runs each test on this host exactly as a sealed test instance runs: the engine's fuel, limits, clock and seed, the test's root preopened at `/` as the suite's own runners preopen it, its arguments and environment as its JSON gives them, and standard input at its end.
A test passes where it ends with the exit code its JSON names, zero where it names none, and writes exactly the output it names.

The expectations name every test with what the host must give: `pass`, or `refused`, for a test that fails because the host refuses what it asks by design.
Each names every refusal the host records while the test runs, and a refused one also how the test then ends and the row of the import table above that documents the refusal; a test of the harness holds every such row to this page, listing the function and answering it as the reason says.
The command fails on a test that ends otherwise, on a test the file does not name, and on a name the suite does not hold, so a test added upstream is caught when the pin moves.
At the pinned commit 63 tests pass and 9 are refused.
Three of the 63 pass past a refusal: one tests for the answer an escaping path is given, and two skip what they test once a symbolic link cannot be made.
Of the 9, two call `sock_shutdown`, which the host refuses with every socket call, and seven make a hard or symbolic link, which it refuses too.
A result that is neither a pass nor a refusal the import table documents is a defect of the host, fixed with a test of its own in this crate so that it is held without the suite.
`mise run wasi-testsuite` runs the command with its cache in the user's cache directory, and the CI job of the same name runs that task.
