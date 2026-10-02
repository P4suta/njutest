<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0049 — OS records and sensitive values carry their boundaries

## Status

Accepted, 2026-10-01.
Implemented by `capdir::records`, `sensitive::Sensitive`, the `raw-buffer-pointer` and `sensitive-name` syntax gates, their planted regressions, and the security tasks in `mise.toml`.

## Context

PR #237 received eleven CodeQL review comments: two invalid-pointer accesses in the Windows ACL reader and nine sensitive-log flows from `libtest::account` and `libtest::accounts` into test diagnostics.
The ACL reader accepted a naked `*const ACL`, fetched naked ACE pointers, read whole structs without bounding them to `AceSize`, and advanced a pointer to a variable-length SID whose length it obtained by dereferencing it again.
The reproduced CodeQL path begins at `ace = null_mut()` and ends at each `read_unaligned`; the analyzer does not establish what `GetAce` writes through its out parameter.
A Windows regression also demonstrated a real bounds defect: an eight-byte allowing ACE borrowed a valid SID from ACL slack beyond its own `AceSize`, and the old reader accepted it.
The allocated descriptor was kept alive until after the copies, so its ordinary call path did retain its owner; the abstraction nevertheless carried neither that lifetime nor a byte range into the reader.
Token SIDs and rename payloads independently repeated typed pointer reads and buffer arithmetic.
Directory enumeration had a byte parser, but bounded a name to the batch rather than to the current record, allowing a malformed next-record offset to overlap it.
An FFI module was permission to write unsafe code, not a proof that any one read belonged to a live, sufficiently large record.

