// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Assertions shared by the documentation ledgers.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

/// A documentation ledger that cannot be read or disagrees with the value it describes.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// The marker naming a table was absent.
    #[error("the page has no table beginning {marker}")]
    MissingTable {
        /// The exact header the ledger sought.
        marker: String,
    },
    /// A byte boundary before the marker could not be read as text.
    #[error("the page breaks between characters before {marker}")]
    BrokenBoundary {
        /// The table header after the unreadable boundary.
        marker: String,
    },
    /// The prose count immediately above a table disagreed with the derived count.
    #[error(
        "the paragraph above the table counts its rows, and this one does not say {said:?}. A reader who takes the number rather than counting the table is told something nothing held: {paragraph:?}"
    )]
    CountMismatch {
        /// The phrase the paragraph must contain.
        said: String,
        /// The exact paragraph that was checked.
        paragraph: String,
    },
    /// A ledger count has no deliberately reviewed English spelling.
    #[error("no documentation ledger spells {many} yet")]
    UnspelledCount {
        /// The count whose wording must be added explicitly.
        many: usize,
    },
    /// The schema could not be read as JSON.
    #[error("the schema is not JSON")]
    SchemaJson(#[source] serde_json::Error),
    /// The schema closes a set the ledger below does not name, or names one the schema no longer closes.
    #[error(
        "a closed set on the wire is one a reader writes a `match` over, and the schema is the only place some of them are written down. These pointers are in the schema and not in the ledger: {unheld:?}; these are in the ledger and not in the schema: {gone:?}"
    )]
    SchemaSetsDiffer {
        /// Pointers the schema closes that no ledger row names.
        unheld: Vec<String>,
        /// Pointers the ledger names that the schema no longer closes.
        gone: Vec<String>,
    },
    /// A set the schema closes holds names the ledger does not, or the other way round.
    #[error(
        "{pointer}: the schema admits {schema:?} and this release produces {rust:?}. A name the schema admits and nothing emits is a name a consumer writes a branch for and never reaches; one it refuses and a run emits fails every run that produces it"
    )]
    SchemaSetDiffers {
        /// The pointer of the set that differs.
        pointer: String,
        /// What the schema admits.
        schema: Vec<String>,
        /// What the ledger says this release produces.
        rust: Vec<String>,
    },
    /// A trace table has no Markdown divider.
    #[error("the trace field table {marker} has no divider")]
    MissingDivider {
        /// The header whose divider is absent.
        marker: String,
    },
    /// A trace table divider has a shape other than three plain dash cells.
    #[error("the trace field table {marker} has a malformed divider: {divider:?}")]
    MalformedDivider {
        /// The table header.
        marker: String,
        /// The divider that was found.
        divider: String,
    },
    /// A trace row has the wrong number of cells.
    #[error("a trace field row must have Type, Fields, and Records cells: {line:?}")]
    MalformedRow {
        /// The row that was found.
        line: String,
    },
    /// A Markdown row is not bounded by pipes.
    #[error("a trace field table row is not pipe-delimited: {line:?}")]
    NotPipeDelimited {
        /// The row that was found.
        line: String,
    },
    /// A table name is not exactly one non-empty backticked token.
    #[error("a {what} must be one non-empty backticked name: {cell:?}")]
    BadName {
        /// What the cell was naming.
        what: String,
        /// The malformed cell.
        cell: String,
    },
    /// A trace type has an empty field list.
    #[error("trace type {type_name:?} lists no fields")]
    NoFields {
        /// The trace type.
        type_name: String,
    },
    /// One field occurs twice in one documented type.
    #[error("trace type {type_name:?} lists serialized field {field:?} twice")]
    DuplicateField {
        /// The trace type.
        type_name: String,
        /// The repeated field.
        field: String,
    },
    /// One trace type occurs in two rows.
    #[error("the trace field table lists type {type_name:?} twice")]
    DuplicateType {
        /// The repeated type.
        type_name: String,
    },
    /// A table has a header but no records.
    #[error("the trace field table {marker} has no rows")]
    NoRows {
        /// The empty table's header.
        marker: String,
    },
    /// A specimen could not be serialized.
    #[error("a trace specimen does not serialize: {0}")]
    TraceSerialize(#[source] serde_json::Error),
    /// Serialized JSON was invalid or repeated a key.
    #[error("a trace specimen is not unique-key JSON: {0}")]
    TraceJson(#[source] serde_json::Error),
    /// A specimen's top level was not an object.
    #[error("a trace specimen does not serialize as an object")]
    TraceNotObject,
    /// A specimen did not have a string type tag.
    #[error("a trace specimen has no string `type` tag")]
    TraceMissingType,
    /// A wrapped payload lacked its declared record.
    #[error("trace specimen {type_name:?} has no {record:?} record")]
    TraceMissingRecord {
        /// The specimen's type tag.
        type_name: String,
        /// The expected wrapper.
        record: String,
    },
    /// A wrapped payload also serialized sibling fields.
    #[error("trace specimen {type_name:?} has fields beside its {record:?} record: {fields:?}")]
    TraceFieldsBesideRecord {
        /// The specimen's type tag.
        type_name: String,
        /// The record wrapper.
        record: String,
        /// The unexpected siblings.
        fields: Vec<String>,
    },
    /// A declared record wrapper did not contain an object.
    #[error("trace specimen {type_name:?} has a {record:?} value that is not an object")]
    TraceRecordNotObject {
        /// The specimen's type tag.
        type_name: String,
        /// The wrapper whose value had the wrong shape.
        record: String,
    },
    /// The documented and serialized closed sets differ.
    #[error("{detail}")]
    TraceDrift {
        /// Every difference, already rendered as one deterministic report.
        detail: String,
    },
}

/// Holds the count stated in the paragraph immediately above a table to the
/// count derived from code.
///
/// # Errors
/// The table marker is absent, or its immediately preceding paragraph does
/// not state `many` `noun` in English.
///
pub fn table_count(text: &str, marker: &str, many: usize, noun: &str) -> Result<(), LedgerError> {
    let Some(table) = text.find(marker) else {
        return Err(LedgerError::MissingTable {
            marker: marker.to_owned(),
        });
    };
    let Some(lead) = text.get(..table) else {
        return Err(LedgerError::BrokenBoundary {
            marker: marker.to_owned(),
        });
    };
    let above = lead.trim_end();
    let paragraph = above.rsplit("\n\n").next().unwrap_or(above);
    let said = format!("{} {noun}", spelled(many)?);
    if paragraph.contains(&said) {
        return Ok(());
    }
    Err(LedgerError::CountMismatch {
        said,
        paragraph: paragraph.to_owned(),
    })
}

/// One closed specimen of a tagged trace payload and the key under which its
/// record is nested.
///
/// `record` is `None` only when the record's fields sit directly beside the
/// `type` tag. A ledger normally passes the wrapper chosen by its payload
/// variant (`exec`, `phase`, and so on), which also refuses an unexpected
/// top-level field rather than silently counting it as part of the record.
#[derive(Debug, Clone, Copy)]
pub struct TraceSpecimen<'a, T> {
    value: &'a T,
    record: Option<&'static str>,
}

