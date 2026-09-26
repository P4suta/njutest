# Error codes

**Status: implemented.** Every code here is one the engine, the runner, or the repository's own `cargo xtask` can return, and a test in each keeps its table and its list of codes in step in both directions.

Every failure the engine, the runner, and `cargo xtask` report carries a stable code.
The code is the searchable name of the failure: grep this file, the issue tracker,
and a trace for it.
A test in each crate keeps this table and the code's own list equal in both directions, so a code is either here or it does not exist.

Codes are `RM` (rust-mutants), `NJ` (njutest), or `XT` (xtask, the repository's gates and audits) followed by four digits.
The first digit names an area:

| Digit | rust-mutants | njutest |
| ---: | --- | --- |
| 0 | the engine's own contract | the command line and its contract |
| 1 | workspace and snapshot | configuration |
| 2 | discovery | repository and evidence identity |
| 3 | instrumentation | targets and baseline |
| 4 | validation | coverage |
| 5 | execution | mutation |
| 6 | probing | reports and stores |
| 7 | process supervision | providers |
| 8 | — | caches and temporary directories |
| 9 | internal invariants | internal invariants |

## rust-mutants

| Code | Meaning | Remedy |
| --- | --- | --- |
| `RM0001` | The caller cancelled the operation before it completed. | nothing was left half-done; run it again when you are ready |
| `RM0002` | A configuration file that could not be read. | check the file is readable by this user; the path names it |
| `RM0003` | A configuration file that is not the document this version understands: an unknown key, a malformed value, a duration that is not a duration. | `rust-mutants init` writes a file this release understands, with every key commented |
| `RM0004` | A configuration or a flag that parses but says something a run cannot honour: an expectation without a reason, two expectations that name one mutation, a harness flag the engine owns, a report directory outside the workspace, a shard that is not a part of something, a line range that addresses no line, a run name a directory cannot be, a `--file` this run does not read, a `--rule` or `--family` this release does not know. | the message says which key and why; `rust-mutants init` writes one that is valid |
| `RM0005` | A configuration whose `version` is not one this release understands. | this release reads version 1; a newer file needs a newer release |
| `RM0006` | A process environment names any mutation-control value unless this is an instrumented engine binary carrying the same nonempty catalog and exactly one nonempty activation or touch mode. | unset the RUST_MUTANTS_ variable the message names and run again |
| `RM0007` | A stored run report or recording that is not there or cannot be read. | every run is a directory named for it in the configured reports directory (`reports/mutation` by default); `rust-mutants report` with no `--run` reads the newest |
| `RM0008` | A file a command would write that is already there, and `--force` was not given. | remove the file, or name another path: a command here never writes over what it did not write |
| `RM0009` | A report or configuration file that could not be written. | check the directory exists and this user may write in it; the path names the file |
| `RM0010` | A change set git could not be asked for: the tree is not a repository, or it does not know the revision. A run that could not see what changed never reads as a run that saw nothing change. | run inside a git working tree, or name what to measure with --include |
| `RM0011` | The reports given are not the parts of one catalog: a different tree, a different catalog, or two parts that hold the same mutant. | merge the parts of one run: the same tree, the same catalog, and one report per --shard |
| `RM0012` | A source file a report names cannot be read from the root given, so the mutation cannot be shown as a change. | pass --root at the tree the run measured, or check the file out again |
| `RM0013` | An outcome cache cannot be enumerated completely, so its size or removal cannot be stated exactly. | check the cache directory is readable by this user, or pass --cache-dir at another one |
| `RM0014` | A continuous integration host a command was asked to write for, which the environment does not provide: `--host github` outside a GitHub Actions step, or in one that names no step summary, output file, or checkout. | run the step inside GitHub Actions, which names GITHUB_STEP_SUMMARY, GITHUB_OUTPUT and GITHUB_WORKSPACE, or pass --host plain |
| `RM0015` | A workspace root outside the checkout the host places annotations in, so no annotation could name a file the host can find. | pass --root at the workspace inside the checkout GITHUB_WORKSPACE names |
| `RM0016` | A file the host named for a step's summary or outputs, which could not be appended to. | the runner names the file for this step; check no earlier step removed it or its directory |
| `RM0017` | Rust text the engine read is not Rust of the kind it asked for. | the message names the line and column in the text read; the file or fragment it came from is the one to look at |
| `RM0018` | Rust text that would take its reading thread's locations past what they address: proc-macro2 keeps 32-bit locations per thread, and past them every location wraps. | a single file this large is refused rather than read to the wrong places; split it, or skip it with an exclude pattern |
| `RM0019` | The thread Rust text is read on could not be started. | the operating system refused a thread; check the process and memory limits of this user, and run again |
| `RM1001` | Snapshot options that cannot be honoured, such as a report directory that is absolute or climbs out of the source root. | name a report directory inside the workspace; one that climbs out of it would have the run write where nothing sweeps |
| `RM1002` | A source root that is relative, cannot be read, or is not a directory. | pass --root at a directory that exists and holds the workspace manifest |
| `RM1003` | An operating system failure while reading a tree: a directory that cannot be listed, an entry that cannot be stat'ed. | this is what the operating system said; the path it names is the one to look at |
| `RM1004` | A symbolic link inside the source tree. Links are refused rather than followed or skipped; add an exclude pattern. | a copy cannot follow a link out of the tree and cannot leave it dangling, so remove it or name its directory in [snapshot] omit |
| `RM1005` | A Windows reparse point (junction or mount point) inside the source tree. | a copy cannot reproduce a junction, so remove it or name its directory in [snapshot] omit |
| `RM1006` | A file that is neither a directory nor a regular file: a device, a socket, a named pipe. | a device, socket, or pipe is not a file a copy can hold; name its directory in [snapshot] omit |
| `RM1007` | A file name that cannot survive the round trip through a slash-separated relative path, such as one containing a backslash. | rename the file: a run says the same thing on every platform, and this name cannot |
| `RM1008` | The snapshot directory could not be created or claimed. | check TMPDIR is a directory this user may write in, and that there is room under it |
| `RM1009` | A failure while copying the tree into the snapshot. | check there is room under TMPDIR, and that nothing is writing the tree while it is copied |
| `RM1010` | A cleanup refused because the recorded directory does not look like a snapshot directory. The guard between a bug and a user's source tree. | the recorded path is not one this tool made; remove it yourself rather than having a tool remove a directory it cannot identify |
| `RM1011` | A snapshot directory that survived every removal attempt, usually a file still locked by a test binary on Windows. | something is holding it open, and a sweep cannot take it back; `rust-mutants cache` says where it is |
| `RM1012` | The cargo or rustc executable could not be found: an explicit path that is not a file, or a bare name absent from the search path, naming every entry of it that could not be read and was passed over as a shell passes over it. | install the toolchain, or name cargo with --cargo, or put it on the PATH this process was given |
| `RM1013` | A `-vV` banner lacks its `release:` or `host:` line, so the toolchain cannot be named. | the toolchain answered something this release cannot read; `rustup update` and try again |
| `RM1014` | A cargo command could not start, timed out, or exited unsuccessfully; cargo's own words follow. | run the same cargo command yourself: what it says there is what it said here |
| `RM1015` | `cargo metadata` printed something that is not its document. | run `cargo metadata` yourself on this tree; what it prints is what could not be read |
| `RM1016` | A line of `cargo … --message-format=json` output is not a message. | run the same `cargo … --message-format=json` yourself; what it prints is what could not be read |
| `RM1017` | The workspace reads code from a path outside itself, which the copy a run measures does not hold. Allow the directory with `--allow-outside`, or vendor it inside the tree. | --allow-outside DIR copies that directory into the copy where the tree reaches it, or [project] allow_outside does |
| `RM1018` | `--root` names a member of a workspace rather than the workspace. A run measures a copy of what it was given, and a member on its own is not a buildable tree. | run with --root at the workspace root the message names, and --package to narrow it |
| `RM1019` | A directory a run would copy has no place in the copy that keeps every path into it resolving: it is not absolute, it still climbs, it is the tree or holds it or is inside it, or it lies on another filesystem root. A copy places what it holds by substituting one prefix, so a directory it cannot place is one every path into it would stop reaching. | --allow-outside takes an existing absolute directory outside the tree and on the same filesystem root as it; a copy reproduces the shape of what it copies, and cannot hold a directory that is the tree, holds it, or lies across a volume |
| `RM1020` | A manifest a run has to read is there and could not be read. A run decides what it may copy, which targets carry a harness, and which lints a crate forbids from these, and an empty answer to any of them is a different run rather than a missing one. | read the manifest the message names yourself: a run decides what it may copy, which targets carry a harness, and which lints a crate forbids from it, and an empty answer to any of those is a different run rather than a missing one |
| `RM1021` | A name handed to a capability directory is not one path component. | a store names its entries itself; a name that could reach a parent, a stream or a device is a defect in the caller, so report it |
| `RM1022` | A target directory's record of what its members were built from could not be read or written, or a unit it names as stale could not be forgotten. | remove the target directory the message names: a run compiles again what it cannot vouch for, and a record it cannot read is one it cannot vouch by |
| `RM1023` | A bare `cargo` from the copy a run measures answers as no toolchain the run can put first on the tests' search path. | run `cargo -vV` from the directory the message names with the environment the run was given: a shim that chooses a toolchain by the directory it runs in has to answer there, or the toolchain rustc names has to hold a cargo |
| `RM2001` | A dep-info file has no rule to read. | run `cargo test --no-run` yourself, then try again: a build that did not finish leaves this behind |
| `RM2002` | An artifact's dep-info file could not be read, so the files its unit compiled are unknown. | run `cargo clean` and try again; a dep-info file from an interrupted build cannot be read |
| `RM2003` | A source file a unit compiled could not be read. | the file a unit compiled is not readable from the copy; check it is not written while the run reads it |
| `RM2004` | A source file the compiler accepted does not parse as Rust for the engine's parser, which may lag the compiler; the file and position are named. | this release parses the edition the manifest declares; a file the compiler accepts and this does not is a defect in this tool, and the path names it |
| `RM2005` | A unit compiled a file outside the workspace root, which the snapshot does not hold. | name the directory in allow_outside, or move the file into the workspace: a run measures a copy, and what is outside it is not in the copy |
| `RM2006` | The candidates could not be assembled into a catalog: a display-id collision or an incoherent candidate. | this is a defect in this tool: no candidate the walk produces should be one the catalog refuses |
| `RM2007` | A selected package is not a workspace member. | name a package `cargo metadata` lists for this workspace; a name no member has narrows nothing |
| `RM2008` | A `rust-mutants: skip` marker names no reason; a skip nobody explained is one nobody can review. | write the marker as `rust-mutants: skip <why this place is not worth measuring>` |
| `RM2009` | A `rust-mutants:` marker names a directive this release does not know, which is a typo or a newer release's word. | `skip` is the only directive this release knows |
| `RM3001` | A candidate is not in the catalog being instrumented, which means the two were computed from different trees. | this is a defect in this tool: the catalog and the instrumentation disagree about which mutants exist |
| `RM3002` | The source is not the one the candidates were discovered from. | the file changed between being read and being instrumented; make sure nothing writes the tree while a run is preparing |
| `RM3003` | Two rewrite sites partially overlap, which a syntax tree cannot produce: an engine bug rather than a fact about the program. | this is a defect in this tool: two rules claimed overlapping bytes, which a syntax tree cannot produce |
| `RM3004` | An alternative could not be folded onto one line. | this is a defect in this tool: a guard has to fit on the line it replaces, and this one did not |
| `RM3005` | The guards could not be applied to the file. | this is a defect in this tool: the guards could not be written back over the file they were cut from |
| `RM3006` | A guard would have moved a line, breaking the invariant every position depends on. | this is a defect in this tool: a guard moved a line, and every position a run reports is relative to lines that did not move |
| `RM3007` | A mutant index makes the generated runtime's inclusive window overflow. | this is a defect in this tool: the catalog outgrew the u32 window the generated runtime can represent |
| `RM3008` | The rewritten file does not read as Rust, down to what every identity macro holds: a guard changed how the syntax around it reads. | this is a defect in this tool: a guard changed how the syntax around it reads; the line is named, and the source there is the case to report |
| `RM4001` | The tree does not compile before any mutant is live, so nothing about the failure is the mutants' doing. | make `cargo test --no-run` pass on the tree as committed, then run again |
| `RM4002` | The mutants a compilation failure came from could not be isolated. | run `cargo test --no-run` on the tree yourself; the compilation failed for a reason this tool could not attribute to one mutant |
| `RM4003` | An instrumented compilation could not be attempted at all: the tree could not be written, or the toolchain could not be reached. | the compilation could not be started at all: check cargo runs on this tree and that there is room under TMPDIR |
| `RM5001` | The workspace does not compile before anything is instrumented. | make `cargo test --no-run` pass on the tree as committed, then run again |
| `RM5002` | A target fails with nothing active, so no outcome under a mutation would be about the mutation. Verification does not stop at the first: every target is run and the refusal names all of them, because somebody is about to fix what it names. The run type-checks the pristine tree rather than running it, so this says what it observed and not that the tree was passing before. | [execution] skip_targets leaves that target out; --no-verify makes every result a result about instrumentation |
| `RM5003` | No mutant of the catalog answers to the identity or prefix given, or several do. | `rust-mutants catalog` lists what this run holds; an identity is re-minted whenever its file changes, so name the mutation by `path:item:rule` instead |
| `RM5004` | No test target of the session answers to the name given. | `rust-mutants catalog` names every target this session built; a name no target has starts nothing |
| `RM5005` | The workspace builds no test target, so no mutant can be measured. | the workspace has nothing that tests, so there is nothing a mutation could be put to; write a test, or point --root at the workspace that has them |
| `RM5006` | The instrumented tree could not be written. | check there is room under TMPDIR and that nothing is removing the run's directory while it writes |
| `RM5007` | The crate planted for the routing layers could not be written, so no layer could be checked before the run believed what it removes. | check the run's scratch directory is one this user may write in and that there is room under it |
| `RM5008` | The crate planted for the routing layers was built by another compiler than the run's, so what it showed about the layers is not about this run. | the planted crate is built by the binaries in the run's own sysroot, so they answered with another version than the run's rustc: the toolchain directory is broken or mixed; reinstall it, or, where the run's rustc names no sysroot, make the cargo on PATH the one the tree resolves to |
| `RM5009` | A fault was asked to run beside something that is not a mutation, or what was named beside it is not a fault. | a fault is put beside a mutation of the same session: name an `inject-error` fault beside a mutant of any other rule |
| `RM5010` | The system gave no randomness for the nonce that ties a crash's notice to its execution. | the operating system's random source failed; nothing the run could do stands in for it, so check the machine rather than the tree |
| `RM5011` | A mutant's execution changed the test executables the run starts, so no answer after it would be about the tests. | run the mutant it names alone with `rust-mutants run --mutant <id> --jobs 1` to confirm, then keep its tests from writing where the test binaries live, or skip it with a reason |
| `RM5012` | The home an execution is given could not be made, or the given home's git identity could not be copied into it. | the path named is under the run's scratch directory or is the given home's git configuration: check there is room under TMPDIR and that this user may read the file named |
| `RM6001` | A coverage export could not be read. | run again without --coverage to measure without it, or check llvm-tools-preview is installed |
| `RM6002` | The LLVM tools the toolchain ships are not installed (`rustup component add llvm-tools`). | rustup component add llvm-tools, or run with --no-coverage |
| `RM6003` | `llvm-profdata` or `llvm-cov` failed. | `rustup component add llvm-tools-preview`, and check the versions match the toolchain in use |
| `RM6004` | A test process wrote no coverage profile at all: the build was not instrumented, or the process did not exit normally. | the test process wrote no profile: check nothing in the suite sets LLVM_PROFILE_FILE for itself |
| `RM7001` | An executable a successful build named could not be read back for equivalence comparison. | run again after checking nothing removes or rewrites target files while the build is being measured |
| `RM9001` | A rule name the canonical registry does not know. | `rust-mutants rules` lists every rule this release knows |
| `RM9002` | A pattern the caller gave is not a pattern. | a pattern is workspace-relative with forward slashes: `src/**/*.rs`, never a leading or trailing slash |
| `RM9003` | A duration the caller gave is not a duration: an empty text, a number without a unit, a unit without a number, an unknown unit, or a number no duration can hold. | write a duration as 30s, 5m, or 1h30m |

## njutest

| Code | Meaning | Remedy |
| --- | --- | --- |
| `NJ0001` | The caller cancelled the operation before it completed. | nothing was left half-done; run it again when you are ready |
| `NJ0002` | The command's output stream could not be written. | check the destination is writable and has space; a composition root may treat a deliberately closed pipe as success |
| `NJ1001` | The configuration file could not be read. | check the file is readable by this user; the path names it |
| `NJ1002` | The configuration file is not the document this version understands: an unknown key, a malformed value. | `njutest init` writes a file this release understands, with every key commented |
| `NJ1003` | The configuration says something a run cannot honour: a harness flag njutest owns, an environment assignment, a resource that is both shared and exclusive, an acceptance without a reason. | the message says which key and why; `njutest init` writes one that is valid |
| `NJ1004` | The configuration names a version this release does not understand. | this release reads version 1; a newer file needs a newer release |
| `NJ1005` | A configuration file is already there, and `init` was not told to replace it. | remove the file first, or edit the one already there: init never writes over a configuration somebody wrote |
| `NJ2001` | The tree a run is about could not be read: a directory that cannot be listed, a file that cannot be read, a lock file that is not the document cargo writes. | run this inside the tree you mean to verify, or pass --directory at it |
| `NJ2002` | The tree changed while it was being measured, so the measurement would describe files it did not read. | run again on a tree nothing else is writing: a measurement is kept only of the bytes it read |
| `NJ3001` | A test binary could not be asked what tests it holds. | run `cargo test --no-run` yourself: a binary that will not list its tests is one the build did not finish |
| `NJ3002` | The build could not be run: cargo would not start, or was stopped. A workspace that does not *compile* is a finding in the report, not this. | run the same cargo command yourself; what it says there is what it said here |
| `NJ3003` | The build's output is not the message stream this version understands. | run `cargo clean` and try again; output from an interrupted build cannot be read |
| `NJ4001` | A coverage export could not be read. | run again without coverage to verify without it, or check llvm-tools-preview is installed |
| `NJ4002` | The LLVM tools the toolchain ships are not installed (`rustup component add llvm-tools`). | `rustup component add llvm-tools-preview` |
| `NJ4003` | `llvm-profdata` or `llvm-cov` failed. | `rustup component add llvm-tools-preview`, and check the versions match the toolchain in use |
| `NJ4004` | A test process wrote no coverage profile at all: the build was not instrumented, or the process did not exit normally. | check nothing in the suite sets LLVM_PROFILE_FILE for itself; a run composes it and an inherited one sends the profile elsewhere |
| `NJ5001` | A provider could not be started: the command is empty, or the operating system refused it. | run the provider's command yourself: it could not be started, and the message names it |
| `NJ5002` | A provider said nothing in the time it was given, so the run cannot say what its resources were. | raise the provider's timeout, or check the command it runs answers at all |
| `NJ5003` | A provider said something this version does not understand: another protocol version, an unknown field, an answer without an instance. | this is a defect in the provider, not in this tool: what it printed is not the document the contract asks for |
| `NJ5004` | A provider said it could not do what it was asked. | the provider refused and said why; nothing here can answer for it |
| `NJ5005` | A provider offered an environment variable a run composes itself, which would decide what every test process measures. | a provider may not set a variable a run composes; remove it from what the provider offers |
| `NJ5006` | A generation provider said something this version does not understand: another protocol version, an unknown field, more candidates than are read, content that is not base64. | this is a defect in the provider, not in this tool: what it printed is not the document the contract asks for |
| `NJ5007` | A generation provider would write where it may not: outside the allowed paths, out of the tree, or a path that is absolute. | a generated candidate is stored beside the tree and never written into it; the provider named a path outside what it may write |
| `NJ5008` | The file a candidate patches is not the file the provider saw, so applying it would overwrite something nobody read. | the file changed after the provider read it; run again on a tree nothing else is writing |
| `NJ5009` | A routing layer did not route the mutant planted for it before the baseline: the reach measurement, `branch-never-taken`, or `never-infected` left a planted mutant where that layer must not, so nothing the layer would remove from the run is believed and the run ends in `ERROR`. | this is a defect in the engine, not in the code under test; no setting skips a sentinel, because a layer that fails one would be deciding which of your mutants never run |
| `NJ6001` | The report could not be written as JSON, which is an invariant failure rather than anything about the code under test. | this is a defect in this tool: a report it built could not be written as JSON |
| `NJ6002` | A document is not the assurance report this version understands: an unknown field, a missing field, a value of the wrong shape. | the document is from another release or another tool; `njutest verify` writes one this release reads |
| `NJ6004` | The report could not be written where a reader will look for it. | check the report directory exists and this user may write in it; the path names the file |
| `NJ6005` | There is no such run to answer about, or none at all. A command never answers about a different run than the one it was asked about. | every run is a directory under `runs/` in the configured reports directory (`reports/runs` by default); `njutest report` with no run reads the newest |
| `NJ6006` | `njutest spec` was asked about something the run made no change in: a file, an item, or `PATH:ITEM` that matches nothing the run cataloged. The run is named with how much of the workspace it asked about, because a changed or scoped run catalogs only part of it. | `njutest report` lists what the run changed; name a file, `PATH:ITEM`, or an item as the source names it |
| `NJ6020` | The measurement a selection reads could not be written. | check the report directory exists and this user may write in it; the path names the file |
| `NJ6021` | There is no measurement to select by, or it is not one this release reads. | `njutest measure` writes one; a selection with nothing measured to stand on selects nothing |
| `NJ8003` | The store of earlier answers could not be used, a report was offered for storage that must not be stored, or the stream answers were being carried on or off this machine stopped. | remove the store and let it be rebuilt: what is in it is read-only evidence and nothing is lost |
| `NJ8004` | A stored answer is not the answer it claims to be, or a line offered to this machine is not an answer at all: a document that does not parse, that does not carry the identity it is filed under, or that does not satisfy the audit every durable report must. | remove the store and let it be rebuilt: a stored answer that is not what it claims is never used |
| `NJ8005` | No port could be listened on in front of a seam, so a run that was to record what went past it could record nothing. | check this machine allows a listener on the loopback interface, and that nothing has taken every port |
| `NJ7001` | The toolchain has no `cargo miri`, and the `deep-v1` contract promises the suite is interpreted. Install it (`rustup +nightly component add miri`) or verify under `standard-v1`. | `rustup +nightly component add miri`, or ask for a contract that does not promise interpretation |
| `NJ7002` | The verified model-checking phase could not preserve its own evidence. | the message names the internal source or artifact boundary that failed; fix its permissions or report the invariant failure |
| `NJ7003` | The run could not schedule measurements: a worker would not start, or state a panic interrupted would have to be trusted. When a worker would not start, the ones already started stop before taking anything. | run it again; a poisoned coordination lock is never recovered as ordinary state |
| `NJ7004` | An assurance phase printed output that is not valid UTF-8, so its text protocol cannot be interpreted exactly. | the named tool violated its text-output contract; fix or replace that tool before trusting its result |
| `NJ7005` | The run ran out of file descriptors or memory while reading the sources a single-threaded proof rests on. | raise the open-file limit or free memory and run it again; what could not be opened is not known to be unreadable |
| `NJ7006` | A worker of the run panicked, which is a defect in njutest. The message names the worker's thread, what it was measuring (a mutation, or a package whose sources it was reading), and what the panic said, and every other worker stops after the item it holds. | report it with the message, which names the worker, what it was measuring and what it said; running it again meets the same panic |
| `NJ8001` | The run has nowhere to work: its scratch directory could not be made. Failing to *claim* one is a limitation, not an error. | check TMPDIR is a directory this user may write in, and that there is room under it |
| `NJ9001` | The reports offered to `njutest merge` are neither one whole report nor one complete division of one catalog: none were offered; a `K/N` label is missing, repeated, malformed, mixed with an unsharded report, or uses another N; the reports disagree about the tree, configuration, contract, effective scope, or tool versions; or two judged the same mutant. A partial union cannot be relabelled as the whole. | every part of one catalog has the same catalog digest; the parts offered do not, so they are not parts of one run |
| `NJ6003` | The report contradicts itself — the numbers do not add up, the verdict is more than what ran supports, a fact recorded as unavailable is also present — so nothing was written. | this is a defect in this tool: it refused to write a report whose parts disagree, rather than store one a reader could not trust |

## xtask

`cargo xtask` prints a failure as its code, a colon, then what it says.
A gate that refused because of another coded failure prints both: `XT0001` for the refusal, then the cause's code.
The first digit names an area: 0 the gates, their ledgers, and what runs them (the pre-push gate, the lanes, the other machines), 1 fixtures, 2 `proofaudit`, 3 `engine-audit`, 4 Kani and the model evidence, 5 audit specimens and lint sentinels, 6 identities and recordings, 7 `report-diff` and the bill of materials.

| Code | Meaning | Remedy |
| --- | --- | --- |
| `XT0001` | A gate refused the tree, and its message names every place it refused. | fix each place the message names, or the rule it names, and run the gate again |
| `XT0002` | A line of the seam allowlist is not one the ratchet can read. | write the line as the ledger's other lines are written |
| `XT0003` | The roadmap declares no milestone, or one twice. | give every milestone one row in the roadmap table |
| `XT0004` | The root manifest could not be read, or its workspace lint table is not one `fuzz-clippy` can carry to the fuzz workspace. | fix the table the message names in the root `Cargo.toml` |
| `XT0005` | `cargo clippy` could not be started for the fuzz workspace. | check `cargo` is on the path and the pinned toolchain is installed |
| `XT0006` | A published JSON schema under `schema/` does not compile, so nothing can be validated against it. | fix the schema the message names; `cargo xtask all` compiles every one |
| `XT0007` | A decision record under the ADR directory is misnamed, shares its number, carries another's heading, is listed wrongly in the book, or is named by a link to no record. | fix the record, the book, or the link the message names |
| `XT0008` | `docflows` could not check the workflows the documentation shows: a page could not be read, or actionlint could not be run or said something other than which workflows it refused. | check `actionlint` is installed, or name it with `--actionlint`, and read what it said |
| `XT0009` | actionlint refused a workflow the documentation shows. | fix the snippet the message names, so a reader who copies it has a workflow that runs |
| `XT0010` | The registry of critical decisions names an item the tree does not define, leaves a layer open that the gaps ledger does not give an owner, or lists a hole it does not have. | fix the cell, the ledger line, or the item the message names; a new critical decision arrives with what holds it |
| `XT0101` | The push names something the pre-push gate cannot check: an update Git did not give whole, an object other than the checked-out commit, a remote commit that is not here, a move that is not a fast-forward, or only deletions. | fetch the remote ref and push the checked-out commit as a fast-forward of it |
| `XT0102` | The tree the pre-push gate checks stopped being the pushed commit while it ran, or the check changed it. | leave the worktree alone while a push runs, then push again |
| `XT0103` | The check the pre-push gate runs failed. | read the check's own output above, fix what it names, and push again |
| `XT0104` | The check the pre-push gate runs was stopped: it outlived its budget, said nothing for longer than the gate allows, or the gate was asked to stop. | push again when the machine is less loaded, or raise the budget the message names |
| `XT0105` | The pre-push gate could not run its check: a program, Git, one of its own files, a setting, its lane, or its progress output failed it. | fix what the message names and push again |
| `XT0201` | The lane a whole-workspace run waits in could not be found, written, locked, or reported on. | set `NJUTEST_SLOT_DIR` to a writable directory, or fix the one the message names |
| `XT0202` | The run was asked to stop while it waited for its lane. | nothing is wrong with the tree; run it again |
| `XT0301` | A program a gate runs could not be started or watched, or the signals that stop it could not be armed. | check the program the message names is installed and that this process may be signalled |
| `XT0401` | `remote-check` could not ask the other machines: its machines file could not be read or names none, or a program, Git, a log, or the thread asking a machine failed it. | fix the machines file or what the message names, and run it again |
| `XT0402` | At least one other machine refused the commit. | read each machine's answer and fix what it names |
| `XT0501` | git could not list what the repository holds, so no gate can say what it read. | run the gate inside the repository's checkout, with git on the path |
| `XT0502` | git listed a path of the repository that is not UTF-8, which no path this repository holds is. | rename the path |
| `XT0503` | The repository holds a symbolic link, which a gate never follows. | replace the link with the file it points at |
| `XT0504` | A path git listed could not be read. | check the path the message names exists and is readable |
| `XT1001` | A fixture's tree could not be walked or one of its files read. | check the path the message names exists and is readable |
| `XT1002` | A fixture tree holds a symbolic link, which the checks never follow. | replace the link with the file it points at |
| `XT1003` | A fixture path is not UTF-8, so no protocol a fixture feeds could spell it. | rename the path |
| `XT1004` | A directory under a fixture group is not a fixture. | make it a fixture, with its manifest, lockfile and README, or move it out of the group |
| `XT1005` | A fixture's configuration is not TOML. | fix the file the message names |
| `XT2001` | The run directory holds no assurance report, or it could not be read. | point `proofaudit` at the directory a completed run wrote |
| `XT2002` | The assurance report is not JSON this audit can read. | re-run the run that wrote it; a report nothing can parse is not one to re-decide |
| `XT2003` | The runner's recording has a line that is not JSON. | re-run with `--trace`; a recording that lost a line cannot be counted as agreement |
| `XT2004` | The report is not one configured build measured whole, which is what this audit re-decides. | audit each part against its own recording |
| `XT2005` | The report departs from the published assurance-report schema, so a reader could meet an absent required field. | re-run with this release; a report off its schema is not one to re-decide |
| `XT2006` | A document given as a merged report is not a merge of shards. | give `proofaudit` the report `njutest merge` wrote, with `--shard` for each part |
| `XT2007` | A document given with `--shard` is not a shard of a catalog. | give each shard's own report or run directory to `--shard` |
| `XT2008` | The same shard was given twice with `--shard`. | give each shard once; counting one part twice is an operator's mistake, not a merge |
| `XT2009` | A shard was given that the merged report does not name among its sources. | give only the shards the merged report names in its composition |
| `XT2010` | The document is on its published schema and is not one this audit can read into a complete report or a shard. | report it; a document on its schema that this audit cannot read is a gap in the audit |
| `XT2011` | The report passed its schema and still lacks a field a layer of this audit reads, so the schema and the reader disagree. | report it; either the schema should require the field or the reader should not demand it |
| `XT2101` | A report's thread standing for a test binary contradicts what the engine recording witnesses, or is no standing a run gives. | the runner decided what its own recording does not support: re-run, and report it if it recurs |
| `XT2102` | A report's exploration of a binary's schedules comes to something other than its recorded controls do. | the runner decided what its own recording does not support: re-run, and report it if it recurs |
| `XT2103` | The recorded controls of an exploration are not a schedule the exploration could have run. | re-run with `--trace`; a recording that cannot be replayed cannot be counted as agreement |
| `XT2104` | The recording lacks the build or baseline record a thread standing is derived from. | re-run with `--trace` using this release |
| `XT2105` | A report's decision about a fault contradicts the fault's recorded executions. | the runner decided what its own recording does not support: re-run, and report it if it recurs |
| `XT2106` | A report's repair of a disposition contradicts the recorded execution of that repair. | the runner decided what its own recording does not support: re-run, and report it if it recurs |
| `XT3001` | The run directory holds no engine run report, or it or a document beside it could not be read. | point `engine-audit` at the directory a completed engine run wrote |
| `XT3002` | The engine run report is not JSON this audit can read. | re-run the run that wrote it |
| `XT3003` | An evidence document beside the engine run report is not one this audit can read. | re-run the run that wrote it |
| `XT3004` | The engine's recording is not one this audit can read, or is of another schema. | re-run with `--trace` using this release |
| `XT3005` | The configuration named as the ledger is not one this audit can read. | fix the configuration file the message names |
| `XT3006` | The document is not the engine run report, or is of another schema version. | point `engine-audit` at a run report this release wrote |
| `XT3007` | The engine run report departs from the published run-report schema, so a reader could meet an absent required field or a value of another shape. | re-run with this release; a report off its schema is not one to re-decide |
| `XT3008` | The carry page, `docs/engine/carry.md`, lacks a closed block the carry audit reads its lists from. | restore the fenced block the message names on the page |
| `XT4001` | The Kani export could not be read, or is not the closed JSON schema of the pinned release. | regenerate the export with the pinned Kani |
| `XT4002` | The Kani export's metadata, project or toolchain is not the pinned release run on this workspace. | regenerate the export here with the pinned Kani and backend |
| `XT4003` | A harness or check ledger of the Kani export is missing, duplicated, or not the selected production one. | regenerate the export from the production harness list |
| `XT4004` | A Kani proof's result, assertions, covers, properties, backend evidence or summary is not successful and exact. | read the harness the message names; a proof that does not hold is a defect to fix, not a gate to relax |
| `XT4005` | Kani's result arithmetic exceeded the type its evidence is held in. | report it; a count that cannot be held is a count this audit refuses to guess |
| `XT4006` | CBMC unfolded a production harness into more program steps than its entry in the harness table allows, so the law has started paying for state it does not reason about, which is what ran a 16 GB runner out of memory. | take the payload out of the law's subject, or raise the harness's ceiling in `xtask/src/kaniaudit.rs` in the same change that says why |
| `XT4101` | The report's model evidence is not the closed verified-v1 shape, or contradicts itself. | re-run the verified run that wrote it |
| `XT4102` | A retained model artifact is outside the run directory or cannot be read. | audit the run directory the artifacts were retained in |
| `XT4103` | A retained Kani export is not the pinned schema, or does not establish the answer the report gives. | read the model record the message names |
| `XT5001` | An audit specimen could not be laid out in a temporary directory. | check the temporary directory is writable |
| `XT5002` | An event of an audit specimen's recording is not an object, or lacks its envelope. | fix the specimen in the sentinel module the gate names |
| `XT5003` | A flat audit specimen could not be completed into the document a run writes. | fix the specimen in the sentinel module the gate names |
| `XT5101` | A planted text of the lint sentinels is not the header-and-files shape they are read in. | fix the planted text under `xtask/sentinels/` the message names |
| `XT6001` | An identity field exceeds the length prefix of the recipe it is minted by. | report it; an identity this recipe cannot spell is not one to truncate |
| `XT6002` | A line of a recording is not JSON. | re-run with `--trace` |
| `XT6003` | A line of a recording departs from its producer's published schema, so a reader could meet an absent required field. | re-run with `--trace` using this release; a recording off its schema is not one to re-decide |
| `XT6004` | A line of a recording passed its producer's schema and still lacks a field a reader of this audit reads, so the schema and the reader disagree. | report it; either the schema should require the field or the reader should not demand it |
| `XT7001` | A report given to `report-diff` is not one this version understands. | give it two reports this release wrote |
| `XT7002` | `cargo metadata` could not be read into a bill of materials. | run `cargo metadata --locked` and fix what it says |
