<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Interrupted assurance checkpoint v1

**Status: historical.** Current interrupted runs use the closed
[checkpoint v2](checkpoint-v2.md). This page is retained only to describe
v1 files; current readers never open them.

`njutest-assurance-checkpoint-v1` is strict scheduling state for continuing an
interrupted verification. It is never assurance evidence, never a partial
report, and never updates `latest-any` or `latest-full`.

One exact input identity owns one checkpoint:

```text
<user cache>/njutest/outcomes-v1/checkpoints/<identity>/checkpoint-v1.json
```

There is no resume flag: only a checkpoint under the newly computed,
identical identity is considered, and `--no-cache` keeps none. Configured
resource providers disable checkpoint reuse. A saved baseline target carries
the files it reached and not the coverage regions inside them, so it is routed
at file granularity for the rest of the run: a resumed run executes at least
the work a cold run would, never less.

Only a kill is inherited from a checkpoint. A named test noticing this exact
mutant remains an existential claim however the next run routes. Historical
v1 checkpoints may contain `runaway`; current readers deliberately ignore it.
Those records carried no matched control and therefore cannot establish that
the mutation caused the finite guard count to be crossed. `step-limit-reached`
and `waited` are likewise not inherited: each is an execution bound rather
than a verdict about the mutant. Every other disposition depends on
which tests the run decided could notice, and a resumed run routes at file
granularity, so it re-derives them rather than inheriting a claim it did not
make. A state that carries one of them is refused rather than read.

A resumed run states the limitation `resumed-from-checkpoint`, naming how much
it continued from. A run that finished removes its checkpoint: there is
nothing to continue from.
