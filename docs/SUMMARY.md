<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Summary

- [Architecture](architecture.md)
- [The assurance contract](assurance-contract.md)
- [Configuration](configuration.md)
- [Limitations](limitations.md)
- [Error codes](errors.md)
- [Continuous integration](ci.md)
- [Development](development.md)
- [Releasing](release.md)
- [Roadmap](roadmap.md)

# Formats

- [Report v1](report-v1.md)
- [Trace v1](trace-v1.md)
- [Checkpoint v1](checkpoint-v1.md)
- [Resource protocols](protocols.md)

# The engine

- [Architecture](engine/architecture.md)
- [Getting started](engine/getting-started.md)
- [Command line](engine/command-line.md)
- [Configuration](engine/configuration.md)
- [Operators](engine/operators.md)
- [Proofs](engine/proofs.md)
- [Reports](engine/reports.md)
- [Trace](engine/trace.md)
- [JSON Schema](engine/json-schema.md)
- [Troubleshooting](engine/troubleshooting.md)
- [Upgrading](engine/upgrading.md)
- [Comparison with cargo-mutants](engine/comparison-with-cargo-mutants.md)

# Decisions

- [0001 Seam policy](adr/0001-seam-policy.md)
- [0002 Trace is not evidence](adr/0002-trace-is-not-evidence.md)
- [0003 No replay engine](adr/0003-no-replay-engine.md)
- [0004 Proof layers, not budgets](adr/0004-proof-layers-not-budgets.md)
- [0005 The build cache mjutest owns](adr/0005-build-cache-mjutest-owns.md)
- [0006 Every temporary directory has an owner](adr/0006-every-temporary-directory-has-an-owner.md)
- [0007 Survived evidence is universal](adr/0007-survived-evidence-is-universal.md)
- [0008 Compiler-validated acceptance](adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)
- [0009 Soundness replaces race](adr/0009-soundness-replaces-race.md)
- [0010 Target directories are the cache layers](adr/0010-target-directories-are-the-cache-layers.md)
- [0011 The runtime lives at the end of each instrumented file](adr/0011-the-runtime-lives-at-the-end-of-each-instrumented-file.md)
- [0012 One workspace, two products](adr/0012-one-workspace-two-products.md)
- [0013 Codegen identity is the equivalence proof](adr/0013-codegen-identity-is-the-equivalence-proof.md)
- [0014 The guards are the measurement](adr/0014-the-guards-are-the-measurement.md)
- [0015 The guard is the infection probe](adr/0015-the-guard-is-the-infection-probe.md)
- [0016 The probe tree is a tree nobody needs](adr/0016-the-probe-tree-is-a-tree-nobody-needs.md)
- [0018 The assurance layer rides the standard interfaces](adr/0018-the-assurance-layer-rides-the-standard-interfaces.md)
