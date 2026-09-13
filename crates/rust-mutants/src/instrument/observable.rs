// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one list of types a guard may compare a value of against what a return replacement would write.
//!
//! A probe reads `==` as the answer to "could this test have seen the
//! replacement", so it is stated only where equality is the whole of what a
//! program can tell apart. A `PartialEq` that ignores a field would answer
//! "no difference" for a state that differs, and the target would be
//! discharged — a test that could kill the mutant removed without ever
//! running.
//!
//! Floats are not in it. `-0.0 == 0.0` holds and `-0.0` is not what
//! `Default::default()` writes, so a probe there would call a mutation that
//! changed the sign of a zero no change at all.
//!
//! The standard library's own types are in it for the reason the primitives
//! are: comparing two of them runs none of the program's code, and coherence
//! lets nobody give them another `PartialEq`. A container is in it only when
//! what it holds is, because comparing a `Vec<T>` is comparing `T` in a loop.
//!
//! Two generated modules name this trait: the witness tree, which puts the
//! question to the compiler, and the runtime of the instrumented tree, which
//! answers it at run time. They must implement it for exactly the same types —
//! a witness tree that vouches for more than the runtime accepts is an
//! instrumented tree that does not build — so both are rendered from here.

/// Every primitive whose equality is the whole of what a program can tell apart.
pub(super) const PRIMITIVE: [&str; 15] = [
    "bool", "char", "()", "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64",
    "u128", "usize",
];

/// The trait declaration and its impls, for a generated module that calls the trait `name` and reaches the standard library as `krate`.
#[must_use]
pub(super) fn declaration(name: &str, krate: &str) -> String {
    let mut written: Vec<String> = PRIMITIVE
        .iter()
        .map(|one| format!("impl {name} for {one} {{}}"))
        .collect();
    written.push(format!("impl {name} for str {{}}"));
    written.push(format!("impl {name} for {krate}::string::String {{}}"));
    written.push(format!(
        "impl<T: {name}> {name} for {krate}::option::Option<T> {{}}"
    ));
    written.push(format!(
        "impl<T: {name}> {name} for {krate}::vec::Vec<T> {{}}"
    ));
    written.push(format!("impl<T: {name} + ?Sized> {name} for &T {{}}"));
    format!("pub(crate) trait {name} {{}} {}", written.join(" "))
}

/// The bound a value must satisfy before a guard may ask whether it already holds what a replacement would write.
#[must_use]
pub(super) fn bound(name: &str, krate: &str) -> String {
    format!("{name} + {krate}::default::Default + {krate}::cmp::PartialEq")
}
