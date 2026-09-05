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

## mjutest

| Code | Meaning |
| --- | --- |
| `MJ0001` | The caller cancelled the operation before it completed. |
| `MJ1001` | The configuration file could not be read. |
| `MJ1002` | The configuration file is not the document this version understands: an unknown key, a malformed value. |
| `MJ1003` | The configuration says something a run cannot honour: a harness flag mjutest owns, an environment assignment, a resource that is both shared and exclusive, an acceptance without a reason. |
| `MJ1004` | The configuration names a version this release does not understand. |
| `MJ3001` | A test binary could not be asked what tests it holds. |
