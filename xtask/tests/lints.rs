// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The gate that keeps lossy Rust shapes and comments beside code out.

#![expect(
    clippy::expect_used,
    reason = "a test reports a setup failure by panicking"
)]

use xtask::lints::{
    Kind, SourceRedirect, manual_variant_lists_across, opaque_proc_macro_synthesis,
    open_and_closed_across, proc_macro_exports, scan_source, source_redirects,
};

fn kinds(source: &str) -> Vec<Kind> {
    scan_source("a.rs", source)
        .expect("the source parses")
        .into_iter()
        .map(|finding| finding.kind)
        .collect()
}

#[test]
fn ordinary_code_is_not_a_finding() {
    assert_eq!(
        kinds("//! A file.\n\npub fn f(x: i32) -> i32 {\n    x + 1\n}\n"),
        []
    );
}

#[test]
fn lossy_text_conversions_cannot_turn_distinct_bytes_or_paths_into_one_string() {
    for source in [
        "//! A file.\nfn read(bytes: &[u8]) { drop(String::from_utf8_lossy(bytes)); }\n",
        "//! A file.\nfn read(bytes: &[u8]) { drop(std::string::String::from_utf8_lossy(bytes)); }\n",
        "//! A file.\nfn name(path: &std::path::Path) { drop(path.to_string_lossy()); }\n",
        "//! A file.\nuse std::string::String::from_utf8_lossy as decode;\n",
        "//! A file.\nmacro_rules! decode { ($bytes:expr) => { String::from_utf8_lossy($bytes) } }\n",
        "//! A file.\nmacro_rules! name { ($path:expr) => { $path.to_string_lossy() } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::LossyText),
            "lossy conversion must be a typed refusal or a lossless renderer: {source}"
        );
    }
    for source in [
        "//! A file.\nfn read(bytes: &[u8]) -> Result<&str, std::str::Utf8Error> { std::str::from_utf8(bytes) }\n",
        "//! A file.\nfn name(path: &std::path::Path) -> Option<&str> { path.to_str() }\n",
        "//! A file.\nfn explain() { println!(\"from_utf8_lossy is prohibited\"); }\n",
    ] {
        assert_eq!(
            kinds(source),
            [],
            "the exact boundary remains available: {source}"
        );
    }
}

#[test]
fn values_cannot_escape_their_destructors_through_forget() {
    for source in [
        "//! A file.\nfn leak(value: String) { std::mem::forget(value); }\n",
        "//! A file.\nuse std::mem::forget as leak;\n",
        "//! A file.\nmacro_rules! leak { ($value:expr) => { core::mem::forget($value) } }\n",
        "//! A file.\nfn leak(value: String) { let held = std::mem::ManuallyDrop::new(value); drop(held); }\n",
        "//! A file.\nstruct Held(std::mem::ManuallyDrop<String>);\n",
        "//! A file.\nuse std::mem::ManuallyDrop as Held;\n",
        "//! A file.\nmacro_rules! held { ($value:expr) => { core::mem::ManuallyDrop::new($value) } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::ForgottenValue),
            "destructor semantics must be the same in proofs and shipped code: {source}"
        );
    }
    assert_eq!(
        kinds("//! A file.\nfn finish(value: String) { drop(value); }\n"),
        [],
        "an explicit ordinary drop still runs the destructor"
    );
}

#[test]
fn domain_newtypes_cannot_implicitly_erase_to_their_representation() {
    for source in [
        "//! A file.\nstruct MutantId(String);\nimpl std::ops::Deref for MutantId { type Target = str; fn deref(&self) -> &str { &self.0 } }\n",
        "//! A file.\nuse std::ops::Deref as Coerce;\nstruct MutantId(String);\nimpl Coerce for MutantId { type Target = str; fn deref(&self) -> &str { &self.0 } }\n",
        "//! A file.\nstruct MutantId(String);\nimpl AsRef<str> for MutantId { fn as_ref(&self) -> &str { &self.0 } }\n",
        "//! A file.\nstruct MutantId(String);\nimpl std::borrow::Borrow<str> for MutantId { fn borrow(&self) -> &str { &self.0 } }\n",
        "//! A file.\nstruct MutantId(String);\nimpl Into<String> for MutantId { fn into(self) -> String { self.0 } }\n",
        "//! A file.\nstruct MutantId(String);\nimpl From<MutantId> for String { fn from(id: MutantId) -> Self { id.0 } }\n",
        "//! A file.\nstruct Payload(Vec<u8>);\nimpl From<Payload> for Vec<u8> { fn from(value: Payload) -> Self { value.0 } }\n",
        "//! A file.\nstruct Message(String);\nimpl Into<std::sync::Arc<str>> for Message { fn into(self) -> std::sync::Arc<str> { self.0.into() } }\n",
        "//! A file.\nstruct Bytes(Vec<u8>);\nimpl AsRef<[u8]> for Bytes { fn as_ref(&self) -> &[u8] { &self.0 } }\n",
        "//! A file.\nstruct Text(String);\nimpl From<Text> for Box<str> { fn from(value: Text) -> Self { value.0.into_boxed_str() } }\n",
        "//! A file.\nuse std::convert::AsRef as Erase;\nstruct MutantId(String);\nimpl Erase<str> for MutantId { fn as_ref(&self) -> &str { &self.0 } }\n",
        "//! A file.\nmacro_rules! scalar { () => { impl Deref for Id { type Target = str; } } }\n",
        "//! A file.\nmacro_rules! scalar { () => { use std::ops::Deref as Coerce; } }\n",
        "//! A file.\nmacro_rules! scalar { () => { impl From<Id> for String { fn from(id: Id) -> Self { id.0 } } } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::ImplicitScalarErasure),
            "implicit representation coercion must be refused: {source}"
        );
    }
    assert_eq!(
        kinds(
            "//! A file.\nstruct MutantId(String); struct Invalid;\nimpl MutantId { fn as_str(&self) -> &str { &self.0 } fn into_inner(self) -> String { self.0 } }\nimpl TryFrom<String> for MutantId { type Error = Invalid; fn try_from(value: String) -> Result<Self, Self::Error> { Ok(Self(value)) } }\n"
        ),
        []
    );
}

#[test]
fn critical_values_cannot_turn_overflow_into_a_plausible_number() {
    for source in [
        "fn count(value: Result<u64, E>) -> u64 { value.unwrap_or(u64::MAX) }",
        "fn count(value: Result<u64, E>) -> u64 { value.unwrap_or(0) }",
        "fn count(value: u64) -> u64 { value.saturating_add(1) }",
        "fn count(value: u64) -> u64 { u64::saturating_mul(value, 2) }",
        "use u64::saturating_add as add; fn count(value: u64) -> u64 { add(value, 1) }",
        "macro_rules! count { ($value:expr) => { $value.saturating_sub(1) } }",
    ] {
        let found = scan_source("crates/rust-mutants/src/work.rs", source).expect("it parses");
        assert!(
            found
                .iter()
                .any(|finding| finding.kind == Kind::FabricatedOverflow),
            "critical arithmetic must refuse overflow instead of inventing a number: {source}: {found:?}"
        );
    }

    assert!(
        scan_source(
            "crates/rust-mutants/src/work.rs",
            "fn count(left: u64, right: u64) -> Option<u64> { left.checked_add(right) }",
        )
        .expect("it parses")
        .is_empty(),
        "checked arithmetic keeps overflow in the type"
    );
    assert!(
        scan_source(
            "crates/rust-mutants/src/count.rs",
            "fn count(value: u64) -> u64 { value.saturating_add(1) }",
        )
        .expect("it parses")
        .iter()
        .any(|finding| finding.kind == Kind::FabricatedOverflow),
        "the central Count type is an evidence-bearing arithmetic boundary"
    );
    assert!(
        scan_source(
            "crates/demo/src/presentation.rs",
            "fn room(width: usize) -> usize { width.saturating_sub(1) }",
        )
        .expect("it parses")
        .is_empty(),
        "presentation geometry has a visibly saturating, non-persisted policy"
    );
}

#[test]
fn cfg_excluded_platform_code_cannot_hide_an_unchecked_cast() {
    for source in [
        "fn handle(raw: *mut core::ffi::c_void) -> HANDLE { raw as HANDLE }",
        "fn size(value: usize) -> u32 { value as u32 }",
        "macro_rules! handle { ($raw:expr) => { $raw as HANDLE } }",
    ] {
        let found = scan_source("crates/rust-mutants/src/runner/windows.rs", source)
            .expect("the Windows source parses");
        assert!(
            found
                .iter()
                .any(|finding| finding.kind == Kind::UncheckedCast),
            "a host build cannot be the only cast checker: {source}: {found:?}"
        );
    }
    assert!(
        scan_source(
            "crates/rust-mutants/src/runner/windows.rs",
            "fn size(value: usize) -> Result<u32, core::num::TryFromIntError> { u32::try_from(value) }",
        )
        .expect("the Windows source parses")
        .is_empty(),
        "a checked conversion preserves the failure branch"
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "the inline hostile-source corpus keeps each ownership proof and counterexample visible to the syntax gate"
)]
fn raw_threads_and_processes_must_live_behind_an_owner() {
    for source in [
        "fn run(work: impl FnOnce() + Send + 'static) { std::thread::spawn(work); }",
        "fn run(builder: std::thread::Builder, work: impl FnOnce() + Send + 'static) { builder.spawn(work); }",
        "fn run(command: &mut std::process::Command) { command.spawn(); }",
        "fn run(scope: &std::thread::Scope<'_, '_>) { scope.spawn(|| {}); }",
        "use std::thread::spawn as detach; fn run() { detach(|| {}); }",
        "macro_rules! run { ($command:expr) => { $command.spawn() } }",
        "macro_rules! call { ($function:path) => { $function(|| {}) } } call!(std::thread::spawn);",
    ] {
        assert!(
            kinds(source).contains(&Kind::UnownedSpawn),
            "a raw spawn must not make cleanup conventional: {source}"
        );
    }

    for source in [
        r"
            use std::process::{Child, Command};
            struct ProcessOwner { child: Child, reaped: bool }
            impl ProcessOwner {
                fn begin(command: &mut Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child, reaped: false })
                }
                fn wait(&mut self) -> std::io::Result<()> {
                    self.child.wait().map(|status| { self.reaped = true; drop(status); })
                }
            }
            impl Drop for ProcessOwner {
                fn drop(&mut self) { if self.wait().is_err() { self.reaped = false; } }
            }
        ",
        r"
            use std::process::{Child, Command};
            struct FailClosedOwner { child: Child }
            impl FailClosedOwner {
                fn launch(command: &mut Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child })
                }
                fn poll(&mut self) -> std::io::Result<Option<std::process::ExitStatus>> {
                    self.child.try_wait()
                }
            }
            impl Drop for FailClosedOwner {
                fn drop(&mut self) {
                    match self.child.try_wait() {
                        Ok(Some(_status)) => {}
                        Ok(None) | Err(_source) => ownership_failure(),
                    }
                }
            }
            fn ownership_failure() -> ! { std::process::abort(); }
        ",
        r"
            use std::thread::{Builder, JoinHandle};
            struct WorkerOwner { handle: Option<JoinHandle<()>> }
            impl WorkerOwner {
                fn begin() -> std::io::Result<Self> {
                    Builder::new().spawn(|| {}).map(|handle| Self { handle: Some(handle) })
                }
                fn join(mut self) {
                    if let Some(handle) = self.handle.take() {
                        let joined = handle.join();
                        assert!(joined.is_ok());
                    }
                }
            }
            impl Drop for WorkerOwner {
                fn drop(&mut self) {
                    if let Some(handle) = self.handle.take() {
                        let joined = handle.join();
                        assert!(joined.is_ok());
                    }
                }
            }
        ",
        r"
            struct ScopedWorker<'scope> {
                handle: Option<std::thread::ScopedJoinHandle<'scope, ()>>,
            }
            impl<'scope> ScopedWorker<'scope> {
                fn launch(scope: &'scope std::thread::Scope<'scope, '_>) -> Self {
                    let handle = scope.spawn(|| {});
                    Self { handle: Some(handle) }
                }
                fn join(mut self) {
                    if let Some(handle) = self.handle.take() {
                        let joined = handle.join();
                        assert!(joined.is_ok());
                    }
                }
            }
        ",
        r"
            struct TupleWorker(Option<std::thread::JoinHandle<()>>);
            impl TupleWorker {
                fn launch() -> Self {
                    Self(Some(std::thread::spawn(|| {})))
                }
                fn join(mut self) {
                    if let Some(handle) = self.0.take() {
                        let joined = handle.join();
                        assert!(joined.is_ok());
                    }
                }
            }
            impl Drop for TupleWorker {
                fn drop(&mut self) {
                    if let Some(handle) = self.0.take() {
                        let joined = handle.join();
                        assert!(joined.is_ok());
                    }
                }
            }
        ",
    ] {
        let owned = scan_source("crates/another/src/owner.rs", source)
            .expect("the owned construction boundary parses");
        assert!(
            !owned
                .iter()
                .any(|finding| finding.kind == Kind::UnownedSpawn),
            "a structurally proved owner may construct the one resource it must clean up: {source}: {owned:?}"
        );
    }

    for source in [
        "struct SupervisedChild; impl SupervisedChild { fn launch(command: &mut std::process::Command) { command.spawn(); } }",
        r"
            struct Owner { child: std::process::Child }
            impl Owner {
                fn launch(command: &mut std::process::Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child })
                }
                fn wait(&mut self) -> std::io::Result<()> { self.child.wait().map(|status| drop(status)) }
            }
        ",
        r"
            struct Owner { child: std::process::Child }
            impl Owner {
                pub(crate) fn launch(command: &mut std::process::Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child })
                }
                fn wait(&mut self) -> std::io::Result<()> { self.child.wait().map(|status| drop(status)) }
            }
            impl Drop for Owner { fn drop(&mut self) { assert!(self.wait().is_ok()); } }
        ",
        r"
            struct Owner { handle: Option<std::thread::JoinHandle<()>> }
            impl Owner {
                fn launch() -> std::io::Result<Self> {
                    std::thread::Builder::new().spawn(|| {});
                    Ok(Self { handle: None })
                }
                fn join(mut self) { if let Some(handle) = self.handle.take() { assert!(handle.join().is_ok()); } }
            }
            impl Drop for Owner {
                fn drop(&mut self) { if let Some(handle) = self.handle.take() { assert!(handle.join().is_ok()); } }
            }
        ",
        r"
            struct TupleOwner(Option<std::thread::JoinHandle<()>>);
            impl TupleOwner {
                fn launch() -> Self { Self(Some(std::thread::spawn(|| {}))) }
                fn join(mut self) {
                    if let Some(handle) = self.0.take() {
                        let joined = handle.join();
                        assert!(joined.is_ok());
                    }
                }
            }
        ",
        r"
            struct Owner { handle: Option<std::thread::JoinHandle<()>> }
            impl Owner {
                fn launch() -> std::io::Result<Self> {
                    let first = std::thread::Builder::new().spawn(|| {})?;
                    std::thread::spawn(|| {});
                    Ok(Self { handle: Some(first) })
                }
                fn join(mut self) { if let Some(handle) = self.handle.take() { assert!(handle.join().is_ok()); } }
            }
            impl Drop for Owner {
                fn drop(&mut self) { if let Some(handle) = self.handle.take() { assert!(handle.join().is_ok()); } }
            }
        ",
        r"
            use std::process::{Child, Command};
            struct Owner { child: Option<Child> }
            impl Owner {
                fn launch(command: &mut Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child: Some(child) })
                }
            }
            impl Drop for Owner {
                fn drop(&mut self) {
                    if self.child.is_some() { ownership_failure(); }
                }
            }
            fn ownership_failure() -> ! { std::process::abort(); }
        ",
        r"
            use std::process::{Child, Command};
            struct Owner { child: Child }
            impl Owner {
                fn launch(command: &mut Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child })
                }
                fn replace(&mut self, replacement: &mut Child) {
                    std::mem::swap(&mut self.child, replacement);
                }
            }
            impl Drop for Owner {
                fn drop(&mut self) {
                    match self.child.try_wait() {
                        Ok(Some(_status)) => {}
                        Ok(None) | Err(_source) => ownership_failure(),
                    }
                }
            }
            fn ownership_failure() -> ! { std::process::abort(); }
        ",
        r"
            use std::process::{Child, Command};
            struct Owner { child: Child }
            impl Owner {
                fn launch(command: &mut Command) -> std::io::Result<Self> {
                    command.spawn().map(|child| Self { child })
                }
            }
            impl Drop for Owner {
                fn drop(&mut self) {
                    match self.child.try_wait() {
                        Ok(Some(_status)) => {}
                        Ok(None) | Err(_source) => ownership_failure(),
                    }
                }
            }
            fn ownership_failure() -> ! { loop {} }
        ",
    ] {
        assert!(
            kinds(source).contains(&Kind::UnownedSpawn),
            "a name, partial owner, public escape, detached result, or second spawn is not ownership proof: {source}"
        );
    }
}

