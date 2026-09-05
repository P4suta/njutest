# Error codes

Every failure the engine and the runner report carries a stable code. The
code is the searchable name of the failure: grep this file, the issue tracker,
and a trace for it. A test in each crate keeps this table and the code's own
list equal in both directions, so a code is either here or it does not exist.

Codes are `RM` (rust-mutants) or `MJ` (mjutest) followed by four digits. The
first digit names an area:

| Digit | rust-mutants | mjutest |
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

| Code | Meaning |
| --- | --- |
| `RM0001` | The caller cancelled the operation before it completed. |
| `RM0002` | A configuration file that could not be read. |
| `RM0003` | A configuration file that is not the document this version understands: an unknown key, a malformed value, a duration that is not a duration. |
| `RM0004` | A configuration that parses but says something a run cannot honour: an expectation without a reason, a harness flag the engine owns, a report directory outside the workspace. |
| `RM0005` | A configuration whose `version` is not one this release understands. |
| `RM0006` | A process environment that already selects a mutant or names a catalog, so nothing a test process said would be about this run. |
| `RM0007` | A stored run report that is not there or cannot be read. |
| `RM0008` | A file a command would write that is already there, and `--force` was not given. |
| `RM0009` | A report or configuration file that could not be written. |
| `RM0010` | A change set git could not be asked for: the tree is not a repository, or it does not know the revision. A run that could not see what changed never reads as a run that saw nothing change. |
| `RM1001` | Snapshot options that cannot be honoured, such as a report directory that is absolute or climbs out of the source root. |
| `RM1002` | A source root that is relative, cannot be read, or is not a directory. |
| `RM1003` | An operating system failure while reading a tree: a directory that cannot be listed, an entry that cannot be stat'ed. |
| `RM1004` | A symbolic link inside the source tree. Links are refused rather than followed or skipped; add an exclude pattern. |
| `RM1005` | A Windows reparse point (junction or mount point) inside the source tree. |
| `RM1006` | A file that is neither a directory nor a regular file: a device, a socket, a named pipe. |
| `RM1007` | A file name that cannot survive the round trip through a slash-separated relative path, such as one containing a backslash. |
| `RM1008` | The snapshot directory could not be created or claimed. |
| `RM1009` | A failure while copying the tree into the snapshot. |
| `RM1010` | A cleanup refused because the recorded directory does not look like a snapshot directory. The guard between a bug and a user's source tree. |
| `RM1011` | A snapshot directory that survived every removal attempt, usually a file still locked by a test binary on Windows. |
| `RM1012` | The cargo or rustc executable could not be found: an explicit path that is not a file, or a bare name absent from the search path. |
| `RM1013` | A `-vV` banner lacks its `release:` or `host:` line, so the toolchain cannot be named. |
| `RM1014` | A cargo command could not start, timed out, or exited unsuccessfully; cargo's own words follow. |
| `RM1015` | `cargo metadata` printed something that is not its document. |
| `RM1016` | A `--message-format=json` line is not a message. |
| `RM2001` | A dep-info file has no rule to read. |
| `RM2002` | An artifact's dep-info file could not be read, so the files its unit compiled are unknown. |
| `RM2003` | A source file a unit compiled could not be read. |
| `RM2004` | A source file the compiler accepted does not parse as Rust for the engine's parser, which may lag the compiler; the file and position are named. |
| `RM2005` | A unit compiled a file outside the workspace root, which the snapshot does not hold. |
| `RM2006` | The candidates could not be assembled into a catalog: a display-id collision or an incoherent candidate. |
| `RM2007` | A selected package is not a workspace member. |
| `RM3001` | A candidate is not in the catalog being instrumented, which means the two were computed from different trees. |
| `RM3002` | The source is not the one the candidates were discovered from. |
| `RM3003` | Two rewrite sites partially overlap, which a syntax tree cannot produce: an engine bug rather than a fact about the program. |
| `RM3004` | An alternative could not be folded onto one line. |
| `RM3005` | The guards could not be applied to the file. |
| `RM3006` | A guard would have moved a line, breaking the invariant every position depends on. |
| `RM3007` | A mutant index collides with the runtime's sentinel values. |
| `RM4001` | The tree does not compile before any mutant is live, so nothing about the failure is the mutants' doing. |
| `RM4002` | The mutants a compilation failure came from could not be isolated. |
| `RM4003` | An instrumented compilation could not be attempted at all: the tree could not be written, or the toolchain could not be reached. |
| `RM5001` | The workspace does not compile before anything is instrumented. |
| `RM5002` | The instrumented baseline fails a test the pristine tree passes, so every later result would be about the instrumentation. |
| `RM5003` | No mutant of the catalog answers to the identity or prefix given, or several do. |
| `RM5004` | No test target of the session answers to the name given. |
| `RM5005` | The workspace builds no test target, so no mutant can be measured. |
| `RM5006` | The instrumented tree could not be written. |
| `RM9001` | A rule name the canonical registry does not know. |
| `RM9002` | A pattern the caller gave is not a pattern. |
| `RM9003` | A duration the caller gave is not a duration: an empty text, a number without a unit, a unit without a number, an unknown unit, or a number no duration can hold. |

## mjutest

| Code | Meaning |
| --- | --- |
| `MJ0001` | The caller cancelled the operation before it completed. |
| `MJ1001` | The configuration file could not be read. |
| `MJ1002` | The configuration file is not the document this version understands: an unknown key, a malformed value. |
| `MJ1003` | The configuration says something a run cannot honour: a harness flag mjutest owns, an environment assignment, a resource that is both shared and exclusive, an acceptance without a reason. |
| `MJ1004` | The configuration names a version this release does not understand. |
| `MJ1005` | A configuration file is already there, and `init` was not told to replace it. |
| `MJ2001` | The tree a run is about could not be read: a directory that cannot be listed, a file that cannot be read, a lock file that is not the document cargo writes. |
| `MJ3001` | A test binary could not be asked what tests it holds. |
| `MJ3002` | The build could not be run: cargo would not start, or was stopped. A workspace that does not *compile* is a finding in the report, not this. |
| `MJ3003` | The build's output is not the message stream this version understands. |
| `MJ4001` | A coverage export could not be read. |
| `MJ4002` | The LLVM tools the toolchain ships are not installed (`rustup component add llvm-tools`). |
| `MJ4003` | `llvm-profdata` or `llvm-cov` failed. |
| `MJ4004` | A test process wrote no coverage profile at all: the build was not instrumented, or the process did not exit normally. |
| `MJ6001` | The report could not be written as JSON, which is an invariant failure rather than anything about the code under test. |
| `MJ6002` | A document is not the assurance report this version understands: an unknown field, a missing field, a value of the wrong shape. |
| `MJ6004` | The report could not be written where a reader will look for it. |
| `MJ6005` | There is no such run to answer about, or none at all. A command never answers about a different run than the one it was asked about. |
| `MJ8002` | A build cache layer could not be used: it holds files this program did not put there, or it could not be written. Never a reason to fail a run — the command builds without one. |
| `MJ8003` | The store of earlier answers could not be used, or a report was offered for storage that must not be stored. |
| `MJ8004` | A stored answer is not the answer it claims to be: a document that does not parse, that does not carry the identity it is filed under, or that does not satisfy the audit every durable report must. |
| `MJ8001` | The run has nowhere to work: its scratch directory could not be made. Failing to *claim* one is a limitation, not an error. |
| `MJ6003` | The report contradicts itself — the numbers do not add up, the verdict is more than what ran supports, a fact recorded as unavailable is also present — so nothing was written. |
