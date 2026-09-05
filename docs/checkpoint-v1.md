<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Interrupted assurance checkpoint v1

**Status: contract only.** Implemented in M4. The rules are goatest's
checkpoint v1; the text is ported when the store exists.

`mjutest-assurance-checkpoint-v1` is strict scheduling state for continuing an
interrupted verification. It is never assurance evidence, never a partial
report, and never updates `latest-any` or `latest-full`.

One exact input identity owns one checkpoint:

```text
.mjutest/cache/v1/<input-digest>/checkpoint-v1.json
```

There is no resume flag: only a checkpoint under the newly computed,
identical digest is considered. Configured resource providers disable
checkpoint reuse. A saved baseline target carries the files it reached and
not the coverage regions inside them, so it is routed at file granularity
for the rest of the run: a resumed run executes at least the work a cold run
would, never less.
