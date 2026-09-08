// SPDX-FileCopyrightText: 2026 mjutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What each operator token becomes, and which spellings already are the default a return replacement would produce.

use syn::{BinOp, Expr, Lit, UnOp};

/// The rule that swaps `op`, with the token it writes in its place.
pub(super) const fn binary_swap(op: &BinOp) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match op {
        BinOp::Eq(_) => ("eq-to-neq", "==", "!="),
        BinOp::Ne(_) => ("neq-to-eq", "!=", "=="),
        BinOp::Lt(_) => ("lt-to-le", "<", "<="),
        BinOp::Le(_) => ("le-to-lt", "<=", "<"),
        BinOp::Gt(_) => ("gt-to-ge", ">", ">="),
        BinOp::Ge(_) => ("ge-to-gt", ">=", ">"),
        BinOp::And(_) => ("and-to-or", "&&", "||"),
        BinOp::Or(_) => ("or-to-and", "||", "&&"),
        BinOp::Add(_) => ("add-to-sub", "+", "-"),
        BinOp::Sub(_) => ("sub-to-add", "-", "+"),
        BinOp::Mul(_) => ("mul-to-div", "*", "/"),
        BinOp::Div(_) => ("div-to-mul", "/", "*"),
        BinOp::Rem(_) => ("rem-to-mul", "%", "*"),
        BinOp::BitAnd(_) => ("band-to-bor", "&", "|"),
        BinOp::BitOr(_) => ("bor-to-band", "|", "&"),
        BinOp::BitXor(_) => ("xor-to-band", "^", "&"),
        BinOp::Shl(_) => ("shl-to-shr", "<<", ">>"),
        BinOp::Shr(_) => ("shr-to-shl", ">>", "<<"),
        BinOp::AddAssign(_) => ("add-assign-to-sub-assign", "+=", "-="),
        BinOp::SubAssign(_) => ("sub-assign-to-add-assign", "-=", "+="),
        BinOp::MulAssign(_) => ("mul-assign-to-div-assign", "*=", "/="),
        BinOp::DivAssign(_) => ("div-assign-to-mul-assign", "/=", "*="),
        BinOp::RemAssign(_) => ("rem-assign-to-mul-assign", "%=", "*="),
        BinOp::BitAndAssign(_) => ("band-assign-to-bor-assign", "&=", "|="),
        BinOp::BitOrAssign(_) => ("bor-assign-to-band-assign", "|=", "&="),
        BinOp::BitXorAssign(_) => ("xor-assign-to-band-assign", "^=", "&="),
        BinOp::ShlAssign(_) => ("shl-assign-to-shr-assign", "<<=", ">>="),
        BinOp::ShrAssign(_) => ("shr-assign-to-shl-assign", ">>=", "<<="),
        _ => return None,
    })
}

/// Whether `op` is a compound assignment (`+=`, `<<=`, ...), which makes the expression a statement-shaped `()` and never a value to wrap.
pub(super) const fn is_compound_assignment(op: &BinOp) -> bool {
    matches!(
        op,
        BinOp::AddAssign(_)
            | BinOp::SubAssign(_)
            | BinOp::MulAssign(_)
            | BinOp::DivAssign(_)
            | BinOp::RemAssign(_)
            | BinOp::BitXorAssign(_)
            | BinOp::BitAndAssign(_)
            | BinOp::BitOrAssign(_)
            | BinOp::ShlAssign(_)
            | BinOp::ShrAssign(_)
    )
}

/// Whether `op` is `&&` or `||`.
pub(super) const fn is_connective(op: &BinOp) -> bool {
    matches!(op, BinOp::And(_) | BinOp::Or(_))
}

/// Whether the expression is `!x`.
pub(super) const fn is_not(op: &UnOp) -> bool {
    matches!(op, UnOp::Not(_))
}

/// The rule that removes `op`, with the name it answers to.
///
/// Both of these replace the whole expression with its operand, which is why
/// the walker reads them as one shape: `!x` becomes `x` and `-x` becomes `x`.
pub(super) const fn unary_removal(op: &UnOp) -> Option<&'static str> {
    match op {
        UnOp::Not(_) => Some("remove-not"),
        UnOp::Neg(_) => Some("remove-unary-minus"),
        _ => None,
    }
}

/// The rule that swaps a method whose name says the opposite of another the same receiver has, with the name it writes in its place.
///
/// The identifier is the whole of the edit: nothing here looks at a type, so a
/// receiver that has no such method is a mutation the compiler refuses, which
/// is where the engine settles acceptance
/// ([ADR 0008](../../../../docs/adr/0008-compiler-validated-acceptance-and-the-type-witness-pass.md)).
pub(super) fn method_swap(name: &str) -> Option<(&'static str, &'static str)> {
    Some(match name {
        "is_some" => ("is-some-to-is-none", "is_none"),
        "is_none" => ("is-none-to-is-some", "is_some"),
        "is_ok" => ("is-ok-to-is-err", "is_err"),
        "is_err" => ("is-err-to-is-ok", "is_ok"),
        "max" => ("max-to-min", "min"),
        "min" => ("min-to-max", "max"),
        "all" => ("all-to-any", "any"),
        "any" => ("any-to-all", "all"),
        "first" => ("first-to-last", "last"),
        "last" => ("last-to-first", "first"),
        "skip" => ("skip-to-take", "take"),
        "take" => ("take-to-skip", "skip"),
        "sum" => ("sum-to-product", "product"),
        "product" => ("product-to-sum", "sum"),
        _ => return None,
    })
}

