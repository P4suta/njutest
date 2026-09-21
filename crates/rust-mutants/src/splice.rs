// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Byte splicing with an offset map.

use crate::span::{Span, SpanError};

/// One byte-range replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The half-open byte range this replaces. An empty span is an insertion at that offset.
    pub span: Span,
    /// The bytes the span is expected to cover.
    pub original: Vec<u8>,
    /// Written in their place. Empty deletes the span.
    pub replacement: Vec<u8>,
}

/// Why a set of splices could not be applied or a span could not be mapped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SpliceError {
    /// A splice's span is invalid or does not fit the source.
    #[error("splice {index} does not fit the source: {source}")]
    Span {
        /// The splice's position in the input.
        index: usize,
        /// The span failure.
        #[source]
        source: SpanError,
    },
    /// A splice's span does not cover the bytes it claims to.
    #[error("splice {index} at {span} covers {covered}, not {original}")]
    Mismatch {
        /// The splice's position in the input.
        index: usize,
        /// Its span.
        span: Span,
        /// What the span really covers, quoted and shortened.
        covered: String,
        /// What the splice claimed, quoted and shortened.
        original: String,
    },
    /// Two splices rewrite overlapping bytes.
    #[error("splice {second} at {second_span} overlaps splice {first} at {first_span}")]
    Overlap {
        /// The earlier splice in span order.
        first: usize,
        /// Its span.
        first_span: Span,
        /// The later splice.
        second: usize,
        /// Its span.
        second_span: Span,
    },
    /// The source or the spliced output is past the 32-bit offset limit.
    #[error("{what} would be {bytes} bytes, past the 32-bit offset limit")]
    TooLarge {
        /// `source` or `spliced source`.
        what: &'static str,
        /// The length.
        bytes: u128,
    },
    /// Internal offset arithmetic contradicted the invariants established by validation.
    #[error("validated splice-map invariant failed: {detail}")]
    Invariant {
        /// The invariant that failed closed.
        detail: &'static str,
    },
    /// A span to map starts or ends inside replaced bytes.
    #[error("span {span} {} inside replaced bytes", if *at_end { "ends" } else { "starts" })]
    Straddles {
        /// The span.
        span: Span,
        /// Whether the end (rather than the start) is the offending endpoint.
        at_end: bool,
    },
    /// A span to map is out of range for the mapped source.
    #[error("span {span} is out of range for {len} mapped bytes")]
    OutOfRange {
        /// The span.
        span: Span,
        /// The mapped source length.
        len: u32,
    },
}

/// Where one splice's bytes went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Edit {
    orig_start: u32,
    orig_end: u32,
    out_start: u32,
    out_end: u32,
}

type OrderedSplice<'a> = (usize, &'a Splice);

/// Translates byte offsets between a source and its spliced output, in both directions.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OffsetMap {
    edits: Vec<Edit>,
    src_len: u32,
    out_len: u32,
}

/// Performs every splice in one left-to-right pass and reports how offsets moved.
///
/// # Errors
/// Returns the first splice that does not fit, does not cover what it claims,
/// or overlaps another, or an output past the offset limit.
pub fn apply(src: &[u8], splices: &[Splice]) -> Result<(Vec<u8>, OffsetMap), SpliceError> {
    let (src_len, order) = validate(src, splices)?;
    let mut grown = u128::from(src_len);
    for splice in splices {
        let added = exact_size(splice.replacement.len())?;
        grown = grown
            .checked_add(added)
            .and_then(|sum| sum.checked_sub(u128::from(splice.span.len())))
            .ok_or(SpliceError::Invariant {
                detail: "replacement length arithmetic is exact",
            })?;
    }
    if grown > u128::from(u32::MAX) {
        return Err(SpliceError::TooLarge {
            what: "spliced source",
            bytes: grown,
        });
    }

    let capacity = usize::try_from(grown).map_err(|_overflow| SpliceError::TooLarge {
        what: "spliced source for this platform",
        bytes: grown,
    })?;
    let mut out: Vec<u8> = Vec::with_capacity(capacity);
    let mut edits = Vec::with_capacity(order.len());
    let mut cursor = 0usize;
    for (_index, splice) in order {
        let start =
            usize::try_from(splice.span.start).map_err(|_overflow| SpliceError::Invariant {
                detail: "a validated source offset fits usize",
            })?;
        let unchanged = src.get(cursor..start).ok_or(SpliceError::Invariant {
            detail: "validated splices preserve a forward source cursor",
        })?;
        out.extend_from_slice(unchanged);
        let out_start = output_offset(out.len())?;
        out.extend_from_slice(&splice.replacement);
        edits.push(Edit {
            orig_start: splice.span.start,
            orig_end: splice.span.end,
            out_start,
            out_end: output_offset(out.len())?,
        });
        cursor = usize::try_from(splice.span.end).map_err(|_overflow| SpliceError::Invariant {
            detail: "a validated source offset fits usize",
        })?;
    }
    let tail = src.get(cursor..).ok_or(SpliceError::Invariant {
        detail: "the final validated source cursor is in range",
    })?;
    out.extend_from_slice(tail);
    let out_len = output_offset(out.len())?;
    Ok((
        out,
        OffsetMap {
            edits,
            src_len,
            out_len,
        },
    ))
}

