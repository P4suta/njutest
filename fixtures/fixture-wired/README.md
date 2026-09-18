<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-wired

Two dependencies a run actually starts, and a suite that acts on what one of
them says and not on what the other does.

`place` posts an order and is told which one it is; `tests/orders.rs` asserts
that identifier. `ping` tells the other service this program is alive and
throws the answer away; `tests/health.rs` calls it and asserts nothing. Both
services are the same program, `examples/fake_upstream.rs`, which binds a port
of its own, tells the run where it is, and answers until the run stops it.

Each seam sees exactly one exchange, from a test of its own. That is on
purpose: a fault names an exchange by its place in the order, so a fixture
where two tests shared a seam would have its questions renamed by whatever
order the harness happened to run them in, and a fate table that moved with
the scheduler is not a fate table.

## What this fixture is the only place to check

Every other suite for the seam layer holds a piece of it. This one starts real
listeners, hands a real address to a real suite through a real provider, and
puts every question the recording licensed back to it. It is the end-to-end
acceptance ADR 0004 asks for, and the table below is the artifact.

## Fates

The mutation block is empty, and that is what a run of this tree establishes:
`rust-mutants run` does not start the dependencies, so the suite fails before
anything is mutated and the run is refused rather than reaching a fate. What
this fixture is for is the seam block under it.

```fates
```

## Seams

Every question the two recordings licensed and what a run established about
it, as `capability:seq rule decision [who decided it]`. The run is
`njutest verify --offline --locked`;
`cargo test -p njutest-cli --test toolchain_wire_fixture` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block.

```seams
health:0 truncate-response proved no-body-to-cut
health:0 delay-response unnoticed
health:0 drop-connection unnoticed
health:0 replay-request unnoticed
health:0 status-server-error unnoticed
health:0 status-not-found unnoticed
orders:0 truncate-response tests fixture-wired/test/orders
orders:0 delay-response unnoticed
orders:0 drop-connection tests fixture-wired/test/orders
orders:0 replay-request unnoticed
orders:0 status-server-error tests fixture-wired/test/orders
orders:0 status-not-found tests fixture-wired/test/orders
```

## What the table says, in words

The orders seam is held up: the four faults that change what the caller is
handed are all noticed, because a test that asserts the identifier cannot be
handed a 500, a 404, a cut body or a dead connection and still pass.

The health seam is held up by nothing. A test that calls a dependency and
asserts nothing about the answer would not notice that dependency being
down, and this is the run saying so.

Two questions nothing notices on either seam are worth more than the rest:

- `delay-response` holds the answer for the `hold` the configuration names —
  300ms here, because how slow is too slow is a property of the system under
  test and not of this tool. Neither client has a deadline of its own, so
  neither notices.
- `replay-request` delivers the request twice and hands the caller the first
  answer, which is what a retry after a lost answer does. Nothing here holds
  the dependency's state, so nothing notices. A suite that does not notice
  this is a suite that would not notice a double charge.

And one question is not answered by running anything: cutting the body of an
answer that has no body leaves the bytes identical, so no observer could tell,
and the run says `proved` rather than measuring it.