#[test]
fn channels_must_have_a_finite_capacity() {
    for source in [
        "fn pair<T>() { std::sync::mpsc::channel::<T>(); }",
        "use std::sync::mpsc::channel; fn pair<T>() { channel::<T>(); }",
        "use std::sync::mpsc::channel as unlimited; fn pair<T>() { unlimited::<T>(); }",
        "macro_rules! pair { () => { std::sync::mpsc::channel::<u8>() } }",
        "macro_rules! call { ($function:path) => { $function::<u8>() } } call!(std::sync::mpsc::channel);",
    ] {
        assert!(
            kinds(source).contains(&Kind::UnboundedChannel),
            "an unbounded queue must not hide stalled-consumer memory growth: {source}"
        );
    }
    assert!(
        !kinds("fn pair<T>() { std::sync::mpsc::sync_channel::<T>(1); }")
            .contains(&Kind::UnboundedChannel),
        "a finite channel makes the capacity policy explicit"
    );
}

#[test]
fn three_way_state_is_a_closed_enum_instead_of_option_bool() {
    for source in [
        "struct Decision { accepted: Option<bool> }",
        "type Flag = bool; struct Decision { accepted: Option<Flag> }",
        "type Maybe<T> = Option<T>; struct Decision { accepted: Maybe<bool> }",
        "use std::option::Option as Maybe; struct Decision { accepted: Maybe<bool> }",
        "macro_rules! field { () => { accepted: Option<bool> } }",
        "macro_rules! field { ($held:ty) => { accepted: Option<$held> } }",
        "macro_rules! field { ($maybe:ident) => { accepted: $maybe<bool> } }",
    ] {
        assert!(
            kinds(source).contains(&Kind::TriStateBool),
            "the three states need names the compiler can keep exhaustive: {source}"
        );
    }
    assert!(
        !kinds(
            "enum Decision { Unrecorded, Rejected, Accepted } struct Record { decision: Decision, retry: Option<u8> }"
        )
        .contains(&Kind::TriStateBool),
        "a closed domain enum names all states while ordinary optional data remains optional"
    );
}

#[test]
fn poisoned_invariants_cannot_be_reclassified_as_valid_values() {
    for source in [
        "fn recover<T>(poisoned: std::sync::PoisonError<T>) -> T { poisoned.into_inner() }",
        "fn recover<T>(poisoned: std::sync::PoisonError<T>) -> T { std::sync::PoisonError::into_inner(poisoned) }",
        "fn recover<T>(value: Result<T, std::sync::PoisonError<T>>) -> T { value.unwrap_or_else(std::sync::PoisonError::into_inner) }",
        "fn recover<T>(lock: &std::sync::Mutex<T>) { lock.clear_poison(); }",
        "use std::sync::PoisonError::into_inner as recover;",
        "macro_rules! recover { ($lock:expr) => { $lock.clear_poison() } }",
        "macro_rules! recover { ($poison:expr) => { $poison.into_inner() } }",
    ] {
        assert!(
            kinds(source).contains(&Kind::PoisonRecovery),
            "a panic-interrupted invariant must remain a typed failure: {source}"
        );
    }
    assert!(
        !kinds(
            "fn read<T>(lock: &std::sync::Mutex<T>) -> Result<(), std::sync::PoisonError<std::sync::MutexGuard<'_, T>>> { let guard = lock.lock()?; drop(guard); Ok(()) }"
        )
        .contains(&Kind::PoisonRecovery),
        "propagating poison preserves the failed state"
    );
}

#[test]
fn counters_cannot_wrap_back_to_plausible_earlier_values() {
    for source in [
        "fn next(counter: &AtomicU64) -> u64 { counter.fetch_add(1, Ordering::Relaxed) }",
        "fn next(counter: &AtomicU64) -> u64 { AtomicU64::fetch_sub(counter, 1, Ordering::Relaxed) }",
        "fn next(counter: u64) -> u64 { counter.wrapping_add(1) }",
        "fn next(counter: u64) -> u64 { counter.wrapping_div(2) }",
        "fn next(counter: u64) -> u64 { counter.wrapping_rem(2) }",
        "fn next(counter: u64) -> u64 { counter.wrapping_pow(2) }",
        "fn next(counter: u64) -> u64 { counter.wrapping_shl(2) }",
        "fn next(counter: u64) -> u64 { counter.wrapping_shr(2) }",
        "fn next(counter: i64) -> i64 { counter.wrapping_neg() }",
        "use u64::wrapping_sub as earlier; fn previous(counter: u64) -> u64 { earlier(counter, 1) }",
        "macro_rules! next { ($counter:expr) => { $counter.fetch_add(1, Ordering::Relaxed) } }",
    ] {
        let found = scan_source("crates/demo/src/counter.rs", source).expect("it parses");
        assert!(
            found
                .iter()
                .any(|finding| finding.kind == Kind::WrappingCounter),
            "counter exhaustion must stay visible: {source}: {found:?}"
        );
    }
    assert!(
        scan_source(
            "crates/demo/src/counter.rs",
            "fn next(counter: &AtomicU64) -> Result<u64, Exhausted> { counter.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |value| value.checked_add(1)).map_err(|_| Exhausted) }",
        )
        .expect("it parses")
        .is_empty(),
        "fetch_update makes the checked transition the atomic operation"
    );
}

#[test]
fn direct_json_inputs_cannot_make_duplicate_keys_last_win() {
    for source in [
        "//! A file.\nfn read(s: &str) { let _ = serde_json::from_str::<serde_json::Value>(s); }\n",
        "//! A file.\nfn read(b: &[u8]) { let _ = serde_json::from_slice::<serde_json::Value>(b); }\n",
        "//! A file.\nfn read(r: impl std::io::Read) { let _ = serde_json::from_reader::<_, serde_json::Value>(r); }\n",
        "//! A file.\nfn read(s: &str) { let _ = serde_json::Deserializer::from_str(s); }\n",
        "//! A file.\nfn read(s: &str) { let _ = <serde_json::Deserializer<'_>>::from_str(s); }\n",
        "//! A file.\nfn read(s: &str) { let _ = serde_json::Value::from_str(s); }\n",
        "//! A file.\nfn read(s: &str) { let _ = <serde_json::Value as std::str::FromStr>::from_str(s); }\n",
        "//! A file.\nfn read(s: &str) { let _ = s.parse::<serde_json::Value>(); }\n",
        "//! A file.\nfn read(s: &str) { let _: u32 = s.parse().unwrap_or(0); }\n",
        "//! A file.\nuse serde_json::Value;\nfn read(s: &str) { let _ = Value::from_str(s); }\n",
        "//! A file.\npub use serde_json::Value;\n",
        "//! A file.\nuse serde_json::Value as Document;\n",
        "//! A file.\ntype Document = serde_json::Value;\nfn read(s: &str) { let _ = <Document as std::str::FromStr>::from_str(s); }\n",
        "//! A file.\nuse serde_json::from_str as decode;\n",
        "//! A file.\nuse serde_json as json;\n",
        "//! A file.\ntype Parser<'a> = serde_json::Deserializer<serde_json::de::StrRead<'a>>;\n",
        "//! A file.\nmacro_rules! read { ($input:expr) => { serde_json::from_str($input) } }\n",
        "//! A file.\nmacro_rules! read { ($method:ident) => { serde_json::$method(input) } }\nread!(from_str);\n",
        "//! A file.\nmacro_rules! read { ($ty:ty) => { input.parse::<$ty>() } }\nread!(serde_json::Value);\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::DirectJsonInput),
            "the direct or hidden reader must be refused: {source}"
        );
    }
}

#[test]
fn strict_json_and_value_conversion_are_the_only_input_boundary() {
    assert_eq!(
        kinds(
            "//! A file.\nfn read(s: &str) { let value = crate::strictjson::decode_str::<serde_json::Value>(s); let document = value.and_then(serde_json::from_value::<Document>); inspect(document); }\nstruct Document;\n"
        ),
        []
    );
    assert_eq!(
        scan_source(
            "crates/rust-mutants/src/strictjson.rs",
            "//! The unique-key reader.\nfn read(s: &str) { let parser = serde_json::Deserializer::from_str(s); inspect(parser); }\n",
        )
        .expect("the reader source parses"),
        [],
        "only the exact implementation module owns the direct parser capability"
    );
    assert_eq!(
        kinds(
            "//! A file.\nfn write<T: serde::Serialize>(value: &T) { let written = serde_json::to_string(value); inspect(written); }\n"
        ),
        [],
        "serialization cannot lose an input key"
    );
    assert_eq!(
        kinds(
            "//! A file.\nfn number(s: &str) { let number = s.parse::<u32>(); inspect(number); }\n"
        ),
        [],
        "an explicit non-JSON parser keeps its concrete type visible"
    );
}

#[test]
fn an_item_allow_is_refused() {
    assert_eq!(
        kinds("//! A file.\n\n#[allow(dead_code)]\npub fn f() {}\n"),
        [Kind::AllowAttribute]
    );
}

#[test]
fn a_crate_level_allow_is_refused_too() {
    assert_eq!(
        kinds("//! A file.\n#![allow(clippy::pedantic)]\n"),
        [Kind::AllowAttribute]
    );
}