The libtest functions returned test counts, announced counts and failed test names from literal harness transcripts.
None of the nine values was a credential.
[CodeQL's Rust sensitive-data model](https://github.com/github/codeql/blob/main/rust/ql/lib/codeql/rust/security/SensitiveData.qll) makes a function invocation or variable access a sensitive source according to its name.
Its [shared name heuristic](https://github.com/github/codeql/blob/main/shared/concepts/codeql/concepts/internal/SensitiveDataHeuristics.qll) treats `account` as identity information, while exempting words such as `accounted` and `accounting` that describe counting.
The code had used the ambiguous noun and verb for harness reports, build verdicts and compiler refusals.
Removing useful panic diagnostics would have left the misleading source names and made tests harder to read.

Clippy checks unsafe syntax and types, but cannot infer an OS allocation's record bounds, ownership or the meaning of a domain name.
The existing xtask gates named the modules allowed to use unsafe and refused casts, but allowed pointer methods and did not classify credential-shaped values.
The existing Windows tests exercised valid OS records or already-decoded privacy grants, not malicious byte layouts.
The Miri CI job interpreted a fixture under the deep contract rather than the platform-independent ACL parser, which did not exist.
The Mac compiler did not compile the Windows module, and CodeQL supplied this feedback only in GitHub's post-push analysis.

## Decision

1. Initialized, aligned `Buffer` owns every variable-length buffer of the Windows capability directory.
   A borrowed `Record` uses checked addition and slice access for every field and subrecord.
   ACL revision, ACL size, ACE count, ACE size, SID revision and SID subauthority count are validated before copying a SID, and each ACE's SID is bounded by that ACE rather than the allocation.
   `GetKernelObjectSecurity` returns a self-relative descriptor directly into owned storage, replacing allocation-owned raw descriptor, owner and ACE pointers.
   A process token's SID pointer is read as an address in initialized bytes, checked against its still-owned buffer and converted to a bounded range; it is never dereferenced.
   Each directory name is bounded by its own record, and rename fields are written with checked slice copies.
   The only unsafe operations at these boundaries are the foreign calls themselves.
2. `raw-buffer-pointer` refuses raw memory reads and writes, pointer arithmetic and dereferences in unsafe scopes, aliased raw readers, explicit and inferred record-pointer casts, safe wrapping pointer arithmetic and macro-token forms outside the one storage module.
   That module is itself outside the unsafe allowlist and uses no unsafe code.
   Fixed typed structures passed by reference and opaque pointer arguments to FFI remain available.
3. Non-sensitive sources say what they are: `harness_report`, `harness_reports`, `build_verdicts`, `explains_a_failure`, `observed_summary` and `unreadable_file`.
   `sensitive-name` refuses credential-shaped value declarations, constants and statics unless their explicit type is the canonical `Sensitive<T>`, and refuses credential-shaped enum constructor names.
   Actual URL user information remains available to redirection as an explicitly exposed protected value, while `Debug` and `Display` on that representation always redact.
   The literal harness tests keep their diagnostics and assertions.
   The judgement entry point is a runtime function so its eight mutations remain measurable while the workspace conservatively retains const bodies linked to doctests outside native validation; its decision and Kani laws are unchanged.
   The public `judged` entry no longer offers const evaluation, and no workspace call site uses that capability.
4. Both lints are required by `mise run security:local` and `mise run check`.
   The official CLI's unfiltered Rust `security-extended` suite also runs offline as `mise run security:codeql`, required by `check`, using the complete CodeQL 2.27.1 bundle.
   `mise run setup:codeql` verifies and caches that bundle once before offline work; the check refuses a missing installation instead of silently skipping analysis or downloading queries.
   The official release has no native macOS ARM64 binary, but this campaign's Mac can execute its x86-64 bundle through Rosetta.
   The structural gate supplements the actual taint analysis and remains independently runnable.
   CI uses the same pinned bundle for both Rust and Actions analysis, requires a SARIF report with zero findings after upload, removes the pre-existing source-path exclusions, and adds Miri over the shared synthetic-record parser.
5. The parser's seven synthetic-buffer tests run on every platform, including Miri where available.
   A Windows integration test reaches real token SIDs and ACLs, enumerates more than one OS batch, renames to a long UTF-16 name and removes the whole directory.
   Cross-target Clippy reads that module even on a Mac.

## Sweep

The two regression tests were run before the new lints existed and both failed: zero of two passed.
The final structural rules found 13 raw-buffer-pointer diagnostics and 16 sensitive-name diagnostics across the baseline's 873 workspace source files.
All 13 pointer shapes were in the Windows capability directory; the naming sweep also reached build presentation, compiler refusals, the decision kernel, URL user information, an unreadable-file fixture and a diagnostics fixture constant.
The repaired tree has 878 workspace source files and zero findings of either class.
Its 17 planted pointer shapes and 13 planted sensitive-name shapes are required by the lint sentinel.
The unfiltered baseline CodeQL Rust suite scanned all 1,077 Rust files and reproduced exactly the eleven review findings, including their source lines and data flows.
The same complete bundle and 39 queries scanned all 1,082 Rust files in the repaired tree and reported zero findings.
The CLI's extraction metrics reported 531 files with errors and 842 without error; these metrics are retained as reported rather than substituted for the distinct source-path coverage.
Diagnostics include unbuilt proc macros and a Rust 1.98 versus 1.97 proc-macro-server mismatch, so zero findings do not claim complete semantic expansion.
The structural gates and compiled platform tests remain necessary alongside that analysis.
Verification passed on Mac (2,938 unit and suite tests plus 27 toolchain tests), Windows (213 related tests and MSVC Clippy), Linux (209 related tests), and Miri (seven parser tests).
All 48 production Kani harnesses were proved and independently audited, and the regenerated receipt retains eight killed mutations with their executions.
An uncached real fixture run's ten mutants and two targets were re-decided with zero violations and zero unaudited layers.
Every verification command and the extractor's limits are recorded in the Job G report.
No alert is dismissed, suppressed, filtered or excluded.
The authenticated alert/data-flow endpoint returned HTTP 401 because the stored `gh` credential was invalid.
The eleven public review comments and the reproduced baseline SARIF provide the source and data-flow evidence without changing alert state.
GitHub's alert state cannot change until the integrator pushes and the full CodeQL job evaluates this commit.
