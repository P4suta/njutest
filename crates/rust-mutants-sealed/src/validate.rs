// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a module must be to run sealed: a core module with one plain memory, a WASI command, importing nothing outside the table.

use wasmparser::{Encoding, Parser, Payload, TypeRef};
use wasmtime::{ExternType, FuncType, Module};

use crate::error::{EntryFault, ImportFault, MemoryFault, SealedError};
use crate::imports::{IMPORT_MODULE, Scalar, WasiFunction};

/// Refuses bytes that are not a core module with exactly one plain, defined memory and no start section.
///
/// # Errors
/// [`SealedError::ModuleMalformed`], [`SealedError::ModuleComponent`], [`SealedError::ModuleMemory`] or [`SealedError::ModuleEntry`].
pub(crate) fn shape(bytes: &[u8]) -> Result<(), SealedError> {
    let malformed = |source| SealedError::ModuleMalformed { source };
    let refuse = |fault| SealedError::ModuleMemory { fault };
    let mut memories = 0_u32;
    for payload in Parser::new(0).parse_all(bytes) {
        let payload = payload.map_err(malformed)?;
        if let Payload::Version {
            encoding: Encoding::Component,
            ..
        } = payload
        {
            return Err(SealedError::ModuleComponent);
        }
        if let Payload::ImportSection(imports) = payload {
            for import in imports.into_imports() {
                if let TypeRef::Memory(_) = import.map_err(malformed)?.ty {
                    return Err(refuse(MemoryFault::Imported));
                }
            }
        } else if let Payload::MemorySection(section) = payload {
            for memory in section {
                let memory = memory.map_err(malformed)?;
                memories = memories.saturating_add(1);
                if memory.memory64 {
                    return Err(refuse(MemoryFault::Memory64));
                }
                if memory.shared {
                    return Err(refuse(MemoryFault::Shared));
                }
                if memory.page_size_log2.is_some() {
                    return Err(refuse(MemoryFault::CustomPageSize));
                }
            }
        } else if let Payload::StartSection { .. } = payload {
            return Err(SealedError::ModuleEntry {
                fault: EntryFault::StartSection,
            });
        }
    }
    if memories != 1 {
        return Err(refuse(MemoryFault::Count { count: memories }));
    }
    Ok(())
}

/// Refuses a compiled module that imports anything outside the table, or is not a WASI command.
///
/// # Errors
/// [`SealedError::ModuleImport`] or [`SealedError::ModuleEntry`].
pub(crate) fn interface(module: &Module) -> Result<(), SealedError> {
    for import in module.imports() {
        let refuse = |fault| SealedError::ModuleImport {
            module: import.module().to_owned(),
            name: import.name().to_owned(),
            fault,
        };
        if import.module() != IMPORT_MODULE {
            return Err(refuse(ImportFault::OtherModule));
        }
        let Some(function) = WasiFunction::named(import.name()) else {
            return Err(refuse(ImportFault::NotInTable));
        };
        let ExternType::Func(signature) = import.ty() else {
            return Err(refuse(ImportFault::NotAFunction));
        };
        if !matches_table(&signature, function) {
            return Err(refuse(ImportFault::Signature));
        }
    }
    let entry = |fault| SealedError::ModuleEntry { fault };
    match module.get_export("_start") {
        Some(ExternType::Func(signature))
            if signature.params().len() == 0 && signature.results().len() == 0 => {}
        _ => return Err(entry(EntryFault::NoStart)),
    }
    match module.get_export("memory") {
        Some(ExternType::Memory(_)) => Ok(()),
        _ => Err(entry(EntryFault::NoMemory)),
    }
}

/// Whether `signature` is exactly the one the table gives `function`.
fn matches_table(signature: &FuncType, function: WasiFunction) -> bool {
    let parameters: Vec<Option<Scalar>> = signature
        .params()
        .map(|parameter| Scalar::of(&parameter))
        .collect();
    let results: Vec<Option<Scalar>> = signature
        .results()
        .map(|result| Scalar::of(&result))
        .collect();
    let expected: Vec<Option<Scalar>> = function.parameters().iter().copied().map(Some).collect();
    let answers: Vec<Option<Scalar>> = if function.answers() {
        vec![Some(Scalar::I32)]
    } else {
        Vec::new()
    };
    parameters == expected && results == answers
}