#[test]
fn cfg_attr_cannot_hide_an_allow_at_any_nesting_or_expansion_site() {
    for source in [
        "//! A file.\n#[cfg_attr(test, allow(unsafe_code, reason = \"forever\"))]\nfn f() {}\n",
        "//! A file.\n#![cfg_attr(test, allow(dead_code, reason = \"forever\"))]\n",
        "//! A file.\n#[cfg_attr(test, cfg_attr(test, allow(dead_code, reason = \"forever\")))]\nfn f() {}\n",
        "//! A file.\nmacro_rules! hidden { () => { #[cfg_attr(test, allow(dead_code, reason = \"forever\"))] fn f() {} } }\nhidden!();\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::AllowAttribute),
            "a conditional allow must remain a refusal: {source}"
        );
    }
}

#[test]
fn source_redirects_are_literal_or_explicitly_opaque() {
    assert_eq!(
        source_redirects("include!(\"support/ok.rs\");\n").expect("literal include parses"),
        [SourceRedirect::Include {
            target: Some("support/ok.rs".to_owned()),
            line: 1,
        }]
    );
    assert_eq!(
        source_redirects("std::include!(\"support/ok.rs\");\n").expect("qualified include parses"),
        [SourceRedirect::Include {
            target: Some("support/ok.rs".to_owned()),
            line: 1,
        }]
    );
    assert_eq!(
        source_redirects("include!(concat!(env!(\"OUT_DIR\"), \"/hidden.rs\"));\n")
            .expect("opaque include parses"),
        [SourceRedirect::Include {
            target: None,
            line: 1,
        }]
    );
    assert_eq!(
        source_redirects("#[cfg_attr(test, cfg_attr(test, path = \"outside.rs\"))]\nmod hidden;\n")
            .expect("nested path parses"),
        [SourceRedirect::Path {
            target: Some("outside.rs".to_owned()),
            line: 1,
        }]
    );
    for source in [
        "macro_rules! hidden { () => { include!(\"hidden.rs\"); } }\n",
        "macro_rules! hidden { () => { #[path = \"hidden.rs\"] mod generated; } }\n",
        "use core::include as load;\nload!(\"hidden.rs\");\n",
        "macro_rules! hidden { ($loader:path) => { $loader!(\"hidden.rs\") } } hidden!(include);\n",
    ] {
        assert_eq!(
            source_redirects(source).expect("macro source parses"),
            [SourceRedirect::Opaque { line: 1 }],
            "macro-generated source is outside a direct AST walk: {source}"
        );
    }
}

#[test]
fn vacuous_cfg_cannot_hide_code_or_pretend_to_condition_it() {
    for source in [
        "#[cfg(any())] fn dormant() {}\n",
        "#[cfg(not(all()))] fn dormant() {}\n",
        "#[cfg(all())] fn unconditional() {}\n",
        "#[cfg(not(any()))] fn unconditional() {}\n",
        "#[cfg_attr(test, cfg(any()))] fn nested() {}\n",
        "macro_rules! hidden { () => { #[cfg(any())] fn dormant() {} } }\n",
        "fn active() -> bool { cfg!(all()) }\n",
        "macro_rules! hidden { () => { cfg!(not(any())) } }\n",
        "fn active() -> bool { core::cfg!(all()) }\n",
        "use core::cfg as condition;\nconst ACTIVE: bool = condition!(all());\n",
        "macro_rules! condition { ($macro:path) => { $macro!(all()) } } condition!(cfg);\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::VacuousCfg),
            "a constant cfg condition escaped the source gate: {source}"
        );
    }
    for source in [
        "#[cfg(unix)] fn platform() {}\n",
        "#[cfg_attr(test, derive(Debug))] struct Conditional;\n",
        "fn platform() -> bool { cfg!(windows) }\n",
    ] {
        assert!(
            !kinds(source).contains(&Kind::VacuousCfg),
            "a real cfg boundary was mistaken for a constant: {source}"
        );
    }
}

#[test]
fn procedural_macro_exports_and_opaque_token_construction_are_structural() {
    let source = r"
use proc_macro::TokenStream;
#[proc_macro_derive(AllVariants)]
pub fn all_variants(input: TokenStream) -> TokenStream { input }
#[proc_macro_attribute]
pub fn integration(_args: TokenStream, input: TokenStream) -> TokenStream { input }
#[proc_macro_attribute]
pub fn unit(_args: TokenStream, input: TokenStream) -> TokenStream { input }
";
    let exports = proc_macro_exports(source).expect("proc-macro source parses");
    assert_eq!(
        exports
            .iter()
            .map(|export| (export.kind, export.name.as_str()))
            .collect::<Vec<_>>(),
        [
            ("attribute", "integration"),
            ("attribute", "unit"),
            ("derive", "AllVariants"),
        ]
    );
    let conditional = proc_macro_exports(
        "#[cfg_attr(feature = \"hidden\", proc_macro_attribute)]\n\
         pub fn hidden(_args: TokenStream, input: TokenStream) -> TokenStream { input }\n",
    )
    .expect("conditional proc-macro source parses");
    assert_eq!(
        conditional
            .iter()
            .map(|export| (export.kind, export.name.as_str()))
            .collect::<Vec<_>>(),
        [("conditional", "hidden")],
        "a cfg_attr cannot hide an additional exported procedural macro"
    );
    for hostile in [
        "fn emit() { let _: proc_macro::TokenStream = \"Box<dyn Debug>\".parse().unwrap(); }",
        "fn emit() { let _ = proc_macro2::Ident::new(\"Box\", proc_macro2::Span::call_site()); }",
        "fn emit() { let _ = quote::format_ident!(\"Box\"); }",
        "use proc_macro2::Ident as I; fn emit() { let _ = I::new(\"Box\", proc_macro2::Span::call_site()); }",
        "use quote::format_ident as make; fn emit() { let _ = make!(\"Box\"); }",
        "type I = proc_macro2::Ident; fn emit() { let _ = I::new(\"Box\", proc_macro2::Span::call_site()); }",
        "fn emit() { let make = proc_macro2::Ident::new; let _ = make(\"Box\", proc_macro2::Span::call_site()); }",
        "macro_rules! make { ($constructor:path) => { $constructor::new(\"Box\", proc_macro2::Span::call_site()) } } fn emit() { let _ = make!(proc_macro2::Ident); }",
        "macro_rules! export { () => { #[proc_macro_attribute] pub fn hidden(args: TokenStream, input: TokenStream) -> TokenStream { input } } }",
        "fn emit() { let owner = quote::quote!(Box); let held = quote::quote!(dyn Debug); let _ = quote::quote!(#owner<#held>); }",
    ] {
        let findings = opaque_proc_macro_synthesis("crates/macros/src/lib.rs", hostile)
            .expect("hostile proc-macro source parses");
        assert!(
            findings
                .iter()
                .any(|finding| finding.kind == Kind::OpaqueMacroSyntax),
            "opaque token construction passed: {hostile}"
        );
    }
    let closed = r"
fn emit(ident: syn::Ident, count: usize, variants: Vec<syn::Ident>) {
    let _ = quote::quote! {
        impl #ident {
            /// Every variant, in declaration order.
            pub const ALL: [Self; #count] = [#(Self::#variants),*];
        }
    };
}
";
    assert_eq!(
        opaque_proc_macro_synthesis("crates/macros/src/lib.rs", closed)
            .expect("closed proc-macro template parses"),
        [],
        "the exact inventoried AllVariants template remains visible and usable"
    );
}

#[test]
fn an_expect_is_the_waiver_this_repository_writes() {
    assert_eq!(
        kinds("//! A file.\n\n#[expect(dead_code, reason = \"soon\")]\npub fn f() {}\n"),
        [],
        "an expectation the compiler retires when the lint stops firing"
    );
}

#[test]
fn an_expectation_cannot_waive_future_dead_or_unsafe_code_for_a_module() {
    for source in [
        "//! A file.\n#![expect(dead_code, reason = \"one current item\")]\nfn unused() {}\n",
        "//! A file.\n#[cfg_attr(test, expect(unsafe_code, reason = \"one current block\"))]\nmod ffi {}\n",
    ] {
        assert_eq!(kinds(source), [Kind::BroadExpectation], "{source}");
    }
    assert_eq!(
        kinds(
            "//! A file.\nfn ffi() { #[expect(unsafe_code, reason = \"one FFI call\")] unsafe { std::ptr::read_volatile(&0); } }\n"
        ),
        [],
        "the exact unsafe expression remains a compiler-checked exception"
    );
}

#[test]
fn a_boxed_trait_object_is_refused_wherever_it_appears() {
    for source in [
        "//! A file.\npub struct S {\n    f: Box<dyn std::fmt::Debug>,\n}\n",
        "//! A file.\npub fn f() -> Box<dyn Fn(i32) -> i32> {\n    unimplemented!()\n}\n",
        "//! A file.\npub fn f(v: Vec<Box<dyn std::error::Error>>) {\n    drop(v);\n}\n",
        "//! A file.\npub fn f(v: std::sync::Arc<dyn Send>) {\n    drop(v);\n}\n",
        "//! A file.\npub fn f(v: std::rc::Rc<dyn Send>) {\n    drop(v);\n}\n",
        "//! A file.\npub fn f(v: std::pin::Pin<Box<dyn Send>>) {\n    drop(v);\n}\n",
    ] {
        assert_eq!(kinds(source), [Kind::OwnedTraitObject], "{source}");
    }
}

#[test]
fn macro_tokens_cannot_hide_an_owned_trait_object_from_the_syntax_walk() {
    assert_eq!(
        kinds(
            "//! A file.\nmacro_rules! erased { () => { type Hidden = Box<dyn std::fmt::Debug>; } }\n"
        ),
        [Kind::OwnedTraitObject, Kind::OwnedPointerAlias]
    );
    assert_eq!(
        kinds("//! A file.\nerased!(std::sync::Arc<dyn Send>);\n"),
        [Kind::OwnedTraitObject]
    );
    assert_eq!(
        kinds(
            "//! A file.\nuse std::boxed::Box as Heap;\nmacro_rules! erased { () => { type Hidden = Heap<dyn std::fmt::Debug>; } }\n"
        ),
        [
            Kind::OwnedTraitObject,
            Kind::OwnedPointerAlias,
            Kind::OwnedPointerAlias
        ]
    );
    assert_eq!(
        kinds("//! A file.\nprintln!(\"Box<dyn Trait> is an example\");\n"),
        [],
        "a literal inside a macro is data, not a type the expansion owns"
    );
}

#[test]
fn a_macro_cannot_generate_a_pointer_alias_for_a_later_trait_object() {
    assert_eq!(
        kinds(
            "//! A file.\nmacro_rules! pointer { ($name:ident) => { type $name<T> = Box<T>; } }\npointer!(Heap);\npub struct S { field: Heap<dyn std::fmt::Debug> }\n"
        ),
        [Kind::OwnedTraitObject, Kind::OwnedPointerAlias],
        "both the opaque constructor declaration and the later erased argument are refused"
    );
}

#[test]
fn a_macro_cannot_split_the_pointer_from_its_trait_object_argument() {
    for pointer in ["Box", "std::rc::Rc", "std::sync::Arc"] {
        let source = format!(
            "//! A file.\nmacro_rules! own {{ ($t:ty) => {{ struct S({pointer}<$t>); }} }}\nown!(dyn std::fmt::Debug);\n"
        );
        assert_eq!(
            kinds(&source),
            [Kind::OwnedTraitObject],
            "the macro body owns every type accepted at the separate invocation: {source}"
        );
    }
}

#[test]
fn a_macro_cannot_supply_the_pointer_and_trait_object_as_separate_arguments() {
    let source = "//! A file.\nmacro_rules! own { ($pointer:ident, $held:ty) => { struct S($pointer<$held>); } }\nown!(Box, dyn std::fmt::Debug);\n";
    assert_eq!(kinds(source), [Kind::OwnedTraitObject]);
}

#[test]
fn a_generic_nominal_wrapper_cannot_hide_an_owned_trait_object() {
    let source =
        "//! A file.\nstruct Heap<T: ?Sized>(Box<T>);\nstruct S(Heap<dyn std::fmt::Debug>);\n";
    assert_eq!(
        kinds(source),
        [Kind::OwnedTraitObject, Kind::OwnedTraitObject]
    );
}

#[test]
fn an_associated_type_cannot_hide_either_half_of_owned_erasure() {
    for (source, expected) in [
        (
            "//! A file.\ntrait Owner<T: ?Sized> { type Out: ?Sized; }\nstruct Factory;\nimpl<T: ?Sized> Owner<T> for Factory { type Out = Box<T>; }\ntype Heap<T> = <Factory as Owner<T>>::Out;\nstruct S(Heap<dyn std::fmt::Debug>);\n",
            Kind::OwnedPointerAlias,
        ),
        (
            "//! A file.\ntrait Erase { type Out: ?Sized; }\nimpl Erase for () { type Out = dyn std::fmt::Debug; }\ntype Hidden = <() as Erase>::Out;\nstruct S(Box<Hidden>);\n",
            Kind::TraitObjectAlias,
        ),
    ] {
        assert!(kinds(source).contains(&expected), "{source}");
    }
}

