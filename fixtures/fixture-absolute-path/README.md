<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-absolute-path

A library that reads a setting beside its manifest by a relative path, `setting.txt`, or from the machine's root by an absolute one, `/setting.txt`, and one integration test that reads it by the relative path.

A sealed instance starts in its package's directory, as cargo starts a test there, through the guest's own `chdir`, so the relative path reads the setting beside the manifest and the test passes its control sealed.
The mutations that send the test to `/setting.txt` instead, negating the condition or making it true, fail natively, since the machine's root holds no such file.
Sealed, `/setting.txt` names no place in any tree the instance holds, so it is refused and recorded as an escape, and the failure that follows it is doubted, `refused`: each is unproven, and its native kill is a lead.
Before the guest started in its directory, an absolute path no tree named was read from the working directory as the relative path it became, so both read the package's own `setting.txt`, passed, and survived sealed, where natively the test kills them.

## Fates

What one run of this fixture establishes for every mutation of it, and for every candidate the compiler refused.
The run is `rust-mutants run --tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:9:5 return-default killed
src/lib.rs:9:8 condition-to-false survived
src/lib.rs:9:8 condition-to-true unproven
src/lib.rs:9:8 negate-condition unproven
src/lib.rs:10:9 return-default unreached
src/lib.rs:10:9 string-to-empty unreached
src/lib.rs:12:9 return-default killed
src/lib.rs:12:9 string-to-empty killed
src/lib.rs:19:5 return-default killed
src/lib.rs:19:5 return-some-default killed
src/lib.rs:20:21 return-default killed
src/lib.rs:20:21 return-some-default killed
src/lib.rs:21:25 return-some-default unreached
```