/// Whether a method's name says it answers a question, so that asking the opposite question is a mutation.
///
/// The four `Option` and `Result` predicates are left out: a swap already
/// asks the opposite of each of them, and two rules writing the same question
/// at one span is one mutation reported twice.
pub(super) fn bool_method(name: &str) -> bool {
    if matches!(name, "is_some" | "is_none" | "is_ok" | "is_err") {
        return false;
    }
    name.starts_with("is_")
        || name.starts_with("has_")
        || matches!(
            name,
            "contains" | "contains_key" | "starts_with" | "ends_with"
        )
}

/// The literal one more or one less than `lit`, written in the radix and with the suffix it was written with.
///
/// Rust spells no negative literal — `-1` is a unary minus on `1` — so zero
/// has no predecessor to write, and a suffix that names a type bounds what
/// the literal may become. What the syntax cannot spell is not offered, which
/// is a mutation the compiler would have refused.
#[must_use]
pub fn respell_int(lit: &syn::LitInt, delta: i32) -> Option<String> {
    let raw = lit.token().to_string();
    let suffix = lit.suffix();
    let digits = raw.strip_suffix(suffix).unwrap_or(&raw).replace('_', "");
    let (prefix, radix) = match digits.get(..2) {
        Some("0x" | "0X") => ("0x", 16),
        Some("0o" | "0O") => ("0o", 8),
        Some("0b" | "0B") => ("0b", 2),
        _ => ("", 10),
    };
    let body = digits.get(prefix.len()..)?;
    let value = u128::from_str_radix(body, radix).ok()?;
    let moved = if delta < 0 {
        value.checked_sub(1)?
    } else {
        value.checked_add(1)?
    };
    if moved > ceiling(suffix) {
        return None;
    }
    let written = match radix {
        16 => format!("{moved:x}"),
        8 => format!("{moved:o}"),
        2 => format!("{moved:b}"),
        _ => format!("{moved}"),
    };
    Some(format!("{prefix}{written}{suffix}"))
}

/// The largest value a suffix says the literal may hold. An unsuffixed literal is bounded by nothing the syntax knows, and `usize` and `isize` are read as the sixty-four bit ones the compiler will settle.
fn ceiling(suffix: &str) -> u128 {
    match suffix {
        "u8" => u128::from(u8::MAX),
        "u16" => u128::from(u16::MAX),
        "u32" => u128::from(u32::MAX),
        "u64" | "usize" => u128::from(u64::MAX),
        "i8" => i8::MAX.unsigned_abs().into(),
        "i16" => i16::MAX.unsigned_abs().into(),
        "i32" => i32::MAX.unsigned_abs().into(),
        "i64" | "isize" => i64::MAX.unsigned_abs().into(),
        "i128" => i128::MAX.unsigned_abs(),
        _ => u128::MAX,
    }
}

/// The `else` an `if` chain ends with, when the chain ends with a block rather than another `if`.
pub(super) fn terminal_else(expr: &Expr) -> Option<(&syn::Block, &syn::Block)> {
    let Expr::If(one) = expr else {
        return None;
    };
    let (_, otherwise) = one.else_branch.as_ref()?;
    match otherwise.as_ref() {
        Expr::Block(block) => Some((&one.then_branch, &block.block)),
        nested @ Expr::If(_) => terminal_else(nested),
        _ => None,
    }
}

/// The last path segment of `expr` when it is a bare path.
fn last_segment(expr: &Expr) -> Option<String> {
    match expr {
        Expr::Path(path) => path.path.segments.last().map(|s| s.ident.to_string()),
        _ => None,
    }
}

/// A call `name(arg)` with exactly one argument, when `func` ends in `name`.
fn unary_call<'e>(expr: &'e Expr, name: &str) -> Option<&'e Expr> {
    let Expr::Call(call) = expr else {
        return None;
    };
    if call.args.len() != 1 || last_segment(&call.func).as_deref() != Some(name) {
        return None;
    }
    call.args.first()
}