#[test]
fn macro_expansion_cannot_hide_allow_or_derived_enum_default() {
    assert_eq!(
        kinds(
            "//! A file.\nmacro_rules! hidden { () => { #[allow(dead_code, reason = \"forever\")] const fn unused() {} } }\nhidden!();\n"
        ),
        [Kind::AllowAttribute]
    );
    assert_eq!(
        kinds(
            "//! A file.\nmacro_rules! state { () => { #[derive(Default)] enum State { #[default] Waiting, Done } } }\nstate!();\n"
        ),
        [Kind::DerivedEnumDefault]
    );
}

#[test]
fn aliases_cannot_hide_an_owned_trait_object() {
    for (source, expected) in [
        (
            "//! A file.\ntype Erased = dyn std::fmt::Debug;\npub struct S { f: Box<Erased> }\n",
            vec![Kind::OwnedTraitObject, Kind::TraitObjectAlias],
        ),
        (
            "//! A file.\nuse std::boxed::Box as Heap;\ntype Erased = dyn std::fmt::Debug;\npub struct S { f: Heap<Erased> }\n",
            vec![
                Kind::OwnedTraitObject,
                Kind::TraitObjectAlias,
                Kind::OwnedPointerAlias,
            ],
        ),
        (
            "//! A file.\ntype Heap<T> = Box<T>;\ntype Erased = dyn std::fmt::Debug;\npub struct S { f: Heap<Erased> }\n",
            vec![
                Kind::OwnedTraitObject,
                Kind::TraitObjectAlias,
                Kind::OwnedPointerAlias,
            ],
        ),
        (
            "//! A file.\ntype Heap<T> = Option<Box<T>>;\npub struct S { f: Heap<dyn std::fmt::Debug> }\n",
            vec![Kind::OwnedTraitObject, Kind::OwnedPointerAlias],
        ),
        (
            "//! A file.\ntype Both<T, E> = Box<Result<T, E>>;\n",
            vec![Kind::OwnedPointerAlias, Kind::ResultAlias],
        ),
    ] {
        assert_eq!(kinds(source), expected, "{source}");
    }
    assert_eq!(
        kinds("//! A file.\npub struct S<'a> { f: &'a dyn std::fmt::Debug }\n"),
        [],
        "the policy is about erasing the owned implementation set, not borrowing one"
    );
}

#[test]
fn a_box_of_something_concrete_is_not_a_finding() {
    assert_eq!(
        kinds("//! A file.\npub struct S {\n    f: Box<str>,\n    g: Box<[u8]>,\n}\n"),
        [],
        "the cost this gate is about is the vtable, not the allocation"
    );
}

#[test]
fn an_unknown_generic_constructor_cannot_hide_ownership_of_a_trait_object() {
    for source in [
        "//! A file.\nstruct S(ForeignOwner<dyn std::fmt::Debug>);\n",
        "//! A file.\nstruct S(<Factory as Owner>::Held<dyn std::fmt::Debug>);\n",
        "//! A file.\nstruct S(std::marker::PhantomData<dyn std::fmt::Debug>);\n",
        "//! A file.\nstruct S(&'static ForeignOwner<dyn std::fmt::Debug>);\n",
    ] {
        assert!(kinds(source).contains(&Kind::OwnedTraitObject), "{source}");
    }
    for source in [
        "//! A file.\nstruct S<'a>(&'a dyn std::fmt::Debug);\n",
        "//! A file.\nstruct S<'a>(Option<&'a dyn std::fmt::Debug>);\n",
    ] {
        assert!(
            !kinds(source).contains(&Kind::OwnedTraitObject),
            "a direct borrow of the vtable remains the one explicit exception: {source}"
        );
    }
}

#[test]
fn an_enum_default_must_be_an_explicit_implementation() {
    let derived = "//! A file.\n#[derive(Default)]\npub enum State { #[default] Waiting, Done }\n";
    assert_eq!(kinds(derived), [Kind::DerivedEnumDefault]);
    let explicit = "//! A file.\npub enum State { Waiting, Done }\nimpl Default for State { fn default() -> Self { Self::Waiting } }\n";
    assert_eq!(kinds(explicit), [Kind::SemanticDefault]);
    let neutral_struct = "//! A file.\n#[derive(Default)]\npub struct Counts { passed: usize }\n";
    assert_eq!(kinds(neutral_struct), []);
}

#[test]
fn qualified_aliased_and_conditional_default_derives_are_still_derives() {
    for (source, expected) in [
        (
            "//! A file.\n#[derive(core::default::Default)]\npub enum State { #[default] Waiting, Done }\n",
            vec![Kind::DerivedEnumDefault],
        ),
        (
            "//! A file.\nuse std::default::Default as Initial;\n#[derive(Initial)]\npub enum State { #[default] Waiting, Done }\n",
            vec![Kind::DerivedEnumDefault, Kind::DefaultDeriveAlias],
        ),
        (
            "//! A file.\n#[cfg_attr(test, derive(Default))]\npub enum State { #[cfg_attr(test, default)] Waiting, Done }\n",
            vec![Kind::DerivedEnumDefault],
        ),
    ] {
        assert_eq!(kinds(source), expected, "{source}");
    }
}

#[test]
fn unit_cannot_invent_a_domain_value_through_from() {
    for source in [
        "//! A file.\nstruct Domain;\nimpl From<()> for Domain { fn from(_: ()) -> Self { Self } }\n",
        "//! A file.\nstruct Domain;\nimpl std::convert::From<()> for Domain { fn from(_: ()) -> Self { Self } }\n",
        "//! A file.\nuse std::convert::From as Convert;\nstruct Domain;\nimpl Convert<()> for Domain { fn from(_: ()) -> Self { Self } }\n",
    ] {
        assert_eq!(kinds(source), [Kind::UnitDomainConversion], "{source}");
    }
    assert_eq!(
        kinds(
            "//! A file.\ntype Empty = ();\nstruct Domain;\nimpl From<Empty> for Domain { fn from(_: Empty) -> Self { Self } }\n"
        ),
        [Kind::UnitDomainConversion, Kind::UnitDomainConversion]
    );
    assert_eq!(
        kinds(
            "//! A file.\ntype Empty = ();\ntype Nothing = Empty;\nstruct Domain;\nimpl From<Nothing> for Domain { fn from(_: Nothing) -> Self { Self } }\n"
        ),
        [
            Kind::UnitDomainConversion,
            Kind::UnitDomainConversion,
            Kind::UnitDomainConversion
        ]
    );
}

#[test]
fn macro_tokens_cannot_split_a_unit_conversion_across_expansion() {
    for source in [
        "//! A file.\nmacro_rules! conversion { () => { impl From<()> for Domain { fn from(_: ()) -> Self { Domain } } } }\n",
        "//! A file.\nmacro_rules! conversion { ($t:ty) => { impl From<$t> for Domain { fn from(_: $t) -> Self { Domain } } } }\nconversion!(());\n",
        "//! A file.\nmacro_rules! conversion { ($trait_:path) => { impl $trait_<()> for Domain { fn from(_: ()) -> Self { Domain } } } }\nconversion!(From);\n",
        "//! A file.\nconvert!(From, ());\n",
        "//! A file.\nuse std::convert::From as Convert;\nmacro_rules! conversion { ($t:ty) => { impl Convert<$t> for Domain { fn from(_: $t) -> Self { Domain } } } }\n",
        "//! A file.\nmacro_rules! alias { ($name:ident, $t:ty) => { type $name = $t; } }\nalias!(Empty, ());\n",
    ] {
        assert_eq!(kinds(source), [Kind::UnitDomainConversion], "{source}");
    }
}

#[test]
fn named_domain_defaults_and_non_unit_conversions_remain_explicit() {
    let source = "//! A file.\nstruct Domain;\nimpl Domain { const EMPTY: Self = Self; }\nimpl Default for Domain { fn default() -> Self { Self::EMPTY } }\nimpl From<u8> for Domain { fn from(_: u8) -> Self { Self::EMPTY } }\nfn observes(_: ()) {}\n";
    assert_eq!(kinds(source), [Kind::SemanticDefault]);
}

#[test]
fn only_exact_configuration_and_ui_defaults_are_allowlisted() {
    let contract = "//! A file.\npub enum Contract { Standard, Deep }\nimpl Default for Contract { fn default() -> Self { Self::Standard } }\n";
    assert_eq!(
        scan_source("crates/njutest/src/config.rs", contract)
            .expect("the source parses")
            .into_iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>(),
        []
    );
    assert_eq!(
        scan_source("crates/njutest/src/report/mod.rs", contract)
            .expect("the source parses")
            .into_iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>(),
        [Kind::SemanticDefault],
        "the same spelling in a domain module does not inherit configuration authority"
    );
    assert_eq!(
        kinds(
            "//! A file.\nmacro_rules! state { () => { impl Default for State { fn default() -> Self { State::Waiting } } } }\n"
        ),
        [Kind::SemanticDefault]
    );
}

#[test]
fn only_an_explicit_prelude_may_be_imported_as_a_glob() {
    assert_eq!(
        kinds("//! A file.\nuse crate::model::*;\n"),
        [Kind::GlobImport]
    );
    assert_eq!(
        kinds("//! A file.\nuse proptest::prelude::*;\n"),
        [],
        "a prelude declares that its vocabulary is intentionally imported as one set"
    );
    assert_eq!(
        kinds("//! A file.\nuse crate::{model::*, prelude::*};\n"),
        [Kind::GlobImport],
        "grouping and qualification do not hide which module supplies the glob"
    );
    assert_eq!(
        kinds("//! A file.\nuse crate::prelude::{self, *};\n"),
        [],
        "a grouped import from the explicitly named prelude is still a prelude"
    );
}

#[test]
fn an_error_is_not_flattened_into_words_before_the_output_boundary() {
    for source in [
        "//! A file.\nfn read() -> Result<(), String> { Ok(()) }\n",
        "//! A file.\nfn read() -> std::result::Result<(), &'static str> { Ok(()) }\n",
    ] {
        assert_eq!(kinds(source), [Kind::StringError], "{source}");
    }
    assert_eq!(
        kinds(
            "//! A file.\n#[derive(Debug)] struct ReadError;\nfn read() -> Result<(), ReadError> { Ok(()) }\n"
        ),
        []
    );
}

#[test]
fn standard_owners_do_not_make_text_a_typed_error() {
    for source in [
        "//! A file.\nfn read() -> Result<(), Box<str>> { unimplemented!() }\n",
        "//! A file.\nfn read() -> Result<(), std::sync::Arc<str>> { unimplemented!() }\n",
        "//! A file.\nfn read() -> Result<(), std::rc::Rc<str>> { unimplemented!() }\n",
        "//! A file.\nfn read() -> Result<(), std::borrow::Cow<'static, str>> { unimplemented!() }\n",
        "//! A file.\ntype Heap<T> = Box<T>;\nfn read() -> Result<(), Heap<str>> { unimplemented!() }\n",
        "//! A file.\nuse std::borrow::Cow as Text;\nfn read() -> Result<(), Text<'static, str>> { unimplemented!() }\n",
    ] {
        assert!(kinds(source).contains(&Kind::StringError), "{source}");
    }
}

#[test]
fn an_associated_projection_cannot_hide_owned_text_from_an_error() {
    let source = "//! A file.\ntrait Text { type Out; }\nimpl Text for () { type Out = Box<str>; }\nfn read() -> Result<(), <() as Text>::Out> { unimplemented!() }\n";
    assert!(kinds(source).contains(&Kind::StringAlias), "{source}");
}

#[test]
fn owned_non_text_data_is_not_a_string_error() {
    let source = "//! A file.\n#[derive(Debug)] struct ReadError;\nfn bytes() -> Result<(), Box<[u8]>> { unimplemented!() }\nfn typed() -> Result<(), ReadError> { Err(ReadError) }\n";
    assert!(!kinds(source).contains(&Kind::StringError), "{source}");
}

#[test]
fn aliases_cannot_hide_a_string_error() {
    for (source, expected) in [
        (
            "//! A file.\ntype Failure = String;\nfn read() -> Result<(), Failure> { Ok(()) }\n",
            vec![Kind::StringError, Kind::StringAlias],
        ),
        (
            "//! A file.\ntype Text = String;\ntype Failure = Text;\nfn read() -> Result<(), Failure> { Ok(()) }\n",
            vec![Kind::StringError, Kind::StringAlias, Kind::StringAlias],
        ),
        (
            "//! A file.\nuse std::result::Result as Fallible;\ntype Failure = String;\nfn read() -> Fallible<(), Failure> { Ok(()) }\n",
            vec![Kind::StringError, Kind::StringAlias, Kind::ResultAlias],
        ),
        (
            "//! A file.\ntype Fallible<T, E> = Result<T, E>;\ntype Failure = String;\nfn read() -> Fallible<(), Failure> { Ok(()) }\n",
            vec![Kind::StringError, Kind::StringAlias, Kind::ResultAlias],
        ),
        (
            "//! A file.\ntype Fallible<T, E> = Vec<Result<T, E>>;\ntype Failure = String;\nfn read() -> Fallible<(), Failure> { Ok(()) }\n",
            vec![Kind::StringError, Kind::StringAlias, Kind::ResultAlias],
        ),
        (
            "//! A file.\nuse std::string::String as Failure;\nfn read() -> Result<(), Failure> { Ok(()) }\n",
            vec![Kind::StringError, Kind::StringAlias],
        ),
    ] {
        assert_eq!(kinds(source), expected, "{source}");
    }
}

