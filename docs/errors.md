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

## mjutest

| Code | Meaning |
| --- | --- |
| `MJ0001` | The caller cancelled the operation before it completed. |
