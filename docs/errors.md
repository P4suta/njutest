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

## mjutest

| Code | Meaning |
| --- | --- |
| `MJ0001` | The caller cancelled the operation before it completed. |