#[test]
fn unit_cannot_erase_an_error() {
    for source in [
        "//! A file.\nfn run() -> Result<u8, ()> { Err(()) }\n",
        "//! A file.\ntype Empty = ();\nfn run() -> Result<u8, Empty> { Err(()) }\n",
        "//! A file.\nuse std::result::Result as Fallible;\nfn run() -> Fallible<u8, ()> { Err(()) }\n",
        "//! A file.\nmacro_rules! run { () => { fn run() -> Result<u8, ()> { Err(()) } } }\n",
        "//! A file.\nmacro_rules! run { ($error:ty) => { fn run() -> Result<u8, $error> { unimplemented!() } } }\n",
    ] {
        assert!(kinds(source).contains(&Kind::UnitError), "{source}");
    }
    assert!(
        kinds(
            "//! A file.\ntrait ErrorType { type Error; }\nimpl ErrorType for Domain { type Error = (); }\nstruct Domain;\n"
        )
        .contains(&Kind::UnitDomainConversion),
        "an associated projection is closed at its declaration even when a different file uses it as an error"
    );
    assert_eq!(
        kinds(
            "//! A file.\n#[derive(Debug)] enum Failure { Refused }\nfn run() -> Result<u8, Failure> { Err(Failure::Refused) }\n"
        ),
        []
    );
}

#[test]
fn traversal_errors_are_not_turned_into_absent_entries() {
    for source in [
        "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { let _files = entries.filter_map(Result::ok); }\n",
        "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { let _files = entries.filter_map(std::result::Result::ok); }\n",
        "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { let _files = entries.filter_map(|entry| entry.ok()); }\n",
        "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { let _files = entries.map_while(Result::ok); }\n",
        "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { let _files = entries.flat_map(Result::into_iter); }\n",
        "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { for Ok(entry) in entries { drop(entry); } }\n",
    ] {
        assert!(kinds(source).contains(&Kind::DiscardedResult), "{source}");
    }
    assert!(
        kinds(
            "//! A file.\nfn values(entries: impl Iterator<Item = Option<u8>>) { let _values = entries.flatten(); }\n"
        )
        .contains(&Kind::DiscardedResult),
        "a syntax-only gate cannot prove that an unknown iterator carries no Result, so flatten is refused outright"
    );
    assert_eq!(
        kinds(
            "//! A file.\nfn values(entries: impl Iterator<Item = Option<u8>>) { let values = entries.filter_map(std::convert::identity); consume(values); }\n"
        ),
        [],
        "an explicit Option-only operation remains available"
    );
}

#[test]
fn result_ok_and_err_are_never_branch_policy() {
    for source in [
        "//! A file.\nfn f(result: Result<u8, Error>) { let _ = result.ok(); }\n",
        "//! A file.\nfn f(result: Result<u8, Error>) { let _ = result.err(); }\n",
        "//! A file.\nfn f(result: Result<u8, Error>) { let _ = Result::ok(result); }\n",
        "//! A file.\nfn f(result: Result<u8, Error>) { let _ = std::result::Result::err(result); }\n",
        "//! A file.\nmacro_rules! f { ($result:expr) => { $result.ok() } }\n",
        "//! A file.\nmacro_rules! f { ($result:expr) => { Result::err($result) } }\n",
    ] {
        assert!(kinds(source).contains(&Kind::DiscardedResult), "{source}");
    }
    assert_eq!(
        kinds(
            "//! A file.\nstruct Said { out: String, err: String }\nfn check(said: Said) { assert_eq!(said.out, said.err); }\n"
        ),
        [],
        "macro arguments are parsed as expressions: a field named `err` is not `Result::err()`"
    );
}

#[test]
fn a_result_fallback_must_use_the_error_that_justifies_it() {
    for source in [
        "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { result.unwrap_or_else(|_| 0) }\n",
        "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { result.unwrap_or_else(|_error| 0) }\n",
        "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { Result::unwrap_or_else(result, |_| 0) }\n",
        "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { core::result::Result::unwrap_or_else(result, |error| { let fallback = 0; fallback }) }\n",
        "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { result.map_or_else(|_| 0, |value| value) }\n",
        "//! A file.\nfn value(result: Result<u8, Error>) -> Result<u8, Error> { result.or_else(|_| Ok(0)) }\n",
        "//! A file.\nfn value(result: Result<u8, Error>) -> Result<u8, Error> { Result::or_else(result, |_| Ok(0)) }\n",
    ] {
        assert_eq!(kinds(source), [Kind::DiscardedResult], "{source}");
    }
    assert_eq!(
        kinds(
            "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { result.unwrap_or_else(|error| { record(error); 0 }) }\n"
        ),
        [],
        "a fallback that records the typed error states its policy"
    );
    assert_eq!(
        kinds(
            "//! A file.\nfn value(result: Result<u8, Error>) -> u8 { result.unwrap_or_else(|error| panic!(\"{error}\")) }\n"
        ),
        [],
        "using the typed error in a macro is still an explicit policy"
    );
    assert_eq!(
        kinds("//! A file.\nfn value(value: Option<u8>) -> u8 { value.unwrap_or_else(|| 0) }\n"),
        [],
        "Option has no error argument to erase"
    );
}

#[test]
fn a_result_is_not_turned_into_a_zero_or_one_item_iterator() {
    for source in [
        "//! A file.\nfn values(result: Result<u8, Error>) { for value in Result::into_iter(result) { use_value(value); } }\n",
        "//! A file.\nmacro_rules! values { ($result:expr) => { Result::into_iter($result) } }\n",
    ] {
        assert_eq!(kinds(source), [Kind::DiscardedResult], "{source}");
    }
}

#[test]
fn a_call_cannot_be_hidden_from_must_use_with_drop() {
    for source in [
        "//! A file.\nfn f() { drop(std::fs::write(\"out\", b\"bytes\")); }\n",
        "//! A file.\nfn f() { std::mem::drop(std::fs::write(\"out\", b\"bytes\")); }\n",
        "//! A file.\nuse std::mem::drop as discard;\nfn f() { discard(std::fs::write(\"out\", b\"bytes\")); }\n",
        "//! A file.\nfn f(sink: &mut impl std::io::Write) { drop(sink.flush()); }\n",
        "//! A file.\nmacro_rules! ignored { () => { drop(std::fs::write(\"out\", b\"bytes\")); } }\n",
        "//! A file.\nmacro_rules! ignored { ($discard:path, $call:path) => { $discard($call()); } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::DroppedComputation),
            "{source}"
        );
    }
    assert_eq!(
        kinds("//! A file.\nfn f(guard: Guard) { drop(guard); }\n"),
        [],
        "ending one named value's lifetime is the operation drop is for"
    );
}

#[test]
fn renamed_drop_is_rejected_where_it_is_declared() {
    for source in [
        "//! A file.\npub use std::mem::drop as discard;\n",
        "//! A file.\nmacro_rules! exports { () => { pub use std::mem::drop as discard; } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::DroppedComputation),
            "the exporting file must fail before another file can call discard(fallible()): {source}"
        );
    }
}

#[test]
fn an_underscore_binding_cannot_hide_a_computation() {
    for source in [
        "//! A file.\nfn f() { let _written = std::fs::write(\"out\", b\"bytes\"); }\n",
        "//! A file.\nfn f() { let _ = sink.send(message); }\n",
        "//! A file.\nfn f() { let (_sent, value) = send_and_answer(); use_value(value); }\n",
        "//! A file.\nmacro_rules! ignored { () => { let _written = std::fs::write(\"out\", b\"bytes\"); } }\n",
        "//! A file.\nmacro_rules! ignored { ($name:ident, $call:expr) => { let $name = $call; } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::IgnoredComputation),
            "the binding suppresses the compiler diagnostic: {source}"
        );
    }
    for source in [
        "//! A file.\nfn f() { let answer = fallible(); inspect(answer); }\n",
        "//! A file.\nfn f(guard: Guard) { let held_guard = guard; drop(held_guard); }\n",
        "//! A file.\nfn f() { let _annotation: Option<u8> = None; }\n",
        "//! A file.\nfn f() { let (_, answer) = compute_pair(); inspect(answer); }\n",
    ] {
        assert_eq!(
            kinds(source),
            [],
            "the value remains compiler-visible: {source}"
        );
    }
}

#[test]
fn map_shaped_deserialization_is_closed_to_unknown_fields() {
    for source in [
        "//! A file.\n#[derive(serde::Deserialize)] struct Config { path: String }\n",
        "//! A file.\n#[derive(serde::Deserialize)] enum Message { Start { id: u64 }, Stop }\n",
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(tag = \"type\")] enum State { Waiting, Done }\n",
        "//! A file.\nuse serde::Deserialize as Decode;\n#[derive(Decode)] struct Config { path: String }\n",
        "//! A file.\n#[cfg_attr(test, derive(serde::Deserialize))] struct Config { path: String }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::OpenDeserialization),
            "{source}"
        );
    }
}

#[test]
fn a_deserialize_derive_cannot_be_reexported_under_an_unrecognised_name() {
    for source in [
        "//! A file.\npub use serde::Deserialize as Decode;\n",
        "//! A file.\nmacro_rules! exports { () => { pub use serde::Deserialize as Decode; } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::DeserializeDeriveAlias),
            "the exporting file must fail before another file can write #[derive(Decode)]: {source}"
        );
    }
}

#[test]
fn closed_scalar_and_serialization_only_shapes_keep_their_narrow_boundary() {
    for source in [
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Config { path: String }\n",
        "//! A file.\n#[derive(serde::Deserialize)] struct Id(u64);\n",
        "//! A file.\n#[derive(serde::Deserialize)] enum Colour { Red, Blue }\n",
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(transparent)] struct Values { values: Vec<u64> }\n",
        "//! A file.\n#[derive(serde::Serialize)] struct Output { path: String }\n",
    ] {
        assert!(
            !kinds(source).contains(&Kind::OpenDeserialization),
            "{source}"
        );
    }
}

#[test]
fn current_owned_inputs_cannot_invent_or_ambiguously_absorb_data() {
    for source in [
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Event { #[serde(flatten)] payload: Payload }\n",
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Event { #[serde(default)] detail: String }\n",
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Event { #[serde(alias = \"old\")] detail: String }\n",
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(untagged)] enum Event { Text(String), Number(u64) }\n",
        "//! A file.\n#[derive(serde::Deserialize)] #[serde(tag = \"type\", deny_unknown_fields)] enum Event { Known, #[serde(other)] Unknown }\n",
        "//! A file.\nmacro_rules! event { () => { #[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Event { #[serde(default)] detail: String } } }\n",
    ] {
        let found = scan_source("crates/demo/src/trace/event.rs", source).expect("it parses");
        assert!(
            found
                .iter()
                .any(|finding| finding.kind == Kind::OpenDeserialization),
            "{source}"
        );
    }
    let provider_default = "//! A file.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Response { #[serde(default)] message: Option<String> }\n";
    assert!(
        scan_source("crates/njutest/src/provider.rs", provider_default)
            .expect("it parses")
            .iter()
            .any(|finding| finding.kind == Kind::OpenDeserialization),
        "an owned provider protocol cannot make a missing field indistinguishable from explicit absence"
    );
}

#[test]
fn output_only_options_are_not_input_escapes_and_owned_v1_is_still_exact() {
    let output_only = "//! A file.\n#[derive(serde::Serialize)] struct Event { #[serde(default, flatten, alias = \"old\")] detail: String }\n";
    assert!(
        !scan_source("crates/demo/src/trace/event.rs", output_only)
            .expect("it parses")
            .iter()
            .any(|finding| finding.kind == Kind::OpenDeserialization)
    );
    let historical = "//! A file.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Exchange { #[serde(flatten)] spoken: Spoken }\n";
    assert!(
        scan_source("crates/njutest/src/wire/mod.rs", historical)
            .expect("it parses")
            .iter()
            .any(|finding| finding.kind == Kind::OpenDeserialization),
        "a version number does not license an owned reader to accept more than its schema"
    );
}

#[test]
fn a_macro_cannot_generate_an_open_deserializer() {
    for source in [
        "//! A file.\nmacro_rules! config { () => { #[derive(serde::Deserialize)] struct Config { path: String } } }\n",
        "//! A file.\nmacro_rules! event { () => { #[derive(serde::Deserialize)] #[serde(tag = \"type\")] enum Event { Started { id: u64 } } } }\n",
        "//! A file.\nmacro_rules! config { () => { use serde::Deserialize as Decode; #[derive(Decode)] struct Config { path: String } } }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::OpenDeserialization),
            "{source}"
        );
    }
    assert_eq!(
        kinds(
            "//! A file.\nmacro_rules! config { () => { #[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Config { path: String } } }\n"
        ),
        []
    );
}

#[test]
fn a_macro_cannot_make_a_checked_attribute_opaque_to_the_gate() {
    for source in [
        "//! A file.\nmacro_rules! input { ($derive:path) => { #[derive($derive)] struct Input { value: String } } }\n",
        "//! A file.\nmacro_rules! input { ($policy:meta) => { #[serde($policy)] struct Input { value: String } } }\n",
        "//! A file.\nmacro_rules! input { ($condition:meta) => { #[cfg_attr($condition, derive(serde::Deserialize))] struct Input { value: String } } }\n",
        "//! A file.\n#[cfg_attr(test, derive($derive))]\nstruct Input { value: String }\n",
    ] {
        assert!(
            kinds(source).contains(&Kind::OpaqueMacroSyntax),
            "an attribute this gate cannot parse is a refusal, not evidence of absence: {source}"
        );
    }
}

#[test]
fn a_foreign_protocol_must_capture_unknown_fields_in_one_exact_shape() {
    let captured = "//! Cargo's JSON.\n#[derive(serde::Deserialize)]\nstruct Artifact {\n    name: String,\n    #[serde(flatten)]\n    external_fields: std::collections::BTreeMap<String, serde_json::Value>,\n}\n";
    assert!(
        !scan_source("crates/rust-mutants/src/cargo/messages.rs", captured,)
            .expect("it parses")
            .iter()
            .any(|finding| finding.kind == Kind::OpenDeserialization),
        "foreign additions are retained rather than silently ignored"
    );
    for (file, source) in [
        ("crates/demo/src/wire.rs", captured),
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            "//! Cargo's JSON.\n#[derive(serde::Deserialize)] struct Artifact { name: String, #[serde(flatten)] unknown: std::collections::BTreeMap<String, serde_json::Value> }\n",
        ),
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            "//! Cargo's JSON.\n#[derive(serde::Deserialize)] struct Artifact { name: String, #[serde(flatten)] pub external_fields: std::collections::BTreeMap<String, serde_json::Value> }\n",
        ),
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            "//! Cargo's JSON.\n#[derive(serde::Deserialize)] struct Artifact { name: String, external_fields: std::collections::BTreeMap<String, serde_json::Value> }\n",
        ),
        ("shadow/crates/rust-mutants/src/cargo/messages.rs", captured),
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            "//! Cargo's JSON.\n#[derive(serde::Deserialize)] struct Artifact { name: String, #[serde(flatten)] external_fields: std::collections::BTreeMap<String, serde_json::Value>, #[serde(flatten)] rogue: std::collections::BTreeMap<String, serde_json::Value> }\n",
        ),
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            "//! Cargo's JSON.\n#[derive(serde::Deserialize)] struct Artifact { name: String, #[serde(flatten, default)] external_fields: std::collections::BTreeMap<String, serde_json::Value> }\n",
        ),
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            "//! Cargo's JSON.\n#[derive(serde::Deserialize)] #[serde(deny_unknown_fields)] struct Artifact { name: String, #[serde(flatten)] external_fields: std::collections::BTreeMap<String, serde_json::Value> }\n",
        ),
    ] {
        assert!(
            scan_source(file, source)
                .expect("it parses")
                .iter()
                .any(|finding| finding.kind == Kind::OpenDeserialization),
            "{file}: {source}"
        );
    }
}

