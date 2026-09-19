<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Interrupted assurance checkpoint v1

**Status: implemented.**

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

Only a kill and a runaway are inherited from a checkpoint. Both are
existential claims about this exact tree — something noticed the mutant, a
named test or a count of steps — and stay true however the next run routes.
A `waited` is not inherited: it is a fact about the machine that measured, and
the next machine is not that one. Every other disposition depends on
which tests the run decided could notice, and a resumed run routes at file
granularity, so it re-derives them rather than inheriting a claim it did not
make. A state that carries one of them is refused rather than read.

A resumed run states the limitation `resumed-from-checkpoint`, naming how much
it continued from. A run that finished removes its checkpoint: there is
nothing to continue from.