/// A JSON value whose object reader refuses a repeated key rather than keeping
/// whichever value a map happened to see last.
#[derive(Debug)]
enum UniqueValue {
    Object(BTreeMap<String, Self>),
    String(String),
    Other,
}

impl<'de> Deserialize<'de> for UniqueValue {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(UniqueVisitor)
    }
}

/// Reads JSON recursively so duplicate keys at a flattened record boundary
/// are visible before it becomes a map.
struct UniqueVisitor;

impl<'de> serde::de::Visitor<'de> for UniqueVisitor {
    type Value = UniqueValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON value with no repeated object key")
    }

    fn visit_bool<E: serde::de::Error>(self, _value: bool) -> Result<Self::Value, E> {
        Ok(UniqueValue::Other)
    }

    fn visit_i64<E: serde::de::Error>(self, _value: i64) -> Result<Self::Value, E> {
        Ok(UniqueValue::Other)
    }

    fn visit_u64<E: serde::de::Error>(self, _value: u64) -> Result<Self::Value, E> {
        Ok(UniqueValue::Other)
    }

    fn visit_f64<E: serde::de::Error>(self, _value: f64) -> Result<Self::Value, E> {
        Ok(UniqueValue::Other)
    }

    fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
        Ok(UniqueValue::String(value.to_owned()))
    }

    fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
        Ok(UniqueValue::String(value))
    }

    fn visit_none<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue::Other)
    }

    fn visit_some<D: serde::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        UniqueValue::deserialize(deserializer)
    }

    fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
        Ok(UniqueValue::Other)
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(
        self,
        mut sequence: A,
    ) -> Result<Self::Value, A::Error> {
        while sequence.next_element::<UniqueValue>()?.is_some() {}
        Ok(UniqueValue::Other)
    }

    fn visit_map<A: serde::de::MapAccess<'de>>(
        self,
        mut object: A,
    ) -> Result<Self::Value, A::Error> {
        let mut unique = BTreeMap::new();
        while let Some(key) = object.next_key::<String>()? {
            match unique.entry(key) {
                std::collections::btree_map::Entry::Occupied(repeated) => {
                    return Err(serde::de::Error::custom(format!(
                        "serialized object repeats field {:?}",
                        repeated.key()
                    )));
                }
                std::collections::btree_map::Entry::Vacant(field) => {
                    let value = object.next_value::<UniqueValue>()?;
                    field.insert(value);
                }
            }
        }
        Ok(UniqueValue::Object(unique))
    }
}