#[test]
fn read_dir_entry_errors_are_not_flattened_away() {
    for source in [
        "//! A file.\nfn files(path: &std::path::Path) { for entry in std::fs::read_dir(path).unwrap().flatten() { drop(entry); } }\n",
        "//! A file.\nfn files(path: &std::path::Path) { let entries = std::fs::read_dir(path).unwrap(); for entry in entries.flatten() { drop(entry); } }\n",
        "//! A file.\nfn files(path: &std::path::Path) { let Ok(entries) = std::fs::read_dir(path) else { return; }; for entry in entries.flatten() { drop(entry); } }\n",
        "//! A file.\nfn files(path: &std::path::Path) { match std::fs::read_dir(path) { Ok(entries) => { let _ = entries.flatten().count(); }, Err(_) => {} } }\n",
        "//! A file.\nfn files(path: &std::path::Path) { let _ = std::fs::read_dir(path).map(|entries| entries.flatten().count()); }\n",
        "//! A file.\nuse std::fs::read_dir as directory_entries;\nfn files(path: &std::path::Path) { let entries = directory_entries(path).unwrap(); let _ = entries.flatten().count(); }\n",
    ] {
        let found = kinds(source);
        assert!(found.contains(&Kind::DiscardedResult), "{source}");
    }
}

#[test]
fn ufcs_cannot_hide_read_dir_entry_errors() {
    assert!(
        kinds(
            "//! A file.\nfn files(path: &std::path::Path) -> std::io::Result<()> { let _entries = Iterator::flatten(std::fs::read_dir(path)?); Ok(()) }\n"
        )
        .contains(&Kind::DiscardedResult)
    );
}

#[test]
fn macro_tokens_reject_iterator_flatten_without_confusing_an_unrelated_function() {
    assert_eq!(
        kinds(
            "//! A file.\nfn files(entries: impl Iterator<Item = Result<u8, E>>) { assert!(Iterator::flatten(entries).next().is_none()); }\n"
        ),
        [Kind::DiscardedResult]
    );
    assert_eq!(
        kinds(
            "//! A file.\nfn flatten(value: &str) -> &str { value }\nfn answer() { assert_eq!(flatten(\"kept\"), \"kept\"); }\n"
        ),
        [],
        "a name is not the lossy Iterator operation unless syntax actually calls that operation"
    );
}

#[test]
fn renaming_a_filesystem_iterator_is_refused_at_the_declaration() {
    assert_eq!(
        kinds("//! A file.\npub use std::fs::read_dir as entries;\n"),
        [Kind::DiscardedResult]
    );
}

#[test]
fn a_result_pattern_cannot_end_a_filesystem_walk_silently() {
    assert_eq!(
        kinds(
            "//! A file.\nfn files(path: &std::path::Path) -> std::io::Result<()> { let mut entries = std::fs::read_dir(path)?; while let Some(Ok(entry)) = entries.next() { drop(entry); } Ok(()) }\n"
        ),
        [Kind::DiscardedResult]
    );
}

#[test]
fn an_if_let_result_cannot_hide_its_error_arm() {
    for source in [
        "//! A file.\nfn inspect() { if let Ok(value) = fallible() { use_value(value); } }\n",
        "//! A file.\nfn inspect() { if let std::result::Result::Ok(value) = fallible() { use_value(value); } }\n",
        "//! A file.\nfn inspect(ready: bool) { if ready && let Ok(value) = fallible() { use_value(value); } }\n",
        "//! A file.\nmacro_rules! inspect { () => { if let Ok(value) = fallible() { use_value(value); } } }\n",
        "//! A file.\nmacro_rules! inspect { ($pattern:pat, $value:expr) => { if let $pattern = $value { use_value(); } } }\n",
        "//! A file.\ninspect!(if let Ok(value) = fallible() { use_value(value); });\n",
    ] {
        assert_eq!(kinds(source), [Kind::DiscardedResult], "{source}");
    }
    for source in [
        "//! A file.\nfn inspect() -> Result<(), Error> { let value = fallible()?; use_value(value); Ok(()) }\n",
        "//! A file.\nfn inspect() { match fallible() { Ok(value) => use_value(value), Err(error) => record(error) } }\n",
        "//! A file.\nfn inspect() { let Ok(value) = fallible() else { record_failure(); return; }; use_value(value); }\n",
        "//! A file.\nfn inspect(value: Option<u8>) { if let Some(value) = value { use_value(value); } }\n",
    ] {
        assert_eq!(
            kinds(source),
            [],
            "the failure policy is explicit: {source}"
        );
    }
}

#[test]
fn the_documented_catalog_and_its_boundaries_match_the_gate() {
    let root = xtask::gates::workspace_root();
    let development = std::fs::read_to_string(root.join("docs/development.md"))
        .expect("the development contract");
    let development_words = development.split_whitespace().collect::<Vec<_>>().join(" ");
    for kind in Kind::ALL {
        let documented = format!("`{}`", kind.label());
        assert!(
            development.contains(&documented),
            "the executable lint kind {documented} needs its policy in docs/development.md"
        );
    }
    for statement in [
        "The `lints` walk is fail-closed",
        "A borrowed `&dyn Trait` is allowed",
        "Tests are in that set",
        "Macro definitions and invocations in repository source",
        "`#[expect(clippy::expect_used, reason = \"…\")]`",
    ] {
        assert!(
            development_words.contains(statement),
            "missing {statement:?}"
        );
    }

    let readme = std::fs::read_to_string(root.join("README.md")).expect("the front page");
    assert!(
        readme.contains("docs/development.md"),
        "the design rules are documented once, on the developer page, and the front \
         page sends a reader there. It used to repeat four of them, which is two \
         declarations of one decision and a front page nobody finishes: {readme}"
    );
}

#[test]
fn a_finding_says_where_it_is_and_what_to_do_instead() {
    let findings = scan_source(
        "crates/a/src/lib.rs",
        "//! A file.\n\n#[allow(dead_code)]\npub fn f() {}\n",
    )
    .expect("the source parses");
    let rendered = findings.first().expect("one finding").to_string();
    assert!(rendered.starts_with("crates/a/src/lib.rs:3:"), "{rendered}");
    assert!(rendered.contains("#[expect("), "{rendered}");
}

#[test]
fn a_file_that_is_not_rust_is_an_error_rather_than_a_pass() {
    scan_source("a.rs", "fn (").expect_err("a file this gate cannot read is not a file it passes");
}

#[test]
fn a_comment_beside_the_code_is_refused_and_documentation_is_not() {
    assert_eq!(
        kinds("//! A file.\n\npub fn f() {\n    // why\n}\n"),
        [Kind::Comment],
        "a comment beside code is a second account of it that nothing keeps true"
    );
    assert_eq!(
        kinds("//! A file.\n\n/// What it is.\npub fn f() {}\n"),
        [],
        "the one line the lint set asks for on a public item is documentation, not a comment"
    );
    assert_eq!(
        kinds("//! A file.\n\npub fn f() {\n    let x = 1; // here\n    drop(x);\n}\n"),
        [Kind::Comment],
        "and one at the end of a line is one too"
    );
    assert_eq!(
        kinds("//! A file.\n\n/* why */\npub fn f() {}\n"),
        [Kind::Comment],
        "whichever way it is spelled"
    );
    assert_eq!(
        kinds("//! A file.\n\n/** What it is. */\npub fn f() {}\n"),
        [],
        "and a block that documents is documentation"
    );
}

#[test]
fn the_licence_header_is_not_a_comment_this_gate_refuses() {
    assert_eq!(
        kinds(
            "// SPDX-FileCopyrightText: 2026 njutest contributors\n             // SPDX-License-Identifier: MIT OR Apache-2.0\n\n//! A file.\n"
        ),
        [],
        "every file of this repository carries it"
    );
}

#[test]
fn slashes_inside_a_literal_are_not_a_comment() {
    for source in [
        "//! A file.\npub const URL: &str = \"https://example.test/a\";\n",
        "//! A file.\npub const RAW: &str = r\"https://example.test/a\";\n",
        "//! A file.\npub const HASHED: &str = r#\"a \"//\" b\"#;\n",
        "//! A file.\npub const BYTES: &[u8] = b\"//\";\n",
        "//! A file.\npub const SLASH: char = '/';\n",
        "//! A file.\npub fn f(s: &'static str) -> &'static str {\n    s\n}\n",
        "//! A file.\npub const ESCAPED: &str = \"a\\\\\";\n",
    ] {
        assert_eq!(
            kinds(source),
            [],
            "the text a program carries is not a thing anybody said about it: {source}"
        );
    }
}

#[test]
fn the_engine_s_own_annotation_is_an_instruction_rather_than_an_account() {
    assert_eq!(
        kinds("//! A file.\n\npub fn f() {\n    // rust-mutants: skip nothing to see\n}\n"),
        [],
        "a skip marker is read by the engine, and its syntax is what says so"
    );
}

#[test]
fn the_development_page_names_every_kind_this_gate_reports() {
    let page = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("the workspace root")
            .join("docs/development.md"),
    )
    .expect("the page");
    for kind in Kind::ALL {
        assert!(
            page.contains(&format!("`{}`", kind.label())),
            "a finding says {} and docs/development.md does not say what it is",
            kind.label()
        );
    }
}

/// A recursive removal in a loop is refused, and one of a single directory is not.
#[test]
fn a_removal_in_a_loop_is_refused_and_one_of_a_named_directory_is_not() {
    let looping = r"
        fn sweep(directories: &[std::path::PathBuf]) {
            for one in directories {
                drop(std::fs::remove_dir_all(one));
            }
        }
    ";
    let single = r"
        fn close(dir: &std::path::Path) -> std::io::Result<()> {
            std::fs::remove_dir_all(dir)
        }
    ";
    let found = scan_source("crates/demo/src/lib.rs", looping).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::UnboundedRemoval),
        "a loop of removals is what runs for a day on a wedged mount while saying nothing: \
         {found:?}"
    );
    let found = scan_source("crates/demo/src/lib.rs", single).expect("it parses");
    assert!(
        found.iter().all(|one| one.kind != Kind::UnboundedRemoval),
        "one directory a caller names is one removal, and a budget over one thing is a \
         bound on nothing: {found:?}"
    );
    assert!(
        found.iter().any(|one| one.kind == Kind::RawTreeRemoval),
        "but shipped code removes a run's tree through tempowner, which gets past a directory \
         a panicking test left read-only: {found:?}"
    );
    let found = scan_source("crates/rust-mutants/src/tempowner/mod.rs", single).expect("it parses");
    assert!(
        found.is_empty(),
        "the one module that restores access on the way down is the one that may: {found:?}"
    );
}

