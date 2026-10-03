// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use super::{Invocation, NativeCompileExpectation, NativeDoctestKind, SELF_BINARY, Variables};
use crate::sensitive::Sensitive;

fn refusal<T>(result: io::Result<T>) -> io::Error {
    match result {
        Err(error) => error,
        Ok(_) => panic!("invalid native custody was accepted"),
    }
}

fn returned<T>(result: io::Result<T>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic!("the original native control was refused: {error}"),
    }
}

#[cfg(unix)]
fn frame(bytes: &mut Vec<u8>, value: &[u8]) -> io::Result<()> {
    bytes.extend_from_slice(
        &u64::try_from(value.len())
            .map_err(io::Error::other)?
            .to_le_bytes(),
    );
    bytes.extend_from_slice(value);
    Ok(())
}

#[cfg(unix)]
fn record() -> io::Result<Vec<u8>> {
    let mut bytes = super::INVOCATION_SCHEMA.to_vec();
    bytes.push(0);
    frame(&mut bytes, b"/original/workspace")?;
    bytes.extend_from_slice(&3_u64.to_le_bytes());
    for value in [
        b"/original/helper".as_slice(),
        b"/original/capture",
        b"/original/rust_out",
    ] {
        frame(&mut bytes, value)?;
    }
    bytes.extend_from_slice(&2_u64.to_le_bytes());
    for (name, value) in [
        (b"PATH".as_slice(), b"/native/toolchain".as_slice()),
        (b"EXACT_BYTES", b"\xff"),
    ] {
        frame(&mut bytes, name)?;
        frame(&mut bytes, value)?;
    }
    Ok(bytes)
}

#[cfg(unix)]
#[test]
fn native_invocation_preserves_lossless_original_execution_conditions() {
    use std::os::unix::ffi::OsStrExt as _;
    let bytes = returned(record());
    let invocation = returned(super::invocation(&bytes));
    assert_eq!(invocation.cwd, Path::new("/original/workspace"));
    assert_eq!(
        invocation.argv.get(2).map(OsString::as_os_str),
        Some(std::ffi::OsStr::new("/original/rust_out"))
    );
    assert_eq!(
        invocation
            .environment
            .expose()
            .var("EXACT_BYTES")
            .map(std::ffi::OsStr::as_bytes),
        Some(b"\xff".as_slice())
    );
    assert_eq!(
        invocation.environment.expose().var("PATH"),
        Some(std::ffi::OsStr::new("/native/toolchain"))
    );
    let (environment, cwd) = returned(super::invocation_runtime_inputs(&bytes));
    assert_eq!(&environment, invocation.environment.expose());
    assert_eq!(cwd, invocation.cwd);
}

#[cfg(unix)]
#[test]
fn unknown_or_incomplete_native_custody_cannot_construct_an_invocation() {
    let original = returned(record());
    for end in 0..original.len() {
        let truncated = returned(
            original
                .get(..end)
                .ok_or_else(|| io::Error::other("invalid control truncation")),
        );
        assert_eq!(
            refusal(super::invocation(truncated)).kind(),
            io::ErrorKind::Other,
            "truncation at {end}"
        );
    }
    let mut unknown = original.clone();
    *returned(
        unknown
            .first_mut()
            .ok_or_else(|| io::Error::other("missing control schema")),
    ) = b'?';
    assert_eq!(
        refusal(super::invocation(&unknown)).kind(),
        io::ErrorKind::Other
    );
    let mut extra = original;
    extra.push(1);
    assert_eq!(
        refusal(super::invocation(&extra)).kind(),
        io::ErrorKind::Other
    );
}

fn observed() -> Invocation {
    Invocation {
        cwd: PathBuf::from("original"),
        argv: ["helper", "capture", "original-native"]
            .into_iter()
            .map(OsString::from)
            .collect(),
        environment: Sensitive::new(Variables::of([(
            OsString::from("PATH"),
            OsString::from("native-libraries"),
        )])),
    }
}

