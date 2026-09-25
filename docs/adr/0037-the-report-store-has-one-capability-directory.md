<!--
SPDX-FileCopyrightText: 2026 njutest contributors
SPDX-License-Identifier: MIT OR Apache-2.0
-->

# 0037 — The report store has one capability directory

## Status

Proposed, 2026-09-25.

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
7. **A sharing violation on a rename or a removal is retried a bounded number of times**, because the indexer and antivirus open new files briefly; one that persists is `NJ6004` with the cause named, never a silent success.
8. **The unsafe code lives in one more named module, and a gate says which.** `capdir/windows.rs` joins `runner/windows.rs`, `runner/unix.rs`, and `tempowner/lock.rs`, and an xtask lint refuses `unsafe` and `expect(unsafe_code)` anywhere else, so the list is a rule rather than a comment; the same list feeds the strict-conversion policy.

## Consequences

- `docs/limitations.md` loses "What a run cannot do on Windows" and gains the durability sentence of decision 6 and the refused volumes of decision 4.
- Clippy runs on the Windows leg of CI, because nothing else lints `cfg(windows)` code.
- Retention of a run somebody is reading may be refused on Windows while the reader holds it open; that is `NJ6004`, not a race.
- The work lands in steps, each green on every platform: clippy on Windows and the Red test; `capdir` with the Unix backend; the store rewritten onto it; the Windows backend; the limitations rewritten.