/// A child given its temporary directory in one variable is refused, and one given it in all of them is not.
#[test]
fn a_temporary_directory_named_in_one_variable_is_refused() {
    for name in ["TMPDIR", "TMP", "TEMP"] {
        let lone = format!(
            r#"
            fn run(command: &mut std::process::Command, dir: &std::path::Path) {{
                command.env("NO_COLOR", "1").env("{name}", dir);
            }}
            "#
        );
        let found = scan_source("crates/demo/tests/run.rs", &lone).expect("it parses");
        assert!(
            found
                .iter()
                .any(|one| one.kind == Kind::LoneTemporaryVariable),
            "std::env::temp_dir reads TMP and TEMP on Windows and TMPDIR elsewhere, so {name} \
             alone leaves the child writing into the parent's directory on some platform: \
             {found:?}"
        );
    }
    let every = r#"
        fn run(command: &mut std::process::Command, dir: &std::path::Path) {
            command
                .env("NO_COLOR", "1")
                .envs(njutest_devkit::paths::temporary_directory(dir));
        }
    "#;
    let found = scan_source("crates/demo/tests/run.rs", every).expect("it parses");
    assert!(
        found.is_empty(),
        "a directory named in every variable a platform reads is the one the child uses: \
         {found:?}"
    );
}

/// A command handed to a reader with an identity in it is refused.
#[test]
fn a_command_built_from_an_identity_is_refused() {
    let perishable = r#"
        fn said(mutant: &Mutant) -> String {
            format!("rust-mutants explain {}", mutant.display_id)
        }
    "#;
    let holding = r#"
        fn said(mutant: &Mutant) -> String {
            format!("rust-mutants explain {}:{}:{}", mutant.path, mutant.item, mutant.rule)
        }
    "#;
    let found = scan_source("crates/demo/src/lib.rs", perishable).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::PerishableHandle),
        "the edit that closes a survivor re-mints the identity naming it, so a command \
         printed with one stops working the moment it is followed: {found:?}"
    );
    let found = scan_source("crates/demo/src/lib.rs", holding).expect("it parses");
    assert!(
        found.is_empty(),
        "a locator holds through that edit, which is the whole reason it exists: {found:?}"
    );
}

/// A test that joins a layout constant is refused, and one that asks for the path is not.
#[test]
fn a_test_that_decides_the_layout_is_refused() {
    let source = "pub const LAYOUT: &str = \"reports/runs\";\n";
    let exported = xtask::lints::exported_strings(source);
    assert_eq!(
        exported,
        vec![(String::from("LAYOUT"), 1)],
        "a constant with a separator in it spells a structure rather than a name"
    );
    assert!(
        xtask::lints::exported_strings("pub const NAME: &str = \"runs\";\n").is_empty(),
        "and one without a separator is a name, which is a thing worth exporting"
    );

    let test = "use njutest::app::reports::LAYOUT;\nlet path = root.join(LAYOUT);\n";
    assert!(xtask::lints::joins(test, "LAYOUT"));
    assert_eq!(
        xtask::lints::imported_from(test, "LAYOUT").as_deref(),
        Some("reports"),
        "the import is what tells four constants of the same name apart"
    );

    let asking = "use njutest::app::reports::Store;\nlet path = Store::read(root).runs();\n";
    assert!(
        !xtask::lints::joins(asking, "LAYOUT"),
        "a test that asks the type that owns the layout decides nothing"
    );
    assert_eq!(
        xtask::lints::imported_from("let it = a::b::config::LAYOUT;\n", "LAYOUT").as_deref(),
        Some("config"),
        "a name written out in full at the point it is used is reached from a module too"
    );
}

#[test]
fn a_directory_the_configuration_can_move_is_spelled_in_one_place() {
    let source = "/// where reports/runs went\npub(crate) const DEFAULT_REPORTS_DIRECTORY: &str \
                  = \"artifacts\";\nconst OTHER_DIR: &str = \"vendor\";\n";
    assert_eq!(
        xtask::lints::configured_directories(source),
        vec![String::from("artifacts")],
        "a default the configuration falls back to is a directory somebody can rename, \
         and one that is not a default is not"
    );

    let directories = vec![String::from("artifacts")];
    assert_eq!(
        xtask::lints::spelled(
            "let path = root.join(\"artifacts/runs/one\");\n",
            &directories
        ),
        vec![1],
        "a path literal under a directory the configuration can move decides it"
    );
    assert_eq!(
        xtask::lints::spelled("let path = root.join(\"artifacts\");\n", &directories),
        vec![1],
        "and so does the directory itself, joined onto a root"
    );
    assert!(
        xtask::lints::spelled("let held = it.expect(\"artifacts\");\n", &directories).is_empty(),
        "while the same word said to a reader is not a path at all"
    );
    assert!(
        xtask::lints::spelled("/// under artifacts/runs\n", &directories).is_empty(),
        "and documentation may say where things are, which is what it is for"
    );
    assert!(
        xtask::lints::spelled("let path = Store::read(root).run(\"one\");\n", &directories)
            .is_empty(),
        "asking the type that owns the layout decides nothing"
    );
}

#[test]
fn a_colour_spelled_by_hand_is_refused_wherever_it_is_not_the_one_place_that_paints() {
    let by_hand = r#"
        pub const fn paint(killed: bool) -> &'static str {
            if killed { "\u{1b}[32m" } else { "\u{1b}[31m" }
        }
    "#;
    let found = scan_source("crates/demo/src/ui.rs", by_hand).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::HandPainted),
        "a second module that spells an escape decides for itself what green means, what \
         a reader's terminal can take, and whether to paint at all — and then the two \
         halves of one workspace look like two tools. The set of things that carry a \
         colour is closed; the set of places that turn one into bytes has to be one: \
         {found:?}"
    );
    let named = r"
        fn drawn(telling: Telling, word: &str) -> String {
            telling.painted(Style::Gap, word)
        }
    ";
    assert!(
        scan_source("crates/demo/src/ui.rs", named)
            .expect("it parses")
            .is_empty(),
        "asking for what a thing is rather than for a colour is the whole point, and it \
         is not what this refuses"
    );
}

#[test]
fn a_type_that_publishes_its_whole_list_may_not_also_say_the_list_is_open() {
    let both = r"
        /// Every way a thing can go.
        #[non_exhaustive]
        pub enum Outcome {
            /// One.
            Killed,
            /// Another.
            Survived,
        }

        impl Outcome {
            /// Every outcome, in declaration order.
            pub const ALL: [Self; 2] = [Self::Killed, Self::Survived];
        }
    ";
    let found = scan_source("crates/demo/src/outcome.rs", both).expect("it parses");
    assert!(
        found.iter().any(|one| one.kind == Kind::OpenAndClosed),
        "`ALL` promises this is every one of them and `#[non_exhaustive]` promises it is \
         not, so a caller outside the crate is made to write an arm for a case the list \
         says cannot exist — and the arm it writes counts the next variant as whatever \
         was nearest. A tally reached one of those and reported every future outcome as \
         a harness failure: {found:?}"
    );

    let closed = r"
        pub enum Outcome { Killed, Survived }
        impl Outcome {
            pub const ALL: [Self; 2] = [Self::Killed, Self::Survived];
        }
    ";
    assert_eq!(
        scan_source("crates/demo/src/outcome.rs", closed)
            .expect("it parses")
            .into_iter()
            .map(|finding| finding.kind)
            .collect::<Vec<_>>(),
        [Kind::ManualVariantList],
        "the set is closed, but the compiler must generate the list from its declaration"
    );

    let open = r"
        #[non_exhaustive]
        pub enum RunError { Refused, Stopped }
    ";
    assert!(
        scan_source("crates/demo/src/error.rs", open)
            .expect("it parses")
            .is_empty(),
        "and an error a caller branches on must already handle one it does not know, so \
         the attribute costs nothing where no list was promised"
    );
}

#[test]
fn nesting_and_cfg_attr_do_not_hide_an_open_and_closed_type() {
    for source in [
        r"
            mod nested {
                #[non_exhaustive]
                pub enum State { Waiting, Done }
                impl State { pub const ALL: &[Self] = &[Self::Waiting, Self::Done]; }
            }
        ",
        r"
            #[cfg_attr(test, non_exhaustive)]
            pub enum State { Waiting, Done }
            impl State { pub const ALL: &[Self] = &[Self::Waiting, Self::Done]; }
        ",
    ] {
        assert_eq!(
            kinds(source),
            [Kind::ManualVariantList, Kind::OpenAndClosed],
            "{source}"
        );
    }
}

#[test]
fn renaming_all_to_an_exhaustive_function_does_not_open_the_set() {
    for list in [
        "impl State { pub fn every() -> [Self; 2] { [Self::Waiting, Self::Done] } }",
        "impl State { pub fn variants() -> &'static [Self] { &[Self::Waiting, Self::Done] } }",
        "impl State { pub fn specimens() -> [Self; 2] { [Self::Waiting, Self::Done] } }",
    ] {
        let source = format!("#[non_exhaustive]\npub enum State {{ Waiting, Done }}\n{list}\n");
        assert_eq!(
            kinds(&source),
            [Kind::ManualVariantList, Kind::OpenAndClosed],
            "{source}"
        );
    }
}

#[test]
fn whole_variant_lists_are_generated_from_the_enum_declaration() {
    let manual = r"
        enum State { Waiting, Done }
        impl State { const BOTH: [Self; 2] = [Self::Waiting, Self::Done]; }
    ";
    assert_eq!(kinds(manual), [Kind::ManualVariantList]);

    let subset = r"
        enum State { Waiting, Running, Done }
        impl State { const TERMINAL: [Self; 1] = [Self::Done]; }
    ";
    assert!(kinds(subset).is_empty(), "a named proper subset is not ALL");

    let generated = r"
        #[derive(njutest_macros::AllVariants)]
        enum State { Waiting, Done }
        impl State { const ALL: [Self; 2] = [Self::Waiting, Self::Done]; }
    ";
    assert!(
        kinds(generated).is_empty(),
        "the compiler itself rejects the duplicate generated ALL before this syntax gate runs"
    );
}

#[test]
fn a_whole_list_split_from_its_enum_is_still_refused() {
    let found = manual_variant_lists_across([
        ("demo", "src/state.rs", "enum State { Waiting, Done }"),
        (
            "demo",
            "src/list.rs",
            "impl crate::state::State { const STATES: [Self; 2] = [Self::Waiting, Self::Done]; }",
        ),
    ])
    .expect("both files parse");
    assert_eq!(
        found,
        [xtask::lints::Finding {
            kind: Kind::ManualVariantList,
            file: "src/list.rs".to_owned(),
            line: 1,
        }]
    );
}

#[test]
fn a_total_inherent_match_also_declares_the_set_closed() {
    for body in [
        "match self { Self::Waiting => 0, Self::Done => 1 }",
        "match *self { Self::Waiting => 0, Self::Done => 1 }",
    ] {
        let source = format!(
            "#[non_exhaustive]\npub enum State {{ Waiting, Done }}\nimpl State {{ pub const fn code(&self) -> u8 {{ {body} }} }}\n"
        );
        assert_eq!(kinds(&source), [Kind::OpenAndClosed], "{source}");
    }
    let extensible_error = "#[non_exhaustive]\npub enum Error { Refused, Stopped }\nimpl Error { pub fn known(&self) -> bool { match self { Self::Refused => true, _ => false } } }\n";
    assert!(
        !kinds(extensible_error).contains(&Kind::OpenAndClosed),
        "an explicit remainder does not claim to publish the whole future set"
    );
    for extensible_error in [
        "#[derive(thiserror::Error)]\n#[non_exhaustive]\npub enum Error { #[error(\"refused\")] Refused, #[error(\"stopped\")] Stopped }\nimpl Error { pub fn code(&self) -> u8 { match self { Self::Refused => 1, Self::Stopped => 2 } } }\n",
        "#[derive(Debug)]\n#[non_exhaustive]\npub enum Error { Refused, Stopped }\nimpl std::error::Error for Error {}\nimpl Error { pub fn code(&self) -> u8 { match self { Self::Refused => 1, Self::Stopped => 2 } } }\n",
    ] {
        assert!(
            !kinds(extensible_error).contains(&Kind::OpenAndClosed),
            "a typed public error is the deliberate open-set boundary: {extensible_error}"
        );
    }
}