/// Whether `expr` is spelled as the value `Default::default()` would produce, as far as syntax can tell: `0`, `0.0`, `false`, `""`, `()`, `None`, `[]`, `&[]`, `vec![]`, `Default::default()`, `T::default()`, and a zero-argument `T::new()`. A return replacement that would write the same value again is not a mutation, so these produce no candidate. The list is necessarily incomplete; what it misses is an equivalent mutant that survives, never a missed defect.
///
/// A borrow counts only in front of an empty array. `<&[T]>::default()` is an
/// empty slice, so `&[]` writes what the replacement would; nothing else the
/// standard library implements `Default` for behind a reference is spelled
/// this way, and unwrapping every borrow would refuse mutations a test can
/// notice.
pub(super) fn is_default_spelling(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(paren) => is_default_spelling(&paren.expr),
        Expr::Group(group) => is_default_spelling(&group.expr),
        Expr::Reference(borrow) => {
            matches!(borrow.expr.as_ref(), Expr::Array(array) if array.elems.is_empty())
        }
        Expr::Lit(lit) => match &lit.lit {
            Lit::Int(int) => int.base10_digits() == "0",
            Lit::Float(float) => float
                .base10_digits()
                .bytes()
                .all(|b| b == b'0' || b == b'.' || b == b'_'),
            Lit::Bool(b) => !b.value,
            Lit::Str(s) => s.value().is_empty(),
            Lit::ByteStr(s) => s.value().is_empty(),
            _ => false,
        },
        Expr::Tuple(tuple) => tuple.elems.is_empty(),
        Expr::Array(array) => array.elems.is_empty(),
        Expr::Path(_) => last_segment(expr).as_deref() == Some("None"),
        Expr::Call(call) => {
            call.args.is_empty()
                && matches!(last_segment(&call.func).as_deref(), Some("default" | "new"))
        }
        Expr::Macro(mac) => mac.mac.path.is_ident("vec") && mac.mac.tokens.is_empty(),
        _ => false,
    }
}

/// Whether `expr` is `Ok(<default>)`.
pub(super) fn is_ok_default(expr: &Expr) -> bool {
    unary_call(expr, "Ok").is_some_and(is_default_spelling)
}

/// Whether `expr` is `Err(<default>)`.
pub(super) fn is_err_default(expr: &Expr) -> bool {
    unary_call(expr, "Err").is_some_and(is_default_spelling)
}

/// Whether `expr` is `Some(<default>)`.
pub(super) fn is_some_default(expr: &Expr) -> bool {
    unary_call(expr, "Some").is_some_and(is_default_spelling)
}

/// Whether `expr` is the literal `true`.
pub(super) fn is_true_literal(expr: &Expr) -> bool {
    match expr {
        Expr::Paren(paren) => is_true_literal(&paren.expr),
        Expr::Lit(lit) => matches!(&lit.lit, Lit::Bool(b) if b.value),
        _ => false,
    }
}

/// Whether a condition holds a `let` anywhere `&&` and parentheses can reach: an `if let`, a `while let`, or a let chain. Such a condition can neither be negated nor have its connective swapped.
pub(super) fn has_let(expr: &Expr) -> bool {
    match expr {
        Expr::Let(_) => true,
        Expr::Paren(paren) => has_let(&paren.expr),
        Expr::Group(group) => has_let(&group.expr),
        Expr::Binary(binary) if is_connective(&binary.op) => {
            has_let(&binary.left) || has_let(&binary.right)
        }
        _ => false,
    }
}

/// How many of an assertion macro's leading arguments are expressions the tests are about.
///
/// The allowlist is fixed here rather than configured. What a macro does with
/// its tokens is the macro's business, and a guard spliced into an invocation
/// the engine does not understand is a guess: these six expand their leading
/// arguments as expressions, evaluate them, and compare or test them, which is
/// exactly what a mutation of one of them is a question about. `panic!`,
/// `unreachable!`, `write!` and `format!` are not here — their arguments are a
/// message and a format string, and mutating those asks nothing about the
/// program.
pub(super) fn assertion_arity(path: &syn::Path) -> Option<usize> {
    if path.segments.len() != 1 {
        return None;
    }
    Some(match path.segments.first()?.ident.to_string().as_str() {
        "assert" | "debug_assert" | "matches" => 1,
        "assert_eq" | "assert_ne" | "debug_assert_eq" | "debug_assert_ne" => 2,
        _ => return None,
    })
}

/// Whether the macro's leading arguments are conditions rather than values.
pub(super) fn assertion_is_condition(path: &syn::Path) -> bool {
    matches!(
        path.segments
            .first()
            .map(|one| one.ident.to_string())
            .as_deref(),
        Some("assert" | "debug_assert")
    )
}

/// The macro's arguments, split at the commas between them.
///
/// Only a comma at depth zero separates arguments: one inside `f(a, b)` or
/// `[a, b]` belongs to the argument it is in, and `proc_macro2` has already
/// grouped those for us.
pub(super) fn arguments(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::TokenStream> {
    let mut split: Vec<proc_macro2::TokenStream> = Vec::new();
    let mut current: Vec<proc_macro2::TokenTree> = Vec::new();
    for tree in tokens.clone() {
        match &tree {
            proc_macro2::TokenTree::Punct(punct)
                if punct.as_char() == ',' && punct.spacing() == proc_macro2::Spacing::Alone =>
            {
                split.push(std::mem::take(&mut current).into_iter().collect());
            }
            _ => current.push(tree),
        }
    }
    if !current.is_empty() {
        split.push(current.into_iter().collect());
    }
    split
}
