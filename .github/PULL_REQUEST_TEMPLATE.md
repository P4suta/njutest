## Summary

<!-- One or two sentences: what this change does and why. -->

## Red

<!--
Development is test-driven. Paste the failing output of the test you wrote
first, before the change, so a reviewer sees that it failed for the stated
reason. A test that passed before the change is not evidence.
-->

```text
```

## Green and refactor

<!-- What the smallest honest change was, and what duplication it removed. -->

## Developer infrastructure

<!--
Each milestone carries its developer-facing deliverables as completion
criteria. Which of these did this change add or extend? Delete the rest.
-->

- [ ] tests (unit / property / fixture-driven / contract / golden)
- [ ] fuzz target
- [ ] trace events or diagnostics
- [ ] error codes (`docs/errors.md`)
- [ ] gates (`cargo xtask`), fixture, or benchmark
- [ ] documentation, ADR, or contract text

## Checklist

- [ ] `mise run check` passes locally.
- [ ] Commit messages follow Conventional Commits (`committed` checks them).
- [ ] The seam ledger (`xtask/seam_allowlist.txt`) did not grow.
