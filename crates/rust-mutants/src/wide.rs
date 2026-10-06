// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A 128-bit integer as a record holds it: its decimal digits in a string, because a JSON reader keeps a number exact only within 64 bits.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A `u128` a record spells as its canonical decimal digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Wide(u128);

impl Wide {
    /// The integer.
    #[must_use]
    pub const fn get(self) -> u128 {
        self.0
    }
}

impl From<u128> for Wide {
    fn from(value: u128) -> Self {
        Self(value)
    }
}

impl Serialize for Wide {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Wide {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let spelled = String::deserialize(deserializer)?;
        match spelled.parse::<u128>() {
            Ok(value) if value.to_string() == spelled => Ok(Self(value)),
            Ok(value) => Err(serde::de::Error::custom(format!(
                "{spelled:?} spells {value} other than as its canonical decimal digits"
            ))),
            Err(source) => Err(serde::de::Error::custom(format!(
                "{spelled:?} is not a 128-bit integer: {source}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Wide;

    #[test]
    fn every_width_reads_back_as_the_integer_it_was_written_from() {
        for value in [
            0,
            1,
            u128::from(u64::MAX),
            18_446_744_073_709_551_616,
            1_313_205_263_863_309_263_863_309,
            u128::MAX,
        ] {
            let text = serde_json::to_string(&Wide::from(value)).expect("a written record");
            assert_eq!(
                crate::strictjson::decode_str::<Wide>(&text)
                    .expect("a record reads back")
                    .get(),
                value,
                "{text}"
            );
        }
    }

    #[test]
    fn only_the_canonical_spelling_is_read() {
        for refused in [
            "\"\"",
            "\"+1\"",
            "\"01\"",
            "\"-1\"",
            "\" 1\"",
            "\"1e3\"",
            "\"340282366920938463463374607431768211456\"",
            "1",
        ] {
            assert!(
                crate::strictjson::decode_str::<Wide>(refused).is_err(),
                "{refused} names no 128-bit integer by its one spelling"
            );
        }
    }
}
