<!--
SPDX-FileCopyrightText: 2026 mjutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# fixture-families

One library whose source is `crates/rust-mutants/tests/testdata/syntax/families.input`
verbatim: at least one site for every v1 rule. The golden beside that
input lists every candidate; discovery over this fixture must find exactly
those 125 candidates in `src/lib.rs` and count one `test-code` skip for the
`assert!` in its test module.

## Fates

What one run of this fixture establishes for every mutation of it, and
for every candidate the compiler refused. The run is `rust-mutants run
--tier all --offline --locked`;
`cargo test -p rust-mutants-cli --test toolchain_fates` does it again and
refuses a difference, and `UPDATE_FATES=1` rewrites the block below.

```fates
src/lib.rs:7:13 true-to-false survived
src/lib.rs:8:13 false-to-true survived
src/lib.rs:9:5 return-true survived
src/lib.rs:9:7 and-to-or survived
src/lib.rs:9:12 or-to-and killed
src/lib.rs:14:8 negate-condition survived
src/lib.rs:14:10 lt-to-le survived
src/lib.rs:14:14 and-to-or survived
src/lib.rs:14:17 remove-not survived
src/lib.rs:14:21 eq-to-neq survived
src/lib.rs:15:9 delete-compound-assignment survived
src/lib.rs:15:15 add-assign-to-sub-assign survived
src/lib.rs:17:11 negate-loop-condition survived
src/lib.rs:17:17 lt-to-le survived
src/lib.rs:18:9 delete-compound-assignment survived
src/lib.rs:18:15 add-assign-to-sub-assign survived
src/lib.rs:20:8 negate-condition survived
src/lib.rs:20:10 le-to-lt survived
src/lib.rs:21:9 or-to-and survived
src/lib.rs:21:14 ge-to-gt survived
src/lib.rs:23:9 delete-compound-assignment survived
src/lib.rs:23:15 sub-assign-to-add-assign survived
src/lib.rs:25:8 negate-condition survived
src/lib.rs:25:10 neq-to-eq survived
src/lib.rs:25:15 or-to-and survived
src/lib.rs:25:20 gt-to-ge survived
src/lib.rs:26:9 delete-compound-assignment survived
src/lib.rs:26:15 mul-assign-to-div-assign survived
src/lib.rs:28:5 return-default survived
src/lib.rs:32:5 return-default survived
src/lib.rs:32:8 add-to-sub survived
src/lib.rs:32:13 mul-to-div survived
src/lib.rs:32:18 sub-to-add survived
src/lib.rs:32:23 div-to-mul survived
src/lib.rs:32:28 rem-to-mul survived
src/lib.rs:32:33 add-to-sub survived
src/lib.rs:32:38 band-to-bor survived
src/lib.rs:32:43 sub-to-add survived
src/lib.rs:32:48 bor-to-band survived
src/lib.rs:32:53 add-to-sub survived
src/lib.rs:32:58 xor-to-band survived
src/lib.rs:32:63 sub-to-add survived
src/lib.rs:32:68 shl-to-shr survived
src/lib.rs:32:74 add-to-sub survived
src/lib.rs:32:79 shr-to-shl survived
src/lib.rs:37:15 range-to-inclusive survived
src/lib.rs:38:9 delete-compound-assignment survived
src/lib.rs:38:11 add-assign-to-sub-assign survived
src/lib.rs:40:15 inclusive-to-range survived
src/lib.rs:41:9 delete-compound-assignment survived
src/lib.rs:41:11 sub-assign-to-add-assign survived
src/lib.rs:43:5 return-default survived
src/lib.rs:43:18 add-to-sub survived
src/lib.rs:43:22 inclusive-to-range survived
src/lib.rs:43:34 add-to-sub survived
src/lib.rs:47:29 question-to-unwrap survived
src/lib.rs:48:5 delete-call-statement survived
src/lib.rs:48:21 ignore-question-statement survived
src/lib.rs:48:21 question-to-unwrap survived
src/lib.rs:49:8 negate-condition survived
src/lib.rs:49:10 lt-to-le survived
src/lib.rs:52:5 return-ok-default survived
src/lib.rs:56:33 question-to-unwrap survived
src/lib.rs:57:5 return-default survived
src/lib.rs:57:5 return-some-default survived
src/lib.rs:61:15 lt-to-le survived
src/lib.rs:62:18 add-to-sub survived
src/lib.rs:63:21 sub-to-add survived
src/lib.rs:64:21 neq-to-eq survived
src/lib.rs:65:24 mul-to-div survived
src/lib.rs:68:15 gt-to-ge survived
src/lib.rs:72:5 delete-compound-assignment survived
src/lib.rs:72:7 mul-assign-to-div-assign survived
src/lib.rs:73:5 delete-compound-assignment survived
src/lib.rs:73:7 div-assign-to-mul-assign survived
src/lib.rs:74:5 delete-compound-assignment survived
src/lib.rs:74:7 rem-assign-to-mul-assign survived
src/lib.rs:75:5 delete-compound-assignment survived
src/lib.rs:75:10 band-assign-to-bor-assign survived
src/lib.rs:76:5 delete-compound-assignment survived
src/lib.rs:76:10 bor-assign-to-band-assign survived
src/lib.rs:77:5 delete-compound-assignment survived
src/lib.rs:77:10 xor-assign-to-band-assign survived
src/lib.rs:78:5 delete-compound-assignment survived
src/lib.rs:78:10 shl-assign-to-shr-assign survived
src/lib.rs:79:5 delete-compound-assignment survived
src/lib.rs:79:10 shr-assign-to-shl-assign survived
src/lib.rs:80:5 return-default survived
src/lib.rs:84:5 remove-unary-minus survived
src/lib.rs:84:5 return-default survived
src/lib.rs:88:19 max-to-min survived
src/lib.rs:88:26 add-to-sub survived
src/lib.rs:88:30 min-to-max survived
src/lib.rs:89:8 negate-condition survived
src/lib.rs:89:10 is-some-to-is-none survived
src/lib.rs:90:9 delete-compound-assignment survived
src/lib.rs:90:11 add-assign-to-sub-assign survived
src/lib.rs:92:8 negate-condition survived
src/lib.rs:92:10 is-none-to-is-some survived
src/lib.rs:93:9 delete-compound-assignment survived
src/lib.rs:93:11 add-assign-to-sub-assign survived
src/lib.rs:95:8 negate-condition survived
src/lib.rs:95:10 is-ok-to-is-err survived
src/lib.rs:96:9 delete-compound-assignment survived
src/lib.rs:96:11 add-assign-to-sub-assign survived
src/lib.rs:98:8 negate-condition survived
src/lib.rs:98:10 is-err-to-is-ok survived
src/lib.rs:99:9 delete-compound-assignment survived
src/lib.rs:99:11 add-assign-to-sub-assign survived
src/lib.rs:101:5 return-default survived
src/lib.rs:106:5 delete-call-statement survived
src/lib.rs:107:5 delete-assignment survived
src/lib.rs:108:5 delete-compound-assignment survived
src/lib.rs:108:7 add-assign-to-sub-assign survived
src/lib.rs:109:5 delete-call-statement survived
src/lib.rs:110:5 delete-call-statement survived
src/lib.rs:120:13 delete-compound-assignment survived
src/lib.rs:120:20 add-assign-to-sub-assign survived
src/lib.rs:121:13 return-true survived
src/lib.rs:121:20 gt-to-ge survived
src/lib.rs:125:13 return-default survived
src/lib.rs:127:24 lt-to-le survived
src/lib.rs:134:9 return-default survived
src/lib.rs:134:29 mul-to-div survived
src/lib.rs:134:48 gt-to-ge survived
```