#[test]
fn an_open_enum_and_its_closed_list_cannot_be_split_across_files() {
    let declaration = "#[non_exhaustive]\npub enum State { Waiting, Done }\n";
    let list =
        "impl crate::state::State { pub const ALL: &[Self] = &[Self::Waiting, Self::Done]; }\n";
    let found = open_and_closed_across([
        ("demo", "src/state.rs", declaration),
        ("demo", "src/state/list.rs", list),
    ])
    .expect("both files parse");
    assert_eq!(
        found,
        [xtask::lints::Finding {
            kind: Kind::OpenAndClosed,
            file: "src/state.rs".to_owned(),
            line: 2,
        }]
    );
    assert!(
        open_and_closed_across([("one", "one.rs", declaration), ("two", "two.rs", list),])
            .expect("both files parse")
            .is_empty(),
        "equal type names in different crates do not denote the same type"
    );
    assert!(
        open_and_closed_across([
            ("demo", "src/error.rs", declaration),
            ("demo", "src/error/list.rs", list),
            (
                "demo",
                "src/error/trait.rs",
                "impl std::error::Error for crate::state::State {}\n",
            ),
        ])
        .expect("all files parse")
        .is_empty(),
        "an Error implementation in another module keeps the public error boundary extensible"
    );
}

#[test]
fn a_struct_from_another_crate_may_not_leave_its_remainder_to_that_crate() {
    let source = "use other_crate::session::Options;\n\
                  fn go() -> Options { Options { verify: true, ..Options::default() } }\n";
    assert_eq!(
        kinds(source),
        [Kind::ForeignRemainder],
        "the fields this does not name are answered by whoever owns Options, and a field \
         they add next arrives here decided: {source}"
    );
}

#[test]
fn a_remainder_this_crate_computed_is_not_the_owners_idea_of_neutral() {
    let ours = "use other_crate::session::Options;\n\
                fn switches() -> Options { Options { verify: true, quiet: false } }\n\
                fn go() -> Options { Options { verify: false, ..switches() } }\n";
    assert!(
        !kinds(ours).contains(&Kind::ForeignRemainder),
        "a remainder taken from a value this crate built names every field somewhere, \
         which is the whole ask: {ours}"
    );
    let local = "struct Options { verify: bool }\n\
                 fn go() -> Options { Options { verify: true, ..Options::default() } }\n";
    assert!(
        !kinds(local).contains(&Kind::ForeignRemainder),
        "and a type this file declares is one whose new field is the same change and the \
         same review: {local}"
    );
    let rooted = "fn go() -> crate::Options { crate::Options { ..Default::default() } }\n";
    assert!(
        !kinds(rooted).contains(&Kind::ForeignRemainder),
        "as is one this crate declares elsewhere: {rooted}"
    );
}

#[test]
fn a_renamed_import_does_not_hide_which_crate_owns_the_remainder() {
    let renamed = "use other_crate::session::Options as Knobs;\n\
                   fn go() -> Knobs { Knobs { verify: true, ..Default::default() } }\n";
    assert_eq!(
        kinds(renamed),
        [Kind::ForeignRemainder],
        "the name is this crate's and the fields are not: {renamed}"
    );
    let grouped = "use other_crate::session::{Failing, Options};\n\
                   fn go() -> Options { Options { verify: true, ..Options::default() } }\n";
    assert_eq!(
        kinds(grouped),
        [Kind::ForeignRemainder],
        "and a grouped import roots every leaf the same way: {grouped}"
    );
}

#[test]
fn code_that_measures_rather_than_ships_may_take_the_owners_defaults() {
    let source = "use other_crate::session::Options;\n\
                  fn go() -> Options { Options { verify: true, ..Options::default() } }\n";
    for measuring in [
        "crates/x/tests/a.rs",
        "crates/x/benches/a.rs",
        "crates/x/examples/a.rs",
    ] {
        let found = scan_source(measuring, source).expect("the source parses");
        assert!(
            !found.iter().any(|one| one.kind == Kind::ForeignRemainder),
            "{measuring} asks no question of those switches, so it answers none of them \
             wrongly: {found:?}"
        );
    }
}

#[test]
fn a_whole_set_list_is_refused_wherever_it_is_written() {
    let listed = "enum Step { One, Two, Three }\n\
                  fn every() -> [Step; 3] { [Step::One, Step::Two, Step::Three] }\n";
    assert_eq!(
        kinds(listed),
        [Kind::ManualVariantList],
        "the rule used to read only an `impl` of the enum, so the same list in a free \
         function, a test, or a generator was invisible: {listed}"
    );
    let generated = "enum Step { One, Two, Three }\n\
                     fn every() -> [Step; 3] { Step::ALL }\n";
    assert!(
        !kinds(generated).contains(&Kind::ManualVariantList),
        "{generated}"
    );
    let part = "enum Step { One, Two, Three }\n\
                fn some() -> [Step; 2] { [Step::One, Step::Two] }\n";
    assert!(
        !kinds(part).contains(&Kind::ManualVariantList),
        "a list of some of a set claims nothing about the rest: {part}"
    );
}

#[test]
fn a_generator_and_a_screen_do_not_hide_which_set_a_list_is_over() {
    let wrapped = "enum Step { One, Two }\n\
                   fn every() { prop_oneof![Just(Step::One), Just(Step::Two)]; }\n";
    assert_eq!(
        kinds(wrapped),
        [Kind::ManualVariantList],
        "a generator wraps each in `Just`, which changes nothing about the set: {wrapped}"
    );
    let asked = "enum Step { One, Two }\n\
                 fn every() { let _ = [Step::One.name(), Step::Two.name()]; }\n";
    assert_eq!(
        kinds(asked),
        [Kind::ManualVariantList],
        "nor does asking each for its name: {asked}"
    );
}

#[test]
fn a_list_the_same_body_matches_totally_is_one_the_compiler_already_holds() {
    let held = "enum Step { One, Two { at: u8 } }\n\
                fn every() -> [Step; 2] {\n\
                    let steps = [Step::One, Step::Two { at: 1 }];\n\
                    for step in &steps { match step { Step::One | Step::Two { .. } => {} } }\n\
                    steps\n\
                }\n";
    assert!(
        !kinds(held).contains(&Kind::ManualVariantList),
        "the match decides nothing and is the whole point: a variant added to Step makes \
         this function stop compiling, which is the only thing a list of data-bearing \
         variants can be held by: {held}"
    );
    let partial = "enum Step { One, Two { at: u8 } }\n\
                   fn every() -> [Step; 2] {\n\
                       let steps = [Step::One, Step::Two { at: 1 }];\n\
                       for step in &steps { match step { Step::One => {}, _ => {} } }\n\
                       steps\n\
                   }\n";
    assert!(
        kinds(partial).contains(&Kind::ManualVariantList),
        "while a match with an arm that catches the rest sends nobody anywhere: {partial}"
    );
    let elsewhere = "enum Step { One, Two { at: u8 } }\n\
                     fn every() -> [Step; 2] { [Step::One, Step::Two { at: 1 }] }\n\
                     fn apart(step: &Step) { match step { Step::One | Step::Two { .. } => {} } }\n";
    assert!(
        kinds(elsewhere).contains(&Kind::ManualVariantList),
        "and a match in another function is one the next person has no reason to read: \
         {elsewhere}"
    );
}

#[test]
fn a_set_that_says_it_may_grow_is_not_asked_for_a_list_nothing_could_hold() {
    let open = "#[non_exhaustive]\nenum Step { One, Two }\n\
                fn every() -> [Step; 2] { [Step::One, Step::Two] }\n";
    assert!(
        !kinds(open).contains(&Kind::ManualVariantList),
        "no match of an open set is exhaustive either, so there is nothing to ask for: {open}"
    );
}

#[test]
fn a_shell_compiled_only_for_unix_or_only_compared_against_is_not_a_bare_one() {
    for passing in [
        "#[cfg(unix)] fn argv() -> &'static str { \"sh\" }",
        "#![cfg(unix)]\nfn argv() -> &'static str { \"sh\" }",
        "#[cfg(all(unix, test))] mod tests { fn argv() -> Vec<&'static str> { vec![\"sh\"] } }",
        "impl Launcher { #[cfg(unix)] fn argv() -> &'static str { \"sh\" } }",
        "fn script(path: &std::path::Path) -> bool { path.extension().is_some_and(|e| e == \"sh\") }",
        "fn other(text: &str) -> bool { text != \"sh\" }",
        "macro_rules! m { ($x:expr) => { $x == \"sh\" } }",
        "fn name() -> &'static str { \"shell\" }",
    ] {
        assert!(
            !kinds(passing).contains(&Kind::BareShell),
            "a shell only Unix compiles, or a name only compared against, is no program started \
             on a platform without one: {passing}"
        );
    }
    let finder = scan_source(
        "crates/njutest-devkit/src/paths.rs",
        "fn candidates() -> [&'static str; 2] { [\"sh\", \"sh.exe\"] }",
    );
    assert!(
        finder.is_ok_and(|found| found.iter().all(|one| one.kind != Kind::BareShell)),
        "the one place that looks for the shell names what it looks for"
    );
}

#[test]
fn text_read_by_the_reader_or_for_a_test_or_as_no_rust_is_no_raw_lexing() {
    let found = |path: &str, source: &str| -> Vec<Kind> {
        scan_source(path, source)
            .expect("the source parses")
            .into_iter()
            .map(|finding| finding.kind)
            .collect()
    };
    for (path, passing) in [
        (
            "crates/rust-mutants/src/parsing.rs",
            "fn read(text: &str) -> bool { syn::parse_str::<syn::Expr>(text).is_ok() }",
        ),
        (
            "crates/app/src/lib.rs",
            "#[cfg(test)] mod tests { fn read(text: &str) -> bool { syn::parse_file(text).is_ok() } }",
        ),
        (
            "crates/app/src/lib.rs",
            "fn count(text: &str) -> Option<u32> { text.parse::<u32>().ok() }",
        ),
        (
            "crates/app/src/lib.rs",
            "fn table(text: &str) -> bool { toml::from_str::<toml::Table>(text).is_ok() }",
        ),
        (
            "crates/njutest-macros/src/lib.rs",
            "fn tokens() -> proc_macro2::TokenStream { quote::quote!(1) }",
        ),
        (
            "crates/app/tests/suite.rs",
            "fn read(text: &str) -> bool { syn::parse_file(text).is_ok() }",
        ),
    ] {
        assert!(
            !found(path, passing).contains(&Kind::RawLexing),
            "the reader itself, code compiled only for tests, text that is not Rust, the macros' \
             own tokens and a suite are no lexing into a shipped thread's map: {path}: {passing}"
        );
    }
}

#[test]
fn an_environment_held_as_raw_pairs_is_refused_wherever_it_is_named() {
    for refused in [
        "fn given() -> Vec<(OsString, OsString)> { Vec::new() }",
        "struct Options { env: Vec<(std::ffi::OsString, std::ffi::OsString)> }",
        "fn read(env: &[(OsString, OsString)]) {}",
        "fn one() -> (OsString, OsString) { unimplemented!() }",
    ] {
        assert!(
            kinds(refused).contains(&Kind::RawEnvironment),
            "an environment held as raw pairs is one any reader compares names in by bytes, \
             which is the comparison Windows does not make: {refused}"
        );
    }
    for passing in [
        "fn given() -> Variables { Variables::of([(OsString::from(\"A\"), OsString::from(\"b\"))]) }",
        "fn names(env: &Variables) -> Vec<&OsStr> { env.for_process().map(|(n, _)| n).collect() }",
        "fn pair() -> (OsString, String) { unimplemented!() }",
        "fn paths() -> Vec<(PathBuf, OsString)> { Vec::new() }",
    ] {
        assert!(
            !kinds(passing).contains(&Kind::RawEnvironment),
            "a pair built to hand to `Variables`, or one that is not two names and values, is \
             no environment: {passing}"
        );
    }
    for held in [
        "crates/rust-mutants/src/vars.rs",
        "crates/njutest-devkit/src/paths.rs",
    ] {
        let found = scan_source(
            held,
            "fn given() -> Vec<(OsString, OsString)> { Vec::new() }",
        );
        assert!(
            found.is_ok_and(|found| found.iter().all(|one| one.kind != Kind::RawEnvironment)),
            "{held} is where the pairs are read, or test support the engine cannot be a \
             dependency of"
        );
    }
}

#[test]
fn only_the_runner_signals_a_process_group_in_shipped_code() {
    let shipped = |file: &str, source: &str| {
        scan_source(file, source)
            .expect("the source parses")
            .into_iter()
            .any(|finding| finding.kind == Kind::RawGroupSignal)
    };
    let group_kill = "fn stop(pid: rustix::process::Pid) { let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL); }";
    assert!(
        shipped("crates/njutest/src/provider.rs", group_kill),
        "a second place that signals a group decides again what the kernel's refusal means"
    );
    assert!(
        !shipped("crates/rust-mutants/src/runner/unix.rs", group_kill),
        "the runner is where the question is answered for everybody"
    );
    assert!(
        !shipped("crates/njutest/tests/toolchain_interrupt.rs", group_kill),
        "a test that interrupts a process the way a person would is not shipped code"
    );
    for passing in [
        "fn stop(child: &mut std::process::Child) -> std::io::Result<()> { child.kill() }",
        "fn alive(pid: rustix::process::Pid) -> bool { rustix::process::test_kill_process(pid).is_ok() }",
        "fn kill() {}",
    ] {
        assert!(
            !shipped("crates/njutest/src/provider.rs", passing),
            "a child's own `kill`, a liveness probe, and a function merely named `kill` signal no group: {passing}"
        );
    }
}
