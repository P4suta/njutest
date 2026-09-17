// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one list of types a guard may compare a value of against what a return replacement would write.

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