impl<'a, T> TraceSpecimen<'a, T> {
    /// Describes one serialized payload.
    #[must_use]
    pub const fn new(value: &'a T, record: Option<&'static str>) -> Self {
        Self { value, record }
    }
}

/// Holds a documentation table's field list to the union of fields actually
/// serialized by closed payload specimens.
///
/// The table starts at `marker` and has exactly three columns: `Type`,
/// `Fields`, and prose. Every type and field is one backticked name; fields are
/// comma-separated. More than one specimen may carry a type so enum and
/// flattened variants can contribute all of their fields. Optional fields
/// therefore belong in specimens with non-empty values.
///
/// # Errors
/// The table is absent or malformed; a type or field is repeated; a specimen
/// is not a tagged object with the stated record wrapper; or the documented
/// and serialized type/field sets differ in either direction.
pub fn trace_field_ledger<T: Serialize>(
    text: &str,
    marker: &str,
    specimens: &[TraceSpecimen<'_, T>],
) -> Result<(), LedgerError> {
    let documented = documented_trace_fields(text, marker)?;
    let mut serialized: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for specimen in specimens {
        let (type_name, fields) = serialized_trace_fields(specimen)?;
        serialized.entry(type_name).or_default().extend(fields);
    }
    compare_trace_fields(&documented, &serialized)
}

/// Reads the deliberately rigid, machine-readable half of a trace table.
fn documented_trace_fields(
    text: &str,
    marker: &str,
) -> Result<BTreeMap<String, BTreeSet<String>>, LedgerError> {
    let mut lines = text.lines().skip_while(|line| line.trim() != marker);
    if lines.next().is_none() {
        return Err(LedgerError::MissingTable {
            marker: marker.to_owned(),
        });
    }
    let Some(divider) = lines.next() else {
        return Err(LedgerError::MissingDivider {
            marker: marker.to_owned(),
        });
    };
    let divider_cells = table_cells(divider)?;
    if divider_cells.len() != 3
        || divider_cells
            .iter()
            .any(|cell| cell.trim().len() < 3 || !cell.trim().bytes().all(|byte| byte == b'-'))
    {
        return Err(LedgerError::MalformedDivider {
            marker: marker.to_owned(),
            divider: divider.to_owned(),
        });
    }

    let mut documented = BTreeMap::new();
    for line in lines.take_while(|line| line.trim_start().starts_with('|')) {
        let cells = table_cells(line)?;
        let [type_cell, field_cell, description_cell] = cells.as_slice() else {
            return Err(LedgerError::MalformedRow {
                line: line.to_owned(),
            });
        };
        if description_cell.trim().is_empty() {
            return Err(LedgerError::MalformedRow {
                line: line.to_owned(),
            });
        }
        let type_name = backticked(type_cell, "trace type")?.to_owned();
        let fields = field_cell
            .split(',')
            .map(|field| backticked(field, "serialized field").map(str::to_owned))
            .collect::<Result<Vec<String>, LedgerError>>()?;
        if fields.is_empty() {
            return Err(LedgerError::NoFields { type_name });
        }
        let mut unique = BTreeSet::new();
        for field in fields {
            if !unique.insert(field.clone()) {
                return Err(LedgerError::DuplicateField { type_name, field });
            }
        }
        if documented.insert(type_name.clone(), unique).is_some() {
            return Err(LedgerError::DuplicateType { type_name });
        }
    }
    if documented.is_empty() {
        return Err(LedgerError::NoRows {
            marker: marker.to_owned(),
        });
    }
    Ok(documented)
}

/// The cells of one pipe-delimited Markdown table row.
fn table_cells(line: &str) -> Result<Vec<&str>, LedgerError> {
    let trimmed = line.trim();
    let Some(inner) = trimmed
        .strip_prefix('|')
        .and_then(|without_start| without_start.strip_suffix('|'))
    else {
        return Err(LedgerError::NotPipeDelimited {
            line: line.to_owned(),
        });
    };
    Ok(inner.split('|').map(str::trim).collect())
}

/// One cell that consists of exactly one backticked name.
fn backticked<'a>(cell: &'a str, what: &str) -> Result<&'a str, LedgerError> {
    let cell = cell.trim();
    let Some(name) = cell
        .strip_prefix('`')
        .and_then(|without_start| without_start.strip_suffix('`'))
    else {
        return Err(LedgerError::BadName {
            what: what.to_owned(),
            cell: cell.to_owned(),
        });
    };
    if name.is_empty() || name.contains('`') {
        return Err(LedgerError::BadName {
            what: what.to_owned(),
            cell: cell.to_owned(),
        });
    }
    Ok(name)
}

