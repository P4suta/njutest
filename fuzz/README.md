<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Fuzz targets

cargo-fuzz targets for the parts of the engine that read hostile input or
transform bytes. Each target states one property; a crash is a bug in the
property or the code, never in the input.

| Target | Property |
| --- | --- |
| `flatten` | never panics; an accepted result has no line break |
| `trace_reader` | never panics; accepted events round-trip through the writer's encoding |
| `glob` | never panics; a literal pattern matches its own spelling |
| `splice` | never panics; an accepted set yields a monotone offset map of the right length |
| `normalize_path` | never panics; a normalized path is a fixed point |
| `discover_file` | never panics; every candidate validates, is spanned from the source, sits inside its site; deterministic |

```sh
mise run fuzz:smoke                     # every target, 256 runs each
cargo +nightly fuzz run flatten         # one target, until interrupted
cargo +nightly fuzz run flatten -- -runs=100000
```

The crate is standalone (not a workspace member) because fuzzing needs
nightly and a sanitizer. Corpora and artifacts are ignored by git; a
reproducer worth keeping becomes a regular test.
