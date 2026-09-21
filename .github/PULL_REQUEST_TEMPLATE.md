<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

## What this changes

<!-- One paragraph. The title is the changelog line, so write it as one. -->

## Red

<!--
 Paste the output of the test failing for the stated reason, before the change.
 A test that passed before the change is not evidence.
 See CONTRIBUTING.md.
-->

```console

```

## Checklist

- [ ] `mise run check` and `cargo xtask all` are green.
- [ ] Every `Status:` line I changed is true of the code in this branch.
- [ ] Every fixture I touched states each mutant's fate in its `README.md`.
- [ ] The documentation ledger tests pass: a set the code names and a page
      names are still the same set.
- [ ] `UPDATE_GOLDEN=1` diffs are in this branch and I read them.
- [ ] An engine change: `mise run dogfood:engine:audit` reports 0 violations.
- [ ] A new error variant has a code and a row in `docs/errors.md`.
- [ ] A new trace event type is in `EVERY_TYPE`, `events.golden`, the schema,
      and the table in `docs/engine/trace.md`.
- [ ] A behaviour a user would notice has a section in
      `docs/engine/upgrading.md`.
