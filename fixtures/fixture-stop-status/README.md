<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-stop-status

A program that ends with a crash's exit status of its own ([ADR 0035](../../docs/adr/0035-a-crash-is-a-stop-the-next-run-has-to-survive.md)).
`save` returns 93 whenever a perturbation is active, just before its one call that writes, so a crash put at that call never stops the process there.
The exit status is the one a stop has and the runtime publishes no notice, so the stop is not one: njutest decides the crash `undecided`, and a report that called it restarted or corrupt is refused by the audit.
The engine's own score reads any non-zero exit as a kill, which is what the fates below record.

| Path | Unit | Candidates |
| --- | --- | --- |
| `src/lib.rs` | lib | one `crash-after-write` at the call that writes |

```fates --operator crash-after-write
src/lib.rs:16:5 crash-after-write killed
```
