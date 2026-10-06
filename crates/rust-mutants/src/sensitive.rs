// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Values an operation may use but diagnostics may never display.

/// An owned or borrowed value whose diagnostic representations are always redacted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Sensitive<T>(T);

impl<T> Sensitive<T> {
    /// Protects `value` at its input boundary.
    #[must_use]
    pub const fn new(value: T) -> Self {
        Self(value)
    }

    /// Borrows the value explicitly for the operation that requires it.
    #[must_use]
    pub const fn expose(&self) -> &T {
        &self.0
    }
}

impl<T> std::fmt::Debug for Sensitive<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[redacted]")
    }
}

impl<T> std::fmt::Display for Sensitive<T> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("[redacted]")
    }
}

#[cfg(test)]
mod tests {
    use super::Sensitive;

    #[test]
    fn diagnostics_never_call_the_inner_values_formatters() {
        struct Unformattable;
        let protected = Sensitive::new("synthetic-auth-value");
        assert_eq!(
            format!("{protected:?} {protected} {protected:#?}"),
            "[redacted] [redacted] [redacted]"
        );
        assert_eq!(*protected.expose(), "synthetic-auth-value");
        let bytes = Sensitive::new([1, 2, 3]);
        assert_eq!(format!("{bytes:?} {bytes}"), "[redacted] [redacted]");
        let opaque = Sensitive::new(Unformattable);
        assert_eq!(format!("{opaque:?} {opaque}"), "[redacted] [redacted]");
    }
}