/// Reads the type tag and record fields from one serialized specimen.
fn serialized_trace_fields<T: Serialize>(
    specimen: &TraceSpecimen<'_, T>,
) -> Result<(String, BTreeSet<String>), LedgerError> {
    let bytes = serde_json::to_vec(specimen.value).map_err(LedgerError::TraceSerialize)?;
    let value: UniqueValue =
        crate::strictjson::decode_slice(&bytes).map_err(LedgerError::TraceJson)?;
    let UniqueValue::Object(mut payload) = value else {
        return Err(LedgerError::TraceNotObject);
    };
    let Some(UniqueValue::String(type_name)) = payload.remove("type") else {
        return Err(LedgerError::TraceMissingType);
    };
    let fields = if let Some(record) = specimen.record {
        let Some(value) = payload.remove(record) else {
            return Err(LedgerError::TraceMissingRecord {
                type_name,
                record: record.to_owned(),
            });
        };
        if !payload.is_empty() {
            return Err(LedgerError::TraceFieldsBesideRecord {
                type_name,
                record: record.to_owned(),
                fields: payload.keys().cloned().collect(),
            });
        }
        let UniqueValue::Object(record) = value else {
            return Err(LedgerError::TraceRecordNotObject {
                type_name,
                record: record.to_owned(),
            });
        };
        record.keys().cloned().collect()
    } else {
        payload.keys().cloned().collect()
    };
    Ok((type_name, fields))
}

/// Compares types and each type's field set in both directions.
fn compare_trace_fields(
    documented: &BTreeMap<String, BTreeSet<String>>,
    serialized: &BTreeMap<String, BTreeSet<String>>,
) -> Result<(), LedgerError> {
    let documented_types: BTreeSet<&String> = documented.keys().collect();
    let serialized_types: BTreeSet<&String> = serialized.keys().collect();
    let mut problems = Vec::new();
    let missing_types: Vec<&&String> = serialized_types.difference(&documented_types).collect();
    if !missing_types.is_empty() {
        problems.push(format!(
            "the trace field table is missing serialized types {missing_types:?}"
        ));
    }
    let extra_types: Vec<&&String> = documented_types.difference(&serialized_types).collect();
    if !extra_types.is_empty() {
        problems.push(format!(
            "the trace field table names types no specimen serializes {extra_types:?}"
        ));
    }
    for (type_name, actual) in serialized {
        let Some(written) = documented.get(type_name) else {
            continue;
        };
        let missing: Vec<&String> = actual.difference(written).collect();
        let extra: Vec<&String> = written.difference(actual).collect();
        if !missing.is_empty() || !extra.is_empty() {
            problems.push(format!(
                "trace type {type_name:?} fields differ: missing {missing:?}; extra {extra:?}"
            ));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(LedgerError::TraceDrift {
            detail: problems.join("\n"),
        })
    }
}

/// The intentionally small vocabulary used by documentation ledgers.
fn spelled(many: usize) -> Result<&'static str, LedgerError> {
    const WORDS: [&str; 21] = [
        "no",
        "one",
        "two",
        "three",
        "four",
        "five",
        "six",
        "seven",
        "eight",
        "nine",
        "ten",
        "eleven",
        "twelve",
        "thirteen",
        "fourteen",
        "fifteen",
        "sixteen",
        "seventeen",
        "eighteen",
        "nineteen",
        "twenty",
    ];
    let word = match WORDS.get(many) {
        Some(word) => *word,
        None => match many {
            51 => "fifty-one",
            61 => "sixty-one",
            63 => "sixty-three",
            69 => "sixty-nine",
            72 => "seventy-two",
            74 => "seventy-four",
            _ => return Err(LedgerError::UnspelledCount { many }),
        },
    };
    Ok(word)
}

/// Every closed set a JSON Schema declares, by the pointer that declares it.
///
/// # Errors
/// The schema is not JSON.
pub fn schema_enum_sets(schema: &str) -> Result<BTreeMap<String, Vec<String>>, LedgerError> {
    let document: serde_json::Value =
        crate::strictjson::decode_str(schema).map_err(LedgerError::SchemaJson)?;
    let mut found = BTreeMap::new();
    collect_enum_sets(&document, "", &mut found);
    let mut single = BTreeMap::new();
    collect_const_values(&document, "", &mut single);
    for (at, only) in single {
        if beside_a_vocabulary(&at, &found) {
            found.insert(at, only);
        }
    }
    Ok(found)
}

