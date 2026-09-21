# Error codes

**Status: implemented.** Every code here is one the engine or the runner can return, and a test keeps this table and `rust_mutants::error::error_codes` in step in both directions.

Every failure the engine and the runner report carries a stable code.
The code is the searchable name of the failure: grep this file, the issue tracker,
and a trace for it.
A test in each crate keeps this table and the code's own list equal in both directions, so a code is either here or it does not exist.

Codes are `RM` (rust-mutants) or `NJ` (njutest) followed by four digits.
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
| `RM0004` | A configuration or a flag that parses but says something a run cannot honour: an expectation without a reason, a harness flag the engine owns, a report directory outside the workspace, a shard that is not a part of something, a line range that addresses no line, a run name a directory cannot be, a `--file` this run does not read, a `--rule` or `--family` this release does not know. | the message says which key and why; `rust-mutants init` writes one that is valid |
| `RM0005` | A configuration whose `version` is not one this release understands. | this release reads version 1; a newer file needs a newer release |
| `RM0006` | A process environment names any mutation-control value unless this is an instrumented engine binary carrying the same nonempty catalog and exactly one nonempty activation or touch mode. | unset the RUST_MUTANTS_ variable the message names and run again |
| `RM0007` | A stored run report or recording that is not there or cannot be read. | `rust-mutants report --list` names the runs that are stored under this root |
| `RM0008` | A file a command would write that is already there, and `--force` was not given. | remove the file, or name another path: a command here never writes over what it did not write |
| `RM0009` | A report or configuration file that could not be written. | check the directory exists and this user may write in it; the path names the file |
| `RM0010` | A change set git could not be asked for: the tree is not a repository, or it does not know the revision. A run that could not see what changed never reads as a run that saw nothing change. | run inside a git working tree, or name what to measure with --include |
| `RM0011` | The reports given are not the parts of one catalog: a different tree, a different catalog, or two parts that hold the same mutant. | merge the parts of one run: the same tree, the same catalog, and one report per --shard |
| `RM0012` | A source file a report names cannot be read from the root given, so the mutation cannot be shown as a change. | pass --root at the tree the run measured, or check the file out again |
| `RM0013` | An outcome cache cannot be enumerated completely, so its size or removal cannot be stated exactly. | check the cache directory is readable by this user, or pass --cache-dir at another one |
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
| `RM1012` | The cargo or rustc executable could not be found: an explicit path that is not a file, or a bare name absent from the search path. | install the toolchain, or name cargo with --cargo, or put it on the PATH this process was given |
| `RM1013` | A `-vV` banner lacks its `release:` or `host:` line, so the toolchain cannot be named. | the toolchain answered something this release cannot read; `rustup update` and try again |
| `RM1014` | A cargo command could not start, timed out, or exited unsuccessfully; cargo's own words follow. | run the same cargo command yourself: what it says there is what it said here |
| `RM1015` | `cargo metadata` printed something that is not its document. | run `cargo metadata` yourself on this tree; what it prints is what could not be read |
| `RM1016` | A `--message-format=json` line is not a message. | run the same cargo command with --message-format=json yourself; what it prints is what could not be read |
| `RM1017` | The workspace reads code from a path outside itself, which the copy a run measures does not hold. Allow the directory with `--allow-outside`, or vendor it inside the tree. | --allow-outside DIR copies that directory into the copy where the tree reaches it, or [project] allow_outside does |
| `RM1018` | `--root` names a member of a workspace rather than the workspace. A run measures a copy of what it was given, and a member on its own is not a buildable tree. | run with --root at the workspace root the message names, and --package to narrow it |
| `RM1019` | A directory a run would copy has no place in the copy that keeps every path into it resolving: it is not absolute, it still climbs, it is the tree or holds it or is inside it, or it lies on another filesystem root. A copy places what it holds by substituting one prefix, so a directory it cannot place is one every path into it would stop reaching. | --allow-outside takes an existing absolute directory outside the tree and on the same filesystem root as it; a copy reproduces the shape of what it copies, and cannot hold a directory that is the tree, holds it, or lies across a volume |
| `RM1020` | A manifest a run has to read is there and could not be read. A run decides what it may copy, which targets carry a harness, and which lints a crate forbids from these, and an empty answer to any of them is a different run rather than a missing one. | read the manifest the message names yourself: a run decides what it may copy, which targets carry a harness, and which lints a crate forbids from it, and an empty answer to any of those is a different run rather than a missing one |
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
| `RM4001` | The tree does not compile before any mutant is live, so nothing about the failure is the mutants' doing. | make `cargo test --no-run` pass on the tree as committed, then run again |
| `RM4002` | The mutants a compilation failure came from could not be isolated. | run `cargo test --no-run` on the tree yourself; the compilation failed for a reason this tool could not attribute to one mutant |
| `RM4003` | An instrumented compilation could not be attempted at all: the tree could not be written, or the toolchain could not be reached. | the compilation could not be started at all: check cargo runs on this tree and that there is room under TMPDIR |
| `RM5001` | The workspace does not compile before anything is instrumented. | make `cargo test --no-run` pass on the tree as committed, then run again |
| `RM5002` | A target fails with nothing active, so no outcome under a mutation would be about the mutation. Verification does not stop at the first: every target is run and the refusal names all of them, because somebody is about to fix what it names. The run type-checks the pristine tree rather than running it, so this says what it observed and not that the tree was passing before. | [execution] skip_targets leaves that target out; --no-verify makes every result a result about instrumentation |
| `RM5003` | No mutant of the catalog answers to the identity or prefix given, or several do. | `rust-mutants catalog` lists what this run holds; an identity is re-minted whenever its file changes, so name the mutation by `path:item:rule` instead |
| `RM5004` | No test target of the session answers to the name given. | `rust-mutants catalog` names every target this session built; a name no target has starts nothing |
| `RM5005` | The workspace builds no test target, so no mutant can be measured. | the workspace has nothing that tests, so there is nothing a mutation could be put to; write a test, or point --root at the workspace that has them |
| `RM5006` | The instrumented tree could not be written. | check there is room under TMPDIR and that nothing is removing the run's directory while it writes |
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
| `NJ2001` | The tree a run is about could not be read: a directory that cannot be listed, a file that cannot be read, a lock file that is not the document cargo writes. | run this inside the tree you mean to verify, or pass --root at it |
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
| `NJ6001` | The report could not be written as JSON, which is an invariant failure rather than anything about the code under test. | this is a defect in this tool: a report it built could not be written as JSON |
| `NJ6002` | A document is not the assurance report this version understands: an unknown field, a missing field, a value of the wrong shape. | the document is from another release or another tool; `njutest verify` writes one this release reads |
| `NJ6004` | The report could not be written where a reader will look for it. | check the report directory exists and this user may write in it; the path names the file |
| `NJ6005` | There is no such run to answer about, or none at all. A command never answers about a different run than the one it was asked about. | `njutest report --list` names the runs that are stored under this root |
| `NJ8003` | The store of earlier answers could not be used, a report was offered for storage that must not be stored, or the stream answers were being carried on or off this machine stopped. | remove the store and let it be rebuilt: what is in it is read-only evidence and nothing is lost |
| `NJ8004` | A stored answer is not the answer it claims to be, or a line offered to this machine is not an answer at all: a document that does not parse, that does not carry the identity it is filed under, or that does not satisfy the audit every durable report must. | remove the store and let it be rebuilt: a stored answer that is not what it claims is never used |
| `NJ8005` | No port could be listened on in front of a seam, so a run that was to record what went past it could record nothing. | check this machine allows a listener on the loopback interface, and that nothing has taken every port |
| `NJ7001` | The toolchain has no `cargo miri`, and the `deep-v1` contract promises the suite is interpreted. Install it (`rustup +nightly component add miri`) or verify under `standard-v1`. | `rustup +nightly component add miri`, or ask for a contract that does not promise interpretation |
| `NJ7002` | The verified model-checking phase could not preserve its own evidence. | the message names the internal source or artifact boundary that failed; fix its permissions or report the invariant failure |
| `NJ7003` | The run could not schedule measurements without trusting state interrupted by a panic. | run it again; a worker panic or poisoned coordination lock is never recovered as ordinary state |
| `NJ7004` | An assurance phase printed output that is not valid UTF-8, so its text protocol cannot be interpreted exactly. | the named tool violated its text-output contract; fix or replace that tool before trusting its result |
| `NJ8001` | The run has nowhere to work: its scratch directory could not be made. Failing to *claim* one is a limitation, not an error. | check TMPDIR is a directory this user may write in, and that there is room under it |
| `NJ9001` | The reports offered to `njutest merge` are neither one whole report nor one complete division of one catalog: none were offered; a `K/N` label is missing, repeated, malformed, mixed with an unsharded report, or uses another N; the reports disagree about the tree, configuration, contract, effective scope, or tool versions; or two judged the same mutant. A partial union cannot be relabelled as the whole. | every part of one catalog has the same catalog digest; the parts offered do not, so they are not parts of one run |
| `NJ6003` | The report contradicts itself — the numbers do not add up, the verdict is more than what ran supports, a fact recorded as unavailable is also present — so nothing was written. | this is a defect in this tool: it refused to write a report whose parts disagree, rather than store one a reader could not trust |
