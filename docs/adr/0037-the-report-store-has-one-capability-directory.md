<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0037 — The report store has one capability directory

## Status

Accepted, 2026-09-29.
Proposed 2026-09-25, and accepted once every step below had landed and the capdir laws and the store's laws ran on Windows.
The closed set of decision 1 also holds `sync_file`, which flushes a file held for reading, and `open_file_at`, which opens an explicit configuration without following it, both of which the store needs on Windows.

| Decision | Held by |
| --- | --- |
| 1, one capability directory and one store | `rust_mutants::capdir` and its `Name`; `app/reports.rs` with no platform split; the laws in `crates/rust-mutants/tests/capdir.rs`, `app::reports::tests` and `crates/njutest/tests/reports_store.rs` on every platform |
| 2, Unix unchanged | `capdir/unix.rs` on rustix; the laws that were Unix's, now shared |
| 3, handle-relative on Windows | `capdir/windows.rs`: `create`, `open_dir`, `open_entry`, `rename_held`, `dispose`, `file_status`; the shared laws with junctions for links, and `a_removed_or_replaced_entry_is_gone_by_name_while_another_handle_still_holds_it` |
| 4, the volume checked | `volume_of` and `volume_verdict` at `open`; `a_volume_is_trusted_only_as_ntfs_or_refs_with_posix_semantics_and_a_refusal_names_it` |
| 5, owner, system and administrators | `Private`, `privacy_of`, `restrict_to_owner`; `owner_only_is_a_protected_list_of_the_owner_the_system_and_the_administrators`, and the store's `same_owner_legacy_namespaces_are_tightened_before_claiming` on Windows |
| 6, a directory flush | `sync` and `sync_file`, through a second handle opened relative to the first with no name; `a_file_named_by_a_path_is_opened_without_following_a_link_and_flushed_through_any_handle`; the durability sentence in [limitations](../limitations.md) |
| 7, one owned operation after producer completion | `a_removal_another_process_holds_the_entry_against_is_refused_with_the_cause_named`; the original sharing refusal remains observable |
| 8, one list of modules | `xtask/src/lints/ffi.rs` and the `unsafe-outside-ffi` rule with its planted shapes, which `unchecked-cast` reads too; `unsafe_code_lives_only_in_the_modules_the_rule_names` |

## Context

A report is published through a capability rooted at the store's own directory, so that what a reader opens is the file this run wrote and not one a name was pointed at afterwards.
On Unix that capability is a directory descriptor, and every step of the store — claim, write, publish, index, census, retention — is an `openat`-family call relative to one.
Windows had no backend, so every Windows `njutest verify` measured, decided, printed, and then ended in `NJ6004` because it could not keep what it had decided ([limitations](../limitations.md)).

A Windows backend written beside the Unix one would copy the protocol — the census, the case-alias check, the index re-check, retention — into a second place, where it can drift from the audited first.
A backend that pins every directory from the workspace down with handles that refuse delete-sharing would bind names only as long as some other handle happens to be open, which is care rather than a type; it would also block the store's own renames and the person's editor for the whole run, and a junction retargeted on the path renames nothing, so a pin does not bind a spelling at all.

## Decision

1. **One capability directory in the engine, `rust_mutants::capdir`.** A `Dir` owns a directory handle and offers a closed set of operations: open a child directory or file, create a file or a private directory exclusively, the status of a child or of itself, rename without replacing, rename replacing (the index), remove a file or a directory, flush, list entries, and the owner check.
   A `Name` is one path component, refused if empty, `.`, `..`, containing a separator, `:`, or NUL, ending in a dot or a space, or a reserved device name, on every platform.
   The store is written against `Dir` on every platform, so there is one protocol and the compiler holds both platforms to it.
2. **Unix is rustix, unchanged in behaviour.** The existing Unix tests guard the rewrite; they are not edited while it lands.
3. **Windows is handle-relative, not pathname-relative.** Opens are `NtCreateFile` with `OBJECT_ATTRIBUTES.RootDirectory` set to the parent handle and `FILE_OPEN_REPARSE_POINT`, then the reparse tag is checked, which is `openat` with `O_NOFOLLOW`.
   Creation is `FILE_CREATE`, which never opens an existing entry.
   Renames are `NtSetInformationFile(FileRenameInformationEx)` on the source handle with the target's `RootDirectory` and no replace flag, which is `renameat` with `RENAME_NOREPLACE`, and stronger: the source is the object held, not a name.
   Removal is `FileDispositionInfoEx` with POSIX semantics on the verified handle.
   Identity is `FILE_ID_INFO`: the volume serial and the 128-bit file id.
4. **The volume is checked before anything is trusted to it.** Opening the store root asks `GetVolumeInformationByHandleW` for NTFS or ReFS with POSIX unlink and rename semantics; any other volume is refused with `NJ6004`, naming the file system, rather than served with weaker semantics nobody asked for.
5. **Privacy is the owner, the system, and the administrators.** Unix makes a store directory `0700`, which still admits root.
   Windows creates it with a protected DACL granting the owner, `SYSTEM`, and `Administrators`, and an existing owned directory whose DACL grants anyone else is tightened to the same, as Unix tightens a mode; a directory another user owns is refused, as Unix refuses one.
   An owner-only DACL would lock out backup, antivirus, and other administrators, which `0700` does not.
6. **A directory flush is required.** Windows has no documented directory `fsync`; `FlushFileBuffers` on the directory handle commits the journal up to that point, and a volume that refuses it is refused (decision 4) rather than trusted.
   The weaker statement is written down: on Windows a published rename is as durable as the NTFS journal after that flush.
7. **A rename or removal makes one capability-owned attempt after actual producer completion.**
   A sharing violation is `NJ6004` with the cause named; guessed retries do not establish that a writer or mapped executable has ended.
8. **The unsafe code lives in one more named module, and a gate says which.** `capdir/windows.rs` joins `njutest-process/src/windows.rs`, `njutest-process/src/unix.rs`, and `tempowner/lock.rs`, and an xtask lint refuses `unsafe` and `expect(unsafe_code)` anywhere else, so the list is a rule rather than a comment; the same list feeds the strict-conversion policy.

## Consequences

- `docs/limitations.md` loses "What a run cannot do on Windows" and gains the durability sentence of decision 6 and the refused volumes of decision 4.
- Clippy runs on the Windows leg of CI, because nothing else lints `cfg(windows)` code.
- Retention of a run somebody is reading may be refused on Windows while the reader holds it open; that is `NJ6004`, not a race.
- The work lands in steps, each green on every platform: clippy on Windows and the Red test; `capdir` with the Unix backend; the store rewritten onto it; the Windows backend; the limitations rewritten.