/// Whether a one-value set stands where a sibling branch of the same choice spells a whole one.
///
/// A schema writes a discriminator, a version, and a boolean as `const` too,
/// and none of those is a vocabulary a consumer branches over.
/// What is one is the branch of a `oneOf` that admits a single name where its siblings admit several: the third arm of a three-way split is a closed set with one member, and reading only the arrays left it held by nothing.
fn beside_a_vocabulary(at: &str, sets: &BTreeMap<String, Vec<String>>) -> bool {
    let Some((before, rest)) = at.rsplit_once("/oneOf/") else {
        return false;
    };
    let Some(after) = rest.split_once('/').map(|(_branch, tail)| tail) else {
        return false;
    };
    sets.keys().any(|other| {
        other
            .rsplit_once("/oneOf/")
            .and_then(|(theirs, tail)| Some((theirs, tail.split_once('/')?.1)))
            .is_some_and(|(theirs, tail)| theirs == before && tail == after)
    })
}

/// Every `const` under `node`, keyed by the JSON pointer that reaches it.
fn collect_const_values(
    node: &serde_json::Value,
    at: &str,
    into: &mut BTreeMap<String, Vec<String>>,
) {
    match node {
        serde_json::Value::Object(map) => {
            if let Some(only) = map.get("const") {
                into.insert(at.to_owned(), vec![named(only)]);
            }
            for (key, held) in map {
                collect_const_values(held, &format!("{at}/{key}"), into);
            }
        }
        serde_json::Value::Array(values) => {
            for (index, held) in values.iter().enumerate() {
                collect_const_values(held, &format!("{at}/{index}"), into);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

/// One admitted value, as the name a reader compares against.
fn named(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// Every `enum` array and `const` under `node`, keyed by the JSON pointer that reaches it.
///
/// A one-value set is spelled `const` rather than `enum`, and it is a closed set with one member: reading only the arrays left the third branch of a three-way split held by nothing.
fn collect_enum_sets(node: &serde_json::Value, at: &str, into: &mut BTreeMap<String, Vec<String>>) {
    match node {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::Array(values)) = map.get("enum") {
                into.insert(at.to_owned(), values.iter().map(named).collect());
            }
            for (key, held) in map {
                collect_enum_sets(held, &format!("{at}/{key}"), into);
            }
        }
        serde_json::Value::Array(values) => {
            for (index, held) in values.iter().enumerate() {
                collect_enum_sets(held, &format!("{at}/{index}"), into);
            }
        }
        serde_json::Value::Null
        | serde_json::Value::Bool(_)
        | serde_json::Value::Number(_)
        | serde_json::Value::String(_) => {}
    }
}

/// Holds every closed set a JSON Schema declares to the names this release produces.
///
/// The store boundary already refuses a report carrying a name the schema does not admit, so that direction fails in every run that produces one.
/// The other direction fails in no run at all: a name the schema admits and nothing emits is a branch a consumer writes and never reaches, and there were twenty-seven sets here that nothing on the Rust side was held to.
///
/// # Errors
/// The schema is not JSON, closes a set no row names, or disagrees with a row.
pub fn schema_enum_ledger(schema: &str, expected: &[(&str, &[&str])]) -> Result<(), LedgerError> {
    let declared = schema_enum_sets(schema)?;
    let held: BTreeMap<&str, &[&str]> = expected.iter().copied().collect();
    let unheld: Vec<String> = declared
        .keys()
        .filter(|pointer| !held.contains_key(pointer.as_str()))
        .cloned()
        .collect();
    let gone: Vec<String> = held
        .keys()
        .filter(|pointer| !declared.contains_key(**pointer))
        .map(|pointer| (*pointer).to_owned())
        .collect();
    if !unheld.is_empty() || !gone.is_empty() {
        return Err(LedgerError::SchemaSetsDiffer { unheld, gone });
    }
    for (pointer, names) in &declared {
        let Some(rust) = held.get(pointer.as_str()) else {
            continue;
        };
        let mut schema_names = names.clone();
        let mut rust_names: Vec<String> = rust.iter().map(|name| (*name).to_owned()).collect();
        schema_names.sort();
        rust_names.sort();
        if schema_names != rust_names {
            return Err(LedgerError::SchemaSetDiffers {
                pointer: pointer.clone(),
                schema: schema_names,
                rust: rust_names,
            });
        }
    }
    Ok(())
}