fn output_offset(len: usize) -> Result<u32, SpliceError> {
    let bytes = exact_size(len)?;
    u32::try_from(len).map_err(|_overflow| SpliceError::TooLarge {
        what: "spliced source",
        bytes,
    })
}

fn exact_size(len: usize) -> Result<u128, SpliceError> {
    u128::try_from(len).map_err(|_overflow| SpliceError::Invariant {
        detail: "usize converts losslessly to u128",
    })
}

/// Checks every splice against `src` and returns the source length with the indices of the splices in application order.
fn validate<'a>(
    src: &[u8],
    splices: &'a [Splice],
) -> Result<(u32, Vec<OrderedSplice<'a>>), SpliceError> {
    let source_bytes = exact_size(src.len())?;
    let src_len = u32::try_from(src.len()).map_err(|_overflow| SpliceError::TooLarge {
        what: "source",
        bytes: source_bytes,
    })?;
    for (index, splice) in splices.iter().enumerate() {
        let covered = splice
            .span
            .slice(src)
            .map_err(|source| SpliceError::Span { index, source })?;
        if covered != splice.original.as_slice() {
            return Err(SpliceError::Mismatch {
                index,
                span: splice.span,
                covered: quote_bytes(covered),
                original: quote_bytes(&splice.original),
            });
        }
    }
    let mut order: Vec<OrderedSplice<'_>> = splices.iter().enumerate().collect();
    order.sort_by_key(|(_index, splice)| splice.span);

    let mut reach = 0u32;
    let mut reaching: Option<(usize, Span)> = None;
    let mut previous: Option<(usize, Span)> = None;
    for &(index, splice) in &order {
        let current = splice.span;
        if let Some((previous_index, previous_span)) = previous {
            if current == previous_span {
                return Err(SpliceError::Overlap {
                    first: previous_index,
                    first_span: previous_span,
                    second: index,
                    second_span: current,
                });
            }
            if current.start < reach
                && let Some((first, first_span)) = reaching
            {
                return Err(SpliceError::Overlap {
                    first,
                    first_span,
                    second: index,
                    second_span: current,
                });
            }
        }
        if current.end >= reach {
            reach = current.end;
            reaching = Some((index, current));
        }
        previous = Some((index, current));
    }
    Ok((src_len, order))
}

/// Renders a byte range for a diagnostic, shortened so that a mismatch on a long span stays one line.
fn quote_bytes(bytes: &[u8]) -> String {
    const LIMIT: usize = 48;
    let shown = bytes.get(..LIMIT).unwrap_or(bytes);
    let mut quoted = String::from("\"");
    for &byte in shown {
        match byte {
            b'"' => quoted.push_str("\\\""),
            b'\\' => quoted.push_str("\\\\"),
            b'\n' => quoted.push_str("\\n"),
            b'\r' => quoted.push_str("\\r"),
            b'\t' => quoted.push_str("\\t"),
            0x20..=0x7e => quoted.push(char::from(byte)),
            other => {
                quoted.push_str("\\x");
                quoted.push_str(&hex::encode([other]));
            }
        }
    }
    quoted.push('"');
    if bytes.len() > LIMIT {
        quoted.push('\u{2026}');
        quoted.push_str(" (");
        quoted.push_str(&bytes.len().to_string());
        quoted.push_str(" bytes)");
    }
    quoted
}

/// `offset + plus - minus`, which callers have established is in range.
fn shift(offset: u32, plus: u32, minus: u32) -> Option<u32> {
    i64::from(offset)
        .checked_add(i64::from(plus))
        .and_then(|value| value.checked_sub(i64::from(minus)))
        .and_then(|value| match u32::try_from(value) {
            Ok(value) => Some(value),
            Err(_) => None,
        })
}

impl OffsetMap {
    /// The length of the source the map was built from.
    #[must_use]
    pub const fn src_len(&self) -> u32 {
        self.src_len
    }

