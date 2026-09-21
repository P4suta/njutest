// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A JSON value reader that rejects duplicate object keys at every depth.

use std::collections::BTreeSet;

use serde::Deserialize as _;
use serde::de::{Error as _, MapAccess, SeqAccess, Visitor};

/// Decodes one complete JSON value only after proving every object key unique.
///
/// # Errors
///
/// Returns the ordinary JSON syntax error, a trailing-data error, a duplicate
/// key error, or the requested type's deserialization error.
pub fn decode_str<T>(text: &str) -> Result<T, serde_json::Error>
where
    T: serde::de::DeserializeOwned,
{
    from_str(text).and_then(serde_json::from_value)
}

/// Parses one complete JSON value without `serde_json`'s last-key-wins loss.
///
/// # Errors
/// Returns the ordinary JSON syntax error, a trailing-data error, or an error
/// naming the first duplicate object key at any depth.
pub fn from_str(text: &str) -> Result<serde_json::Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_str(text);
    let value = StrictValue::deserialize(&mut deserializer)?.0;
    deserializer.end()?;
    Ok(value)
}

/// Parses one complete UTF-8 JSON byte stream without a last-key-wins loss.
///
/// # Errors
/// Returns the ordinary JSON syntax error, a trailing-data error, or an error
/// naming the first duplicate object key at any depth.
pub fn from_slice(bytes: &[u8]) -> Result<serde_json::Value, serde_json::Error> {
    let mut deserializer = serde_json::Deserializer::from_slice(bytes);
    let value = StrictValue::deserialize(&mut deserializer)?.0;
    deserializer.end()?;
    Ok(value)
}

struct StrictValue(serde_json::Value);

impl<'de> serde::Deserialize<'de> for StrictValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictValue;

    fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("a JSON value with unique object keys")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(serde_json::Value::Bool(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(serde_json::Value::Number(value.into())))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(serde_json::Value::Number(value.into())))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        serde_json::Number::from_f64(value)
            .map(serde_json::Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("a JSON number must be finite"))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        self.visit_string(value.to_owned())
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(serde_json::Value::String(value)))
    }

    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(serde_json::Value::Null))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(serde_json::Value::Null))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        StrictValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element::<StrictValue>()? {
            values.push(value.0);
        }
        Ok(StrictValue(serde_json::Value::Array(values)))
    }

    fn visit_map<A>(self, mut object: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut names = BTreeSet::new();
        let mut values = serde_json::Map::new();
        while let Some(name) = object.next_key::<String>()? {
            if !names.insert(name.clone()) {
                return Err(A::Error::custom(format!(
                    "duplicate JSON object key {name:?}"
                )));
            }
            let value = object.next_value::<StrictValue>()?;
            values.insert(name, value.0);
        }
        Ok(StrictValue(serde_json::Value::Object(values)))
    }
}

#[cfg(test)]
mod tests {
    use njutest_devkit::result::{ResultState, result_state};

    #[test]
    fn duplicate_keys_are_rejected_recursively() {
        for text in [
            r#"{"schema":1,"schema":2}"#,
            r#"{"outer":{"outcome":"killed","outcome":"survived"}}"#,
            r#"{"models":[{"evidence":{"tool":"a","tool":"b"}}]}"#,
        ] {
            let parsed = super::from_str(text);
            assert_eq!(
                result_state(&parsed),
                ResultState::Refused,
                "duplicate keys produced an unexpected result: {parsed:?}"
            );
            let Err(error) = parsed else { continue };
            assert!(
                error.to_string().contains("duplicate JSON object key"),
                "{error}"
            );
        }
        let parsed = super::from_str(r#"{"outer":[null,true,1,"x"]}"#);
        assert_eq!(
            result_state(&parsed),
            ResultState::Returned,
            "closed JSON was refused: {parsed:?}"
        );
        let Ok(value) = parsed else { return };
        assert_eq!(
            value
                .get("outer")
                .and_then(serde_json::Value::as_array)
                .map(Vec::len),
            Some(4)
        );
    }
}
