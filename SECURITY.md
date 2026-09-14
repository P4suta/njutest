# Security policy

## Reporting a vulnerability

Please do not open a public issue for security problems.

Report vulnerabilities privately through GitHub:
<https://github.com/P4suta/njutest/security/advisories/new>

Include the affected version or commit, reproduction steps, and the impact
you observed. You will get an acknowledgement within a week; fixes are
published as a new release together with a security advisory.

## Supported versions

Only the latest release receives security fixes.

## Scope

njutest and rust-mutants run `cargo`, the project's test binaries, fuzz
targets, and mutants from the repository they are pointed at, and njutest can
write repairs into test files and fuzz corpora. Reports about escaping those
write boundaries, executing code outside the target repository or its
snapshot, reading files the tools should not read, or a mutant reaching the
source workspace are in scope.