    /// The length of the spliced output.
    #[must_use]
    pub const fn out_len(&self) -> u32 {
        self.out_len
    }

    /// The number of splices the map records.
    #[must_use]
    pub const fn splices(&self) -> usize {
        self.edits.len()
    }

    /// Translates an original offset into an output offset.
    #[must_use]
    pub fn to_output(&self, offset: u32) -> (u32, bool) {
        if offset > self.src_len {
            return (self.out_len, false);
        }
        let position = self.edits.partition_point(|edit| edit.orig_end <= offset);
        match self.edits.get(position) {
            None => match shift(offset, self.out_len, self.src_len) {
                Some(shifted) => (shifted, true),
                None => (self.out_len, false),
            },
            Some(edit) if edit.orig_start < offset => (edit.out_start, false),
            Some(edit) => match shift(offset, edit.out_start, edit.orig_start) {
                Some(shifted) => (shifted, true),
                None => (edit.out_start, false),
            },
        }
    }

    /// Translates an original endpoint to the output immediately before an
    /// insertion at that exact endpoint. Replacements ending there are still
    /// included. This is the right affinity for the exclusive end of a
    /// nonempty source span.
    fn to_output_before_insertion(&self, offset: u32) -> (u32, bool) {
        if offset > self.src_len {
            return (self.out_len, false);
        }
        let position = self.edits.partition_point(|edit| {
            edit.orig_end < offset || (edit.orig_end == offset && edit.orig_start < edit.orig_end)
        });
        match self.edits.get(position) {
            None => match shift(offset, self.out_len, self.src_len) {
                Some(shifted) => (shifted, true),
                None => (self.out_len, false),
            },
            Some(edit) if edit.orig_start < offset => (edit.out_start, false),
            Some(edit) => match shift(offset, edit.out_start, edit.orig_start) {
                Some(shifted) => (shifted, true),
                None => (edit.out_start, false),
            },
        }
    }

    /// Translates an output offset back into an original offset: the inverse of [`OffsetMap::to_output`] wherever an inverse exists, with an offset strictly inside a replacement answering with the start of the range it replaced.
    #[must_use]
    pub fn to_original(&self, offset: u32) -> (u32, bool) {
        if offset > self.out_len {
            return (self.src_len, false);
        }
        let position = self.edits.partition_point(|edit| edit.out_end <= offset);
        match self.edits.get(position) {
            None => match shift(offset, self.src_len, self.out_len) {
                Some(shifted) => (shifted, true),
                None => (self.src_len, false),
            },
            Some(edit) if edit.out_start < offset => (edit.orig_start, false),
            Some(edit) => match shift(offset, edit.orig_start, edit.out_start) {
                Some(shifted) => (shifted, true),
                None => (edit.orig_start, false),
            },
        }
    }

    /// Translates a whole span into output coordinates. Both endpoints must translate exactly; a span that encloses splices grows or shrinks by their net effect, which is the case the nested-rewrite path depends on.
    ///
    /// # Errors
    /// Returns a span that is invalid, out of range, or starts or ends inside
    /// replaced bytes.
    pub fn map_span(&self, span: Span) -> Result<Span, SpliceError> {
        span.validate()
            .map_err(|source| SpliceError::Span { index: 0, source })?;
        if span.end > self.src_len {
            return Err(SpliceError::OutOfRange {
                span,
                len: self.src_len,
            });
        }
        let (start, exact) = self.to_output(span.start);
        if !exact {
            return Err(SpliceError::Straddles {
                span,
                at_end: false,
            });
        }
        let (end, exact) = if span.is_empty() {
            self.to_output(span.end)
        } else {
            self.to_output_before_insertion(span.end)
        };
        if !exact {
            return Err(SpliceError::Straddles { span, at_end: true });
        }
        Ok(Span { start, end })
    }
}

/// The number of line breaks in `bytes`. Only `\n` is counted: a CRLF file has exactly one `\n` per line break just as an LF file does.
#[must_use]
#[expect(
    clippy::naive_bytecount,
    reason = "line counts of one splice; no dependency for a filter and count"
)]
pub fn count_lines(bytes: &[u8]) -> usize {
    bytes.iter().filter(|&&byte| byte == b'\n').count()
}

/// Whether applying these splices would leave every original byte on the line it started on.
#[must_use]
pub fn line_preserving(splices: &[Splice]) -> bool {
    splices
        .iter()
        .all(|splice| count_lines(&splice.original) == count_lines(&splice.replacement))
}
