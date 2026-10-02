// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a module must be to run sealed, and the typed refusal of everything else.

#![expect(clippy::panic, reason = "a test reports a setup failure by panicking")]

use rust_mutants_sealed::{
    EntryFault, ImportFault, MemoryFault, SealedCode, SealedError, WasiFunction,
};

use crate::common::{SPECIFICATION, runner};

/// The bytes of the module `text`.
fn assembled(text: &str) -> Vec<u8> {
    match wat::parse_str(text) {
        Ok(bytes) => bytes,
        Err(error) => panic!("valid WAT: {error}\n{text}"),
    }
}

/// What preparing the module `text` answers.
fn prepared(text: &str) -> Result<(), SealedError> {
    runner().prepare(&assembled(text)).map(|_module| ())
}

/// A command with `memory` and `rest` besides a `_start` that does nothing.
fn module(memory: &str, rest: &str) -> String {
    format!("(module {memory} {rest} (func (export \"_start\")))")
}

/// The memory every valid module here has.
const MEMORY: &str = "(memory (export \"memory\") 1)";

#[test]
fn a_plain_command_is_accepted() {
    if let Err(refused) = prepared(&module(MEMORY, "")) {
        panic!("a plain command was refused: {refused}");
    }
}

#[test]
fn a_component_is_refused() {
    let bytes = wat::parse_str("(component)").expect("valid WAT");
    let refused = runner()
        .prepare(&bytes)
        .expect_err("a component is refused");
    assert!(
        matches!(refused, SealedError::ModuleComponent),
        "{refused:?}"
    );
    assert_eq!(refused.sealed_code(), SealedCode::ModuleComponent);
}

#[test]
fn memories_sealed_execution_does_not_run_are_refused_each_by_what_it_is() {
    let cases = [
        (
            module("(memory (export \"memory\") i64 1)", ""),
            MemoryFault::Memory64,
        ),
        (
            module("(memory (export \"memory\") 1 1 shared)", ""),
            MemoryFault::Shared,
        ),
        (
            module("(memory (export \"memory\") 1 (pagesize 1))", ""),
            MemoryFault::CustomPageSize,
        ),
        (
            "(module (import \"env\" \"memory\" (memory 1)) (func (export \"_start\")))".to_owned(),
            MemoryFault::Imported,
        ),
        (
            module("(memory (export \"memory\") 1) (memory 1)", ""),
            MemoryFault::Count { count: 2 },
        ),
        (module("", ""), MemoryFault::Count { count: 0 }),
    ];
    for (text, fault) in cases {
        match prepared(&text) {
            Err(SealedError::ModuleMemory { fault: refused }) => assert_eq!(refused, fault),
            other => panic!("{text} was not refused for {fault}: {other:?}"),
        }
    }
}

#[test]
fn imports_outside_the_table_are_refused_each_by_what_is_wrong_with_it() {
    let cases = [
        (
            "(import \"env\" \"args_get\" (func (param i32 i32) (result i32)))",
            ImportFault::OtherModule,
        ),
        (
            "(import \"wasi_unstable\" \"args_get\" (func (param i32 i32) (result i32)))",
            ImportFault::OtherModule,
        ),
        (
            "(import \"wasi_snapshot_preview1\" \"sock_open\" (func (param i32 i32 i32) (result i32)))",
            ImportFault::NotInTable,
        ),
        (
            "(import \"wasi_snapshot_preview1\" \"args_get\" (global i32))",
            ImportFault::NotAFunction,
        ),
        (
            "(import \"wasi_snapshot_preview1\" \"args_get\" (func (param i32 i64) (result i32)))",
            ImportFault::Signature,
        ),
        (
            "(import \"wasi_snapshot_preview1\" \"proc_exit\" (func (param i32) (result i32)))",
            ImportFault::Signature,
        ),
    ];
    for (import, fault) in cases {
        let text = format!("(module {import} {MEMORY} (func (export \"_start\")))");
        match prepared(&text) {
            Err(SealedError::ModuleImport { fault: refused, .. }) => {
                assert_eq!(refused, fault, "{import}");
            }
            other => panic!("{import} was not refused for {fault}: {other:?}"),
        }
    }
}

#[test]
fn a_module_that_is_not_a_wasi_command_is_refused() {
    let cases = [
        (
            format!("(module {MEMORY} (func $f) (start $f) (func (export \"_start\")))"),
            EntryFault::StartSection,
        ),
        (format!("(module {MEMORY})"), EntryFault::NoStart),
        (
            format!("(module {MEMORY} (func (export \"_start\") (param i32)))"),
            EntryFault::NoStart,
        ),
        (
            "(module (memory 1) (func (export \"_start\")))".to_owned(),
            EntryFault::NoMemory,
        ),
    ];
    for (text, fault) in cases {
        match prepared(&text) {
            Err(SealedError::ModuleEntry { fault: refused }) => assert_eq!(refused, fault),
            other => panic!("{text} was not refused for {fault}: {other:?}"),
        }
    }
}

#[test]
fn bytes_that_are_not_webassembly_are_refused_as_malformed() {
    let refused = runner()
        .prepare(b"\0asm\x01\0\0\0\x05\xff")
        .expect_err("a truncated section is refused");
    assert_eq!(
        refused.sealed_code(),
        SealedCode::ModuleMalformed,
        "{refused}"
    );
    let refused = runner()
        .prepare(b"not webassembly")
        .expect_err("text is refused");
    assert_eq!(
        refused.sealed_code(),
        SealedCode::ModuleMalformed,
        "{refused}"
    );
}

#[test]
fn a_nondeterministic_proposal_is_refused_by_the_compiler() {
    let text = format!(
        "(module {MEMORY} (func (export \"_start\") (drop (f32x4.relaxed_madd (v128.const i64x2 0 0) (v128.const i64x2 0 0) (v128.const i64x2 0 0)))))"
    );
    match prepared(&text) {
        Err(refused) => assert_eq!(
            refused.sealed_code(),
            SealedCode::ModuleUncompiled,
            "{refused}"
        ),
        Ok(()) => panic!("relaxed SIMD compiled"),
    }
}

#[test]
fn every_function_of_the_specification_links_under_its_own_signature() {
    let imports: Vec<String> = SPECIFICATION
        .iter()
        .map(|(name, signature)| {
            format!("  (import \"wasi_snapshot_preview1\" \"{name}\" (func {signature}))")
        })
        .collect();
    let text = format!(
        "(module\n{}\n  {MEMORY}\n  (func (export \"_start\")))",
        imports.join("\n")
    );
    if let Err(refused) = prepared(&text) {
        panic!("the specification's own imports were refused: {refused}");
    }
}

#[test]
fn the_table_is_the_specification_function_for_function() {
    let table: Vec<&str> = WasiFunction::ALL
        .iter()
        .map(|function| function.name())
        .collect();
    let specification: Vec<&str> = SPECIFICATION
        .iter()
        .map(|(name, _signature)| *name)
        .collect();
    assert_eq!(table, specification);
    for function in WasiFunction::ALL {
        assert_eq!(WasiFunction::named(function.name()), Some(function));
    }
    assert_eq!(WasiFunction::named("fd_open"), None);
}
