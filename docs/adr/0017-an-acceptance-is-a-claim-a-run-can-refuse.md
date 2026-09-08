<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0017 — An acceptance is a claim a run can refuse

## Status

Accepted, 2026-09-09. Implemented by `Locator::count`,
`LocateError::Counted`, `Session::locate_all`, and the resolution of one
expectation against every mutant it names in `run::verify`.

## Context

A surviving mutant is a hole in the tests or a mutation that changes nothing.
Telling the two apart is the whole of what a reviewer does with a run's
output, and `[[mutation.expect]]` is where the second answer is written down.

Writing it down is easy to do badly. A day of closing this engine's own
survivors produced three mistakes, all of them in reasons that read as
convincing prose:

**A reason can be true of one mutation and false of the one it names.** The
`Err` arm of `witnessed` restored the sources and returned the error. The
reason written for it — the restore after the check writes the same files, so
the run ends with the same error and the same tree — is exactly right. What
made it wrong is that the same locator would have covered the mutation that
never restores anything, and nothing in the reason distinguishes them.

**A reason can hold today by coincidence rather than by construction.** That
same acceptance was defensible only because every file the pass can fail to
write is a file the later restore fails on too, so the caller gets the same
error either way. That is not an invariant. It is what happens when the cause
of the failure is still there a moment later, and the guard exists for the
case where it is not.

**"the same value comes back" is not "the expression need not run".** A
`return-default` on a call to `serve` was accepted because `serve` carries
`EXIT_ASSURED` out of every path and `EXIT_ASSURED` is zero. The mutation does
not replace the value; it replaces the call, and the server never runs. The
run said `stale` and was right.

Two of those three were caught by the tooling rather than by the person
writing them, and the third was caught by the run. That is the pattern worth
writing down.

## Decision

An acceptance is a claim the run resolves, and everything about its shape is
there so the run can refuse it.

**A locator names what the claim is about, in full.** Path, item, rule, and
the bytes the edit replaces are all required, and a locator that names more
than one mutation is `unmatched` rather than a licence over all of them. The
strictness is not a formatting rule. Requiring the bytes an edit replaces is
what makes the writer say which mutation the reason is about, and that is the
question the first mistake above skipped.

**A reason that rests on an invariant names the test that holds it.** An
acceptance is only as good as the test on the thing it appeals to. `Phase`
ends its phase in `Drop`, so the explicit `end()` at the tail of a function
changes no recording — and the ledger's entry for it names
`a_phase_guard_ends_its_phase_once_with_its_duration_and_phases_nest`, which
drops a guard without calling `end` and still finds the `phase-end`. A reason
that appeals to an invariant nobody tests is a reason that will be true until
it silently is not.

**A reason that holds by coincidence is not an acceptance.** Where the
equivalence rests on two failures happening to coincide, or on a condition
that a deterministic test cannot arrange, the answer is to change the code
until the rule is observable, not to write the coincidence down. Both products
did this on the same day: the engine deleted `witnessed` so the tree is put
back in exactly one place, and the language server read a framed message with
`take(length)` and a `limit() == 0` check so that a body shorter than its
header is refused for saying so rather than for failing to parse.

**One reason may cover several mutations, and then it is checked against every
one of them.** `count = N` on a locator says how many mutations the reason was
written for. Two things then have to hold at once: the catalog holds exactly
that many, so a mutation added or removed at that place stops the claim rather
than joining it, and every one of them came to the declared outcome, so a
claim covering three stops holding the moment a test kills one of the three.
Without the second half the count would fix the population and leave the claim
unchecked — a claim that goes on exempting two mutations on the strength of a
test that killed the third.

## Consequences

- `Locator` carries `count`, `Session::locate_all` resolves a locator to the
  set it names, and `run::verify` requires the declared outcome of each. A
  claim that did not hold marks none of them expected: marking the ones that
  did would exempt them on the strength of a test that killed another.
- The report says `covered` for a claim that names more than one, so an audit
  can see how wide a single reason is without re-deriving it from the catalog.
- `unmatched-expectation` keeps its meaning: a locator that names a number of
  mutations other than the one it was written for verifies nothing.
- The engine's own ledger holds six acceptances. Three name the test that
  holds the invariant they rest on. None of them is written for a mutation the
  writer had not read.

## Alternatives

- **Let a locator cover whatever it matches.** This is the licence the count
  exists to refuse. A reason written about one mutation says nothing about a
  second that shares a path, an item, a rule and the bytes it replaces, and
  the second is exactly the mutation a later edit adds.
- **Disambiguate with `line` instead.** A line moves whenever anything above
  it does, which is the reason locators exist at all. It separates two
  mutations that are genuinely different; it cannot say that one reason is
  about both.
- **Name each of them by identity.** An identity is minted from the whole
  file's digest, so every acceptance written that way is stale after the next
  edit anywhere in the file. That is a ledger nobody maintains, and an
  unmaintained ledger is read as noise rather than as claims.