#[test]
fn merged_self_binding_preserves_original_environment_without_reusing_a_route() {
    let mut original = observed();
    let executable = Path::new("immutable-native");
    let environment = returned(super::execution_environment(
        &NativeDoctestKind::Merged,
        &original,
        executable,
    ));
    assert_eq!(environment.var(SELF_BINARY), Some(executable.as_os_str()));
    assert_eq!(
        environment.var("PATH"),
        original.environment.expose().var("PATH")
    );
    assert!(!original.environment.expose().holds(SELF_BINARY));
    let mut changed = original.environment.expose().clone();
    changed.set(crate::sealed::doctest::RUN_ONE, "0");
    original.environment = Sensitive::new(changed.clone());
    assert_eq!(
        refusal(super::execution_environment(
            &NativeDoctestKind::Merged,
            &original,
            executable
        ))
        .kind(),
        io::ErrorKind::Other
    );
    changed.remove(crate::sealed::doctest::RUN_ONE);
    changed.set(SELF_BINARY, "other-native");
    original.environment = Sensitive::new(changed.clone());
    assert_eq!(
        refusal(super::execution_environment(
            &NativeDoctestKind::Merged,
            &original,
            executable
        ))
        .kind(),
        io::ErrorKind::Other
    );
    changed.set(SELF_BINARY, "original-native");
    original.environment = Sensitive::new(changed);
    assert_eq!(
        returned(super::execution_environment(
            &NativeDoctestKind::Merged,
            &original,
            executable
        ))
        .var(SELF_BINARY),
        Some(executable.as_os_str())
    );
}

#[test]
fn compiler_only_attestations_and_standalone_panic_remain_distinct_from_runtime() {
    assert_eq!(
        super::compiler_expectation("example (line 1) - compile fail"),
        Some(NativeCompileExpectation::Refuses)
    );
    assert_eq!(
        super::compiler_expectation("example (line 2) - compile"),
        Some(NativeCompileExpectation::Compiles)
    );
    assert_eq!(super::compiler_expectation("example (line 3)"), None);
    let kind = NativeDoctestKind::Alone {
        name: "panic-example".to_owned(),
        expects: crate::sealed::doctest::Expects::Panic,
    };
    let original = observed();
    assert_eq!(
        returned(super::execution_environment(
            &kind,
            &original,
            Path::new("native")
        )),
        *original.environment.expose()
    );
}

#[test]
fn wasm_and_unknown_native_routes_cannot_be_native_products() {
    for host in [
        "aarch64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
        "unknown-host",
    ] {
        assert_eq!(
            refusal(super::native_format(host, b"\0asm\x01\0\0\0")).kind(),
            io::ErrorKind::Other,
            "{host}"
        );
    }
}

#[test]
fn native_program_diagnostics_preserve_but_never_display_captured_environment() {
    let mut invocation = observed();
    invocation.environment = Sensitive::new(Variables::of([(
        OsString::from("ORIGINAL_CAPTURED_SECRET"),
        OsString::from("synthetic-captured-value"),
    )]));
    let program = super::NativeDoctestProgram {
        executable: PathBuf::from("immutable-native"),
        kind: NativeDoctestKind::Merged,
        environment: invocation.environment.clone(),
        arguments: Vec::new(),
        invocation,
    };
    assert_eq!(program.environment(), program.observed_environment());
    assert_eq!(
        program
            .observed_environment()
            .var("ORIGINAL_CAPTURED_SECRET"),
        Some(std::ffi::OsStr::new("synthetic-captured-value"))
    );
    assert!(program.arguments().is_empty());
    let diagnostic = format!("{program:?} {program:#?}");
    assert!(diagnostic.contains("[redacted]"));
    assert!(!diagnostic.contains("ORIGINAL_CAPTURED_SECRET"));
    assert!(!diagnostic.contains("synthetic-captured-value"));
}
