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

## mjutest

| Code | Meaning |
| --- | --- |
| `MJ0001` | The caller cancelled the operation before it completed. |
