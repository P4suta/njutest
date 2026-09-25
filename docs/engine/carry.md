<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# Carrying an answer across an edit

**Status: implemented** as evidence; the rule that carries an answer by it is not.
A run keeps, in `skeletons-v1.json` (`schema/rust-mutants-skeletons-v1.json`), every cataloged item's body digest and whether the body is sealed, and every compiled unit's skeleton.
Why an answer may be carried at all is ADR 0041 (#151).
This page is the specification the audit implements; the engine's code is one implementation of it.

## Sealed bodies

A body is sealed when everything it contributes to the program is its own execution.
An edit inside a sealed body can change only what an execution that enters it does.

Only the body of a function, a method, or a trait method with a default is ever sealed.
A body is not sealed when any of these holds, and `unsealed` names the first that does, in this order:

1. `unread`: no unit read the item's file.
2. `unlocated`: the catalog's body span names no bytes of the file.
3. `evaluated`: the item is a `const fn`, a `const` or a `static`, which the compiler can evaluate where nothing enters it.
4. `compile-time`: a unit that read the item's file is a procedural macro or a build script, whose code runs in the compiler, where no test enters it, and decides what other code is.
5. `unlocated`: the catalog's body span is not a function body of the file as the parser reads it.
6. `attribute`: an attribute off the list is on the file, on an inline `mod`, `impl` or `trait` around the item, or on the item.
   An attribute is on the list when its path is one segment named in `sealable-attributes`, or its first segment is named in `tool-namespaces`.
   A `cfg_attr` is on the list when every attribute it would apply is.
   `test`, `should_panic` and `ignore` are on it because the harness entry they generate is built from the function's name and attributes, which are outside the body.
7. `opaque-type`: the function is `async`, or its return type holds an `impl` type anywhere.
   The body decides that hidden type, and a caller observes it, through its size, its name, or its layout, without entering the body; an `async` function's body runs only when its future is first polled.
8. The first of these inside the body, in source order:
   - `attribute`: an attribute off the list;
   - `macro`: a macro invocation whose path is not one segment named in `sealable-macros`, or two segments whose first is named in `standard-roots` and whose second is named in `sealable-macros`;
   - `macro`, too: a macro invoked inside the arguments of a listed one.
     The arguments are read as tokens, not parsed: every identifier path followed by `!` and a delimited group is an invocation unless the path's last segment is a keyword, and the rule applies to it and to its own arguments in turn;
   - `declares-item`: any item: `fn`, `struct`, `enum`, `union`, `impl`, `trait`, `type`, `use`, `mod`, `macro_rules!` or another item macro, `const`, `static`, `extern crate`, an `extern` block, or an item the parser keeps as tokens;
   - `const-block`: an inline `const { … }` block.
9. The first of these in the files the unit read, in byte order of their names:
   - `shadowed`: the file declares `macro_rules!` with a name in `sealable-macros`, or a `use` makes a name in `sealable-macros` visible, directly or with `as`, from a path whose first segment is not in `standard-roots`;
   - `foreign-glob`: the file imports `*` from a path whose first segment is in neither `standard-roots` nor `local-roots`, or takes a crate that is not in `standard-roots` with `#[macro_use] extern crate`.
     A glob reaches every module below the one that holds it, so this unseals every body of the unit.

Rule 9 reads every file of the unit that parses as a whole Rust file, whatever its extension.
A file that does not, such as a data file or an included expression, can declare no macro another file sees.
A body is sealed only if it is sealed in every unit that read its file; rule 9 names the first such unit in the order the build reported them.

### Lists

```sealable-macros
assert
assert_eq
assert_ne
cfg
column
concat
dbg
debug_assert
debug_assert_eq
debug_assert_ne
eprint
eprintln
file
format
format_args
line
matches
module_path
panic
print
println
stringify
todo
unimplemented
unreachable
vec
write
writeln
```

`include`, `include_str`, `include_bytes`, `env`, `option_env`, `asm` and `global_asm` are left off on purpose: each reads something outside the body.

```standard-roots
std
core
alloc
```

```local-roots
self
super
crate
```

```sealable-attributes
allow
cfg
cfg_attr
cold
deny
deprecated
doc
expect
forbid
ignore
inline
must_use
should_panic
test
track_caller
warn
```

```tool-namespaces
clippy
diagnostic
rustfmt
```

## Body digests

An item's `body_digest` is the lowercase hex SHA-256 of the bytes `touched-v1.json`'s `items[].body` names, braces included, as the pristine file holds them.
Every item is named by `item`, the reference an entered union names it by: its package, its file, and its `ordinal`.

## Skeletons

A unit is named by its package's name, its target's name, its target's kinds joined by `,`, and whether it is the test build; never by a package id, which carries an absolute path.

Its skeleton is the SHA-256 of one line `<name>\0<digest>\n` per entry, in byte order of the names, and `entries` keeps every one of those names with its digest, so a reader can fold them again and check any entry it can read:

- every file the unit's dep-info names, as `$root/<path>` under the workspace root or `$target/<path>` under the target directory, each path with forward slashes; a file under neither is the lock file's to key and has no entry.
  The digest is the SHA-256 of the file's bytes with every sealed body of it replaced by `{sealed:<name>#<ordinal>/<newlines>:<bytes>:<characters>}`.
  `<name>` is the entry's name, `<ordinal>` is the `ordinal` of the item's reference, its position among the file's cataloged items from 0, `<newlines>` is how many line feeds the body holds, and `<bytes>` and `<characters>` are the length of its last line, after its last line feed, in bytes and in UTF-8 characters.
  The shape is there because a body's length decides where everything after it is: a line added inside a sealed body moves the line `panic!`, `line!()`, `Location::caller()` and a backtrace report for every line after it;
- every variable rustc recorded reading, as `$env/<NAME>`, with the value `unset`, or `set:` and the SHA-256 of the value with the run's own workspace root and target directory spelled `$root` and `$target`;
- every build script the unit's package ran, as `$emitted/<out_dir>` with the directory spelled the same way.
  The digest is the SHA-256 of one line `<kind>\0<value>\n` for each `cargo::rustc-cfg` (`cfg`), `rustc-env` (`env`, as `NAME=value`), `rustc-link-lib` (`lib`) and `rustc-link-search` (`path`) it emitted, each kind's values sorted and spelled the same way.
  A build script's own unit has no such entry.

So an edit to anything outside a sealed body, a signature, a type, a constant, a trait `impl` header, a macro, an unsealed body, a file the build included, a variable, or what a build script emitted, moves the skeleton of every unit that read it, and an edit inside a sealed body moves only that item's digest.

## The rule

A run that keeps outcomes also keeps, for every mutant it decides, a carried record under `rust-mutants/carried-v1/` in the cache directory.
The record is filed under the mutation's locus: the item whose body the edit is inside, named by package, path and ordinal, that body's digest, the edit's offsets from the body's start, its replacement, and its rule; and everything the exact key holds except the closure.
A mutant whose edit is inside no item body has no locus and nothing carried.

The record lists every execution the answer rests on, in the order the route made them: its target, the tests it named, the tree's skeleton, every item its process entered with that item's body digest, and how much of the process the list accounts for.
The tree's skeleton is the fold of every unit's skeleton, one line `<package>\0<target>\0<kind>\0<test>\0<skeleton>\n` per unit in byte order, which is coarser than the units a target links and sound for it.

After the exact key misses, a run reads the record under the mutation's locus and believes it only when every premise holds:

- a kill: its killing execution named everything it entered up to the kill, its target is one the route runs with the same tests, its target held its reach under a control of this tree, the skeleton is unchanged, and every item it entered has the same, sealed, body;
- a survival: every target the route runs has a recorded execution with the same tests that named everything it entered, and each meets the rest of what a kill's execution does; no target that reaches the mutant starts a process the run cannot see into.

The trace's `cache` record says `rule: carried` for this lookup, and `refused` names the first premise that failed: `skeleton-changed`, `item-changed`, `unsealed`, `entry-incomplete`, `route-grew`, `filter-differs`, `reach-moved` or `uncontrolled`.
A believed record is reported like an exact one, with the run that established it as `source_run_id`.
