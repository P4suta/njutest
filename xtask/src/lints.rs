// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Rust shapes this repository does not write.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use syn::parse::Parser as _;
use syn::visit::Visit;

const OWNED_TRAIT_OBJECT_REMEDY: &str = "use an enum for a closed set of implementations, or a \
    generic parameter for an open one; owning a vtable erases the set precisely where ownership \
    should make it explicit";
const TRAIT_OBJECT_ALIAS_REMEDY: &str = "spell `dyn Trait` at the site that borrows it, and use an \
    enum or generic parameter at a site that owns it; an alias lets another file hide the vtable \
    from the ownership gate";
const OWNED_POINTER_ALIAS_REMEDY: &str = "spell `Box`, `Rc`, or `Arc` at the ownership site; a \
    transparent alias lets another file own a trait object without the ownership gate seeing it";
const DERIVED_ENUM_DEFAULT_REMEDY: &str = "use a named constructor at a semantic call site. Only \
    the gate's narrow configuration and UI enum allowlist may implement `Default`; `#[default]` \
    lets declaration order invent a domain state, and a derive on an enclosing struct propagates \
    that invention without another line to review";
const SEMANTIC_DEFAULT_REMEDY: &str = "use a named constructor for execution, evidence, and wire \
    state. `Default` makes `..Default::default()` invent a domain conclusion without naming it, \
    and adding a variant does not make a manual implementation fail to compile. The gate keeps \
    only an exact allowlist of configuration, UI, and neutral container defaults";
const DEFAULT_DERIVE_ALIAS_REMEDY: &str = "import `Default` by its own name; renaming the derive \
    lets another file put an enum default behind a name the enum-default gate cannot recognise";
const DESERIALIZE_DERIVE_ALIAS_REMEDY: &str = "import `Deserialize` by its own name; a renamed \
    proc-macro re-export lets another file derive an input boundary whose strictness the syntax \
    gate cannot recognise";
const STRING_ERROR_REMEDY: &str = "return a typed error whose variants preserve what failed. \
    Rendering belongs at the output boundary; a String cannot be exhaustively handled, assigned a \
    stable code, or distinguished from another failure with the same words";
const UNIT_ERROR_REMEDY: &str = "return a closed error enum that preserves why the operation \
    failed. `Result<T, ()>` proves only that something happened and cannot drive an exhaustive \
    recovery policy, stable error code, or evidence record";
const STRING_ALIAS_REMEDY: &str = "use a domain newtype for text and a typed enum for errors; an \
    alias of `String` or `str` lets another file erase an error behind a name the gate cannot \
    resolve";
const RESULT_ALIAS_REMEDY: &str = "spell `Result` at the signature; a transparent alias lets \
    another file hide a textual error argument from the typed-error gate";
const DISCARDED_RESULT_REMEDY: &str = "propagate or record every iterator error; \
    `Result::ok` turns a failure into absence and `Result::err` turns success into absence; use an \
    exhaustive match even outside an iterator. `filter_map(Result::ok)` turns an unreadable subtree into an absent one and lets a gate prove \
    only what it could see. An `if let Ok` has the same hidden `Err` arm: use an exhaustive \
    `match`, `?`, or a `let ... else` whose failure branch states the policy. Result fallback \
    combinators are compiler-denied in favour of that match, and `Result::into_iter` may not turn \
    its error into an empty iterator";
const DROPPED_COMPUTATION_REMEDY: &str = "match, propagate, or bind the call's result to a name \
    that states the policy; `drop(fallible())` suppresses `must_use` precisely where the compiler \
    would otherwise require the failure to be handled. `drop(guard)` remains available for ending \
    a value's lifetime early, and must keep the standard name so another file cannot hide a call \
    from this rule";
const IGNORED_COMPUTATION_REMEDY: &str = "handle the computed value or give it a semantic name \
    and consume it explicitly. An underscore-prefixed binding suppresses both unused-variable and \
    must-use diagnostics, so `_written = fs::write(...)` is `drop(fs::write(...))` with a quieter \
    spelling; RAII guards use a named guard and an explicit `drop(guard)`";
const OPEN_DESERIALIZATION_REMEDY: &str = "make owned input exact: add \
    `#[serde(deny_unknown_fields)]` and do not use input-side `flatten`, `other`, `untagged`, \
    `default`, or `alias`; silently ignored, invented, or ambiguously matched input turns a \
    different document into one this version claims to understand. A foreign protocol may only \
    retain additions in the gate's exact private `external_fields` capture";
const OPAQUE_MACRO_SYNTAX_REMEDY: &str = "make the generated attribute shape literal and \
    parseable in the macro definition, and keep compiled source redirects inside the scanned \
    Rust universe. A derive, serde option, cfg_attr, include/path redirect, or procedural-macro \
    token stream assembled opaquely is code this source gate cannot prove closed, so it is \
    rejected rather than treated as harmless";
const UNIT_DOMAIN_CONVERSION_REMEDY: &str = "give the domain state a name and construct it by that \
    name; `From<()>` makes absence invent a semantic value, so adding a variant or reusing the \
    conversion silently chooses policy where only a unit value was supplied";
const MANUAL_VARIANT_LIST_REMEDY: &str = "derive `njutest_macros::AllVariants` on a closed \
    fieldless enum and use its compiler-generated `ALL`; a hand-maintained array still compiles \
    when a variant is added, which turns an exhaustive ledger into a partial one without an error";
const DIRECT_JSON_INPUT_REMEDY: &str = "decode through the crate's strictjson boundary, which \
    rejects duplicate object keys before converting the unique Value into a typed document. \
    serde_json's direct readers keep the last repeated key, so a production reader or test oracle \
    can silently prove a different document from the bytes it received";
const IMPLICIT_SCALAR_ERASURE_REMEDY: &str = "keep scalar and domain newtypes nominal: expose an \
    explicit `as_str`, `as_path`, or `into_inner` boundary instead of `Deref`, `AsRef`, `Borrow`, \
    `Into`, or representation-side `From`. Those blanket traits make generic lookup and coercion \
    silently erase the distinction the newtype was introduced to enforce";
const FABRICATED_OVERFLOW_REMEDY: &str = "use checked arithmetic and propagate a typed overflow \
    refusal at report, accounting, identity, cache, key, offset, and count boundaries. Saturation \
    and `unwrap_or(0/MAX)` manufacture a valid-looking value precisely when the real value no \
    longer fits; UI geometry may hide that policy only behind its separately named presentation \
    helper";
const WRAPPING_COUNTER_REMEDY: &str = "use checked arithmetic, or `Atomic*::fetch_update` with a \
    typed/sticky overflow refusal. `fetch_add`, `fetch_sub`, and `wrapping_*` turn exhaustion into \
    a plausible earlier counter value in release builds; a bounded presentation counter belongs \
    behind one named helper with that policy in its type";
const UNCHECKED_CAST_REMEDY: &str = "use `TryFrom`, an exact pointer type, or a documented FFI \
    constructor that can reject an unrepresentable value. `as` truncates integers and changes \
    pointer meaning without a failure branch, and host Clippy cannot inspect code behind another \
    target's cfg";
const UNOWNED_SPAWN_REMEDY: &str = "construct threads and child processes only inside a named \
    owner that must join, kill, or reap them on every path. Raw `thread::spawn`, \
    `Builder::spawn`, and `Command::spawn` make cleanup an optional convention; scoped work must \
    likewise pass through a typed scope helper whose lifetime proves the join";
const UNBOUNDED_CHANNEL_REMEDY: &str = "use a bounded `sync_channel` whose capacity and full or \
    disconnected policy are named at the construction boundary. `mpsc::channel` lets a stalled \
    consumer turn producer progress into unbounded memory growth";
const POISON_RECOVERY_REMEDY: &str = "propagate a typed sticky failure when a lock is poisoned. \
    `PoisonError::into_inner`, `get_ref`, `get_mut`, and `clear_poison` turn a panic-interrupted \
    invariant transition back into an apparently valid value";
const LOSSY_TEXT_REMEDY: &str = "preserve bytes and platform strings until the boundary can \
    either decode them exactly or return a typed refusal. `from_utf8_lossy` and \
    `to_string_lossy` collapse distinct inputs onto the same replacement character, so they \
    cannot participate in identity, evidence, protocol, path, test-oracle, or diagnostic \
    decisions; a human-only renderer escapes undecodable bytes instead of inventing text";
const FORGOTTEN_VALUE_REMEDY: &str = "let ownership run its destructor, or use a named owner \
    whose state machine makes an intentional transfer explicit. `mem::forget` and \
    `ManuallyDrop` turn cleanup and invariant restoration into an unenforced convention; using \
    either only behind `cfg(kani)` also proves a program with weaker ownership semantics than \
    the one that ships";
const TRI_STATE_BOOL_REMEDY: &str = "replace `Option<bool>` with a closed enum whose variants \
    name all three states. `None`, `Some(false)`, and `Some(true)` carry domain meaning that the \
    type leaves every caller to rediscover; aliases and renamed `Option` are rejected at their \
    declaration so another file cannot restore the ambiguity";
const ALLOW_ATTRIBUTE_REMEDY: &str = "use #[expect(…, reason = \"…\")], which the compiler \
    retires when the lint stops firing";
const BROAD_EXPECTATION_REMEDY: &str = "put the expectation on the exact expression it permits; \
    a module-wide `expect(dead_code)` or `expect(unsafe_code)` is fulfilled by one existing hit \
    and silently permits every later one";
const VACUOUS_CFG_REMEDY: &str = "remove a condition that is provably always true or always \
    false. `cfg(any())`, `cfg(not(all()))`, `cfg(all())`, and `cfg(not(any()))` can hide code \
    from every compiler or pretend an unconditional item was checked conditionally; name a real \
    target, feature, or test boundary instead";
const GLOB_IMPORT_REMEDY: &str = "name the imported items. Only a module explicitly named \
    `prelude` may export an intentionally open vocabulary; every other glob lets a dependency add \
    a name to this scope without changing this file";
const COMMENT_REMEDY: &str = "say it in the name, in the item's own documentation, or in the \
    message the assertion prints; a comment beside code is a second account of it that nothing \
    keeps true";
const UNBOUNDED_REMOVAL_REMEDY: &str = "use rust_mutants::reclaim, which stops at a budget and \
    hands back what refused and what it never reached; a directory something else is holding \
    takes minutes to refuse, and a loop over a few hundred of those runs for a day while saying \
    nothing";
const PERISHABLE_HANDLE_REMEDY: &str = "build it from a locator — path, item, rule — which holds \
    after the file has changed; a mutant identity is a function of the whole file, so the edit \
    that closes a survivor re-mints it and the command or record that names it stops naming \
    anything";
const LOOSE_LAYOUT_REMEDY: &str = "a layout written down here freezes it: the configuration cannot \
    name a directory somebody else has already decided, which is how a report directory stayed \
    unconfigurable while four commands read the wrong place. Ask the type that owns the layout \
    for the path, the way the code under test does, and let the default live in configuration";
const WILDCARD_OVER_OUR_OWN_REMEDY: &str = "a catch-all over a set this repository closes is the \
    arm that absorbs the next variant silently. Name the remaining variants so the compiler makes \
    whoever adds one decide where it goes. A catch-all over somebody else's open set is instead \
    the required handling";
const HAND_PAINTED_REMEDY: &str = "ask for what the thing is rather than for a colour: \
    `Style::Gap`, `Style::Command`, or `Style::Limitation`. One module turns a style into bytes; \
    everything else names a meaning";
const OPEN_AND_CLOSED_REMEDY: &str = "drop `#[non_exhaustive]`. A type that publishes its whole \
    list has promised to break callers when it grows, while the attribute promises not to. It \
    also disables `clippy::match_wildcard_for_single_variants`. Keep it only on an error whose \
    callers branch on no published exhaustive list";

macro_rules! declare_kinds {
    ($( $variant:ident => $label:literal),+ $(,)?) => {
        /// What kind of thing was found.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        pub enum Kind {
            $(
                #[doc = concat!("The prohibited `", $label, "` Rust shape.")]
                $variant,
            )+
        }

        impl Kind {
            /// Every kind, in the order a report lists them.
            pub const ALL: &'static [Self] = &[$(Self::$variant),+];

            /// What to write in a report.
            #[must_use]
            pub const fn label(self) -> &'static str {
                match self {
                    $(Self::$variant => $label),+
                }
            }
        }
    };
}

declare_kinds! {
    AllowAttribute => "allow-attribute",
    BroadExpectation => "broad-expectation",
    VacuousCfg => "vacuous-cfg",
    OwnedTraitObject => "owned-trait-object",
    TraitObjectAlias => "trait-object-alias",
    OwnedPointerAlias => "owned-pointer-alias",
    DerivedEnumDefault => "derived-enum-default",
    SemanticDefault => "semantic-default",
    DefaultDeriveAlias => "default-derive-alias",
    DeserializeDeriveAlias => "deserialize-derive-alias",
    UnitDomainConversion => "unit-domain-conversion",
    GlobImport => "glob-import",
    StringError => "string-error",
    UnitError => "unit-error",
    StringAlias => "string-alias",
    ResultAlias => "result-alias",
    DiscardedResult => "discarded-result",
    DroppedComputation => "dropped-computation",
    IgnoredComputation => "ignored-computation",
    OpenDeserialization => "open-deserialization",
    OpaqueMacroSyntax => "opaque-macro-syntax",
    Comment => "comment",
    UnboundedRemoval => "unbounded-removal",
    PerishableHandle => "perishable-handle",
    LooseLayout => "loose-layout",
    WildcardOverOurOwn => "wildcard-over-our-own",
    HandPainted => "hand-painted",
    ManualVariantList => "manual-variant-list",
    DirectJsonInput => "direct-json-input",
    ImplicitScalarErasure => "implicit-scalar-erasure",
    FabricatedOverflow => "fabricated-overflow",
    WrappingCounter => "wrapping-counter",
    UncheckedCast => "unchecked-cast",
    UnownedSpawn => "unowned-spawn",
    UnboundedChannel => "unbounded-channel",
    PoisonRecovery => "poison-recovery",
    LossyText => "lossy-text",
    ForgottenValue => "forgotten-value",
    TriStateBool => "tri-state-bool",
    OpenAndClosed => "open-and-closed",
}

impl Kind {
    /// Why it is refused, in the words a person needs to fix it.
    #[must_use]
    pub const fn remedy(self) -> &'static str {
        match self {
            Self::AllowAttribute => ALLOW_ATTRIBUTE_REMEDY,
            Self::BroadExpectation => BROAD_EXPECTATION_REMEDY,
            Self::VacuousCfg => VACUOUS_CFG_REMEDY,
            Self::OwnedTraitObject => OWNED_TRAIT_OBJECT_REMEDY,
            Self::TraitObjectAlias => TRAIT_OBJECT_ALIAS_REMEDY,
            Self::OwnedPointerAlias => OWNED_POINTER_ALIAS_REMEDY,
            Self::DerivedEnumDefault => DERIVED_ENUM_DEFAULT_REMEDY,
            Self::SemanticDefault => SEMANTIC_DEFAULT_REMEDY,
            Self::DefaultDeriveAlias => DEFAULT_DERIVE_ALIAS_REMEDY,
            Self::DeserializeDeriveAlias => DESERIALIZE_DERIVE_ALIAS_REMEDY,
            Self::UnitDomainConversion => UNIT_DOMAIN_CONVERSION_REMEDY,
            Self::GlobImport => GLOB_IMPORT_REMEDY,
            Self::StringError => STRING_ERROR_REMEDY,
            Self::UnitError => UNIT_ERROR_REMEDY,
            Self::StringAlias => STRING_ALIAS_REMEDY,
            Self::ResultAlias => RESULT_ALIAS_REMEDY,
            Self::DiscardedResult => DISCARDED_RESULT_REMEDY,
            Self::DroppedComputation => DROPPED_COMPUTATION_REMEDY,
            Self::IgnoredComputation => IGNORED_COMPUTATION_REMEDY,
            Self::OpenDeserialization => OPEN_DESERIALIZATION_REMEDY,
            Self::OpaqueMacroSyntax => OPAQUE_MACRO_SYNTAX_REMEDY,
            Self::Comment => COMMENT_REMEDY,
            Self::UnboundedRemoval => UNBOUNDED_REMOVAL_REMEDY,
            Self::PerishableHandle => PERISHABLE_HANDLE_REMEDY,
            Self::LooseLayout => LOOSE_LAYOUT_REMEDY,
            Self::WildcardOverOurOwn => WILDCARD_OVER_OUR_OWN_REMEDY,
            Self::HandPainted => HAND_PAINTED_REMEDY,
            Self::ManualVariantList => MANUAL_VARIANT_LIST_REMEDY,
            Self::DirectJsonInput => DIRECT_JSON_INPUT_REMEDY,
            Self::ImplicitScalarErasure => IMPLICIT_SCALAR_ERASURE_REMEDY,
            Self::FabricatedOverflow => FABRICATED_OVERFLOW_REMEDY,
            Self::WrappingCounter => WRAPPING_COUNTER_REMEDY,
            Self::UncheckedCast => UNCHECKED_CAST_REMEDY,
            Self::UnownedSpawn => UNOWNED_SPAWN_REMEDY,
            Self::UnboundedChannel => UNBOUNDED_CHANNEL_REMEDY,
            Self::PoisonRecovery => POISON_RECOVERY_REMEDY,
            Self::LossyText => LOSSY_TEXT_REMEDY,
            Self::ForgottenValue => FORGOTTEN_VALUE_REMEDY,
            Self::TriStateBool => TRI_STATE_BOOL_REMEDY,
            Self::OpenAndClosed => OPEN_AND_CLOSED_REMEDY,
        }
    }
}

/// What a reader is told to type back at the tool, where an identity in it would not survive them typing it.
const HANDED_OUT: [&str; 5] = [
    "--mutant ",
    "njutest accept ",
    "njutest replay ",
    "rust-mutants explain ",
    "njutest explain ",
];

/// The names of the things that are an identity rather than a place.
const PERISHABLE: [&str; 2] = ["display_id", ".id"];

/// The call this repository does not write directly, because every place that did lost what it could not remove.
const RAW_REMOVAL: &str = "remove_dir_all";

/// What writing a terminal sequence looks like: the introducer and what must follow it, spelled either way.
///
/// The introducer alone is not enough. A renderer that measures how wide a
/// painted line is has to *read* one to skip it, and refusing that would
/// refuse the one function that makes a caret land under the right column.
/// What is refused is putting a sequence together.
const ESCAPES: [&str; 6] = [
    "\u{1b}[", "\u{1b}]", "\\u{1b}[", "\\u{1b}]", "\\x1b[", "\\x1b]",
];

/// The one module that turns a style into bytes, for the whole workspace.
const PAINTER: &str = "rust-mutants/src/telling.rs";

/// Where the rule itself is written, which has to spell what it refuses in order to refuse it.
const PAINT_RULE: [&str; 2] = ["xtask/src/lints.rs", "xtask/tests/lints.rs"];

/// The module that is allowed to make it in a loop, being the one that bounds it.
///
/// A test may make it too: what a test removes is what it made, and it is
/// standing there watching.
const RECLAIMER: &str = "crates/rust-mutants/src/reclaim.rs";

/// The only modules allowed to touch `serde_json`'s last-key-wins readers.
///
/// Repository-relative equality matters: a suffix match would let an
/// arbitrary nested `strictjson.rs` grant itself the parser capability.
const STRICT_JSON_READERS: [&str; 5] = [
    "crates/njutest-cli/src/strictjson.rs",
    "crates/njutest-devkit/src/strictjson.rs",
    "crates/rust-mutants-cli/src/strictjson.rs",
    "crates/rust-mutants/src/strictjson.rs",
    "xtask/src/strictjson.rs",
];

/// State-bearing modules where an overflow changes identity, evidence,
/// accounting, or a stored fact. Presentation geometry is deliberately absent:
/// it owns a visibly saturating UI policy rather than a persisted assertion.
fn overflow_sensitive(file: &str) -> bool {
    const EXACT: &[&str] = &[
        "crates/rust-mutants/src/catalog.rs",
        "crates/rust-mutants/src/count.rs",
        "crates/rust-mutants/src/id.rs",
        "crates/rust-mutants/src/outcomes.rs",
        "crates/rust-mutants/src/splice.rs",
        "crates/rust-mutants/src/work.rs",
        "crates/rust-mutants/src/session/mod.rs",
        "crates/rust-mutants/src/session/prepare.rs",
        "crates/rust-mutants/src/session/verify.rs",
        "crates/rust-mutants/src/trace/mod.rs",
        "crates/rust-mutants/src/trace/summary.rs",
        "crates/njutest-cli/src/cache/store.rs",
        "crates/njutest-cli/src/checkpoint.rs",
        "crates/njutest-cli/src/evidence/digest.rs",
        "crates/njutest-cli/src/evidence/key.rs",
        "crates/njutest-cli/src/evidence/store.rs",
        "crates/njutest-cli/src/evidence/tree.rs",
        "crates/njutest-cli/src/assure/run.rs",
        "crates/njutest-cli/src/wire/derive.rs",
        "crates/njutest-cli/src/wire/interpose.rs",
        "crates/njutest-cli/src/wire/mod.rs",
        "crates/rust-mutants-cli/src/app/stored.rs",
        "crates/rust-mutants-cli/src/kept.rs",
    ];
    EXACT.contains(&file)
        || file.starts_with("crates/rust-mutants/src/report/")
        || file.starts_with("crates/rust-mutants-cli/src/report/")
        || file.starts_with("crates/njutest-cli/src/report/")
        || file.starts_with("xtask/src/engineaudit/")
        || file == "xtask/src/proofaudit.rs"
}

fn strict_conversions(file: &str) -> bool {
    matches!(
        file,
        "crates/rust-mutants/src/runner/windows.rs" | "crates/rust-mutants/src/tempowner/lock.rs"
    )
}

/// One thing found in one file.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    /// What it is.
    pub kind: Kind,
    /// The file, as a repository-relative path.
    pub file: String,
    /// The 1-based line.
    pub line: usize,
}

/// A Rust construct that asks the compiler to read another source file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRedirect {
    /// A direct `include!(...)`; `None` means its argument was not one literal.
    Include {
        /// The literal target, when syntax alone names it exactly.
        target: Option<String>,
        /// The line carrying the invocation.
        line: usize,
    },
    /// A direct or `cfg_attr`-nested `#[path = ...]`; `None` means it was not a string literal.
    Path {
        /// The literal target, when syntax alone names it exactly.
        target: Option<String>,
        /// The line carrying the attribute.
        line: usize,
    },
    /// A macro body can manufacture a redirect only after this source has been inspected.
    Opaque {
        /// The line carrying the macro invocation or definition.
        line: usize,
    },
}

/// One exported entry point of a procedural-macro crate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ProcMacroExport {
    /// `attribute`, `derive`, or `function`; `conditional` is a refused export
    /// hidden behind `cfg_attr` rather than an accepted entry point.
    pub kind: &'static str,
    /// The name callers write.
    pub name: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: {}: {}",
            self.file,
            self.line,
            self.kind.label(),
            self.kind.remedy()
        )
    }
}

/// Everything `source` holds that this repository does not write.
///
/// # Errors
/// A file that is not Rust this version can parse.
pub fn scan_source(file: &str, source: &str) -> Result<Vec<Finding>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let policies = [
        (
            file.ends_with(RECLAIMER) || file.contains("/tests/"),
            SourcePolicy::Reclaimer,
        ),
        (
            STRICT_JSON_READERS.contains(&file),
            SourcePolicy::StrictJsonReader,
        ),
        (overflow_sensitive(file), SourcePolicy::OverflowSensitive),
        (strict_conversions(file), SourcePolicy::StrictConversions),
    ]
    .into_iter()
    .filter(|(enabled, _policy)| *enabled)
    .map(|(_enabled, policy)| policy)
    .collect();
    let mut scan = Scan {
        file: file.to_owned(),
        found: Vec::new(),
        looping: 0,
        aliases: Aliases::of(&parsed),
        policies,
        owned_spawn_boundaries: owned_spawn_boundaries(&parsed),
    };
    scan.visit_file(&parsed);
    scan.found.extend(comments(file, source));
    scan.found.extend(handles(file, source));
    scan.found.extend(painted(file, source));
    scan.found.extend(manual_variant_lists(&parsed, file));
    scan.found.extend(open_and_closed(&parsed, file));
    scan.found.extend(broad_expectations(&parsed, file));
    scan.found.sort();
    scan.found.dedup();
    Ok(scan.found)
}

/// Every source-file redirect visible in one parsed file.
///
/// Literal direct redirects are returned for the repository gate to resolve
/// against its complete source set. Redirects manufactured inside another
/// macro are opaque and are returned as refusals instead.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn source_redirects(source: &str) -> Result<Vec<SourceRedirect>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut visitor = SourceRedirects { found: Vec::new() };
    visitor.visit_file(&parsed);
    visitor.found.sort_by_key(|redirect| match redirect {
        SourceRedirect::Include { line, .. }
        | SourceRedirect::Path { line, .. }
        | SourceRedirect::Opaque { line } => *line,
    });
    visitor.found.dedup();
    Ok(visitor.found)
}

/// The procedural-macro names a crate exports, derived from compiler attributes.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn proc_macro_exports(source: &str) -> Result<Vec<ProcMacroExport>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut exports = Vec::new();
    for item in parsed.items {
        let syn::Item::Fn(function) = item else {
            continue;
        };
        for attribute in &function.attrs {
            if attribute.path().is_ident("proc_macro_attribute") {
                exports.push(ProcMacroExport {
                    kind: "attribute",
                    name: function.sig.ident.to_string(),
                });
            } else if attribute.path().is_ident("proc_macro") {
                exports.push(ProcMacroExport {
                    kind: "function",
                    name: function.sig.ident.to_string(),
                });
            } else if attribute.path().is_ident("proc_macro_derive") {
                let syn::Meta::List(list) = &attribute.meta else {
                    continue;
                };
                let name = list
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
                    )?
                    .first()
                    .map(meta_name);
                if let Some(name) = name {
                    exports.push(ProcMacroExport {
                        kind: "derive",
                        name,
                    });
                }
            } else if attribute.path().is_ident("cfg_attr")
                && conditional_proc_macro_export(&attribute.meta)?
            {
                exports.push(ProcMacroExport {
                    kind: "conditional",
                    name: function.sig.ident.to_string(),
                });
            }
        }
    }
    exports.sort();
    exports.dedup();
    Ok(exports)
}

fn conditional_proc_macro_export(meta: &syn::Meta) -> Result<bool, syn::Error> {
    if matches!(
        meta.path().segments.last().map(|segment| &segment.ident),
        Some(ident)
            if ident == "proc_macro"
                || ident == "proc_macro_attribute"
                || ident == "proc_macro_derive"
    ) {
        return Ok(true);
    }
    let syn::Meta::List(list) = meta else {
        return Ok(false);
    };
    if !list.path.is_ident("cfg_attr") {
        return Ok(false);
    }
    let nested = list.parse_args_with(
        syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
    )?;
    for attribute in nested.iter().skip(1) {
        if conditional_proc_macro_export(attribute)? {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Opaque token constructors in a workspace procedural macro.
///
/// The permitted implementation vocabulary keeps emitted Rust literal in a
/// `quote!` body or passes through compiler-provided input. Parsing strings,
/// constructing token primitives, formatting identifiers, or composing
/// independently quoted fragments through interpolation makes the output
/// invisible to the repository's token walk and is therefore refused. The one
/// interpolated template is the exact closed `AllVariants` implementation.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn opaque_proc_macro_synthesis(file: &str, source: &str) -> Result<Vec<Finding>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut visitor = OpaqueProcMacroSynthesis {
        file,
        found: Vec::new(),
    };
    visitor.visit_file(&parsed);
    visitor.found.sort();
    visitor.found.dedup();
    Ok(visitor.found)
}

fn meta_name(meta: &syn::Meta) -> String {
    match meta {
        syn::Meta::Path(path) => path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.to_string()),
        syn::Meta::List(list) => list
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.to_string()),
        syn::Meta::NameValue(value) => value
            .path
            .segments
            .last()
            .map_or_else(String::new, |segment| segment.ident.to_string()),
    }
}

struct SourceRedirects {
    found: Vec<SourceRedirect>,
}

impl Visit<'_> for SourceRedirects {
    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        if let Some(span) = use_tree_name_span(&item.tree, "include") {
            self.found.push(SourceRedirect::Opaque {
                line: span.start().line,
            });
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_attribute(&mut self, attribute: &syn::Attribute) {
        redirects_in_meta(
            &attribute.meta,
            attribute.pound_token.span.start().line,
            &mut self.found,
        );
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_macro(&mut self, macro_: &syn::Macro) {
        let line = macro_
            .path
            .segments
            .last()
            .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span())
            .start()
            .line;
        let name = macro_
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if name.as_deref() == Some("include") {
            let target = match syn::parse2::<syn::LitStr>(macro_.tokens.clone()) {
                Ok(literal) => Some(literal.value()),
                Err(_not_one_literal) => None,
            };
            self.found.push(SourceRedirect::Include { target, line });
        } else if tokens_can_manufacture_redirect(&macro_.tokens) {
            self.found.push(SourceRedirect::Opaque { line });
        }
        syn::visit::visit_macro(self, macro_);
    }
}

fn use_tree_name_span(tree: &syn::UseTree, name: &str) -> Option<proc_macro2::Span> {
    match tree {
        syn::UseTree::Path(path) => (path.ident == name)
            .then_some(path.ident.span())
            .or_else(|| use_tree_name_span(&path.tree, name)),
        syn::UseTree::Name(import) => (import.ident == name).then_some(import.ident.span()),
        syn::UseTree::Rename(rename) => (rename.ident == name)
            .then_some(rename.ident.span())
            .or_else(|| (rename.rename == name).then_some(rename.rename.span())),
        syn::UseTree::Group(group) => group
            .items
            .iter()
            .find_map(|item| use_tree_name_span(item, name)),
        syn::UseTree::Glob(_) => None,
    }
}

fn use_tree_has_rename(tree: &syn::UseTree) -> bool {
    match tree {
        syn::UseTree::Path(path) => use_tree_has_rename(&path.tree),
        syn::UseTree::Rename(_) => true,
        syn::UseTree::Group(group) => group.items.iter().any(use_tree_has_rename),
        syn::UseTree::Name(_) | syn::UseTree::Glob(_) => false,
    }
}

fn redirects_in_meta(meta: &syn::Meta, line: usize, found: &mut Vec<SourceRedirect>) {
    if meta.path().is_ident("path") {
        let target = match meta {
            syn::Meta::NameValue(value) => match &value.value {
                syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(literal),
                    ..
                }) => Some(literal.value()),
                _ => None,
            },
            syn::Meta::Path(_) | syn::Meta::List(_) => None,
        };
        found.push(SourceRedirect::Path { target, line });
        return;
    }
    let syn::Meta::List(list) = meta else {
        return;
    };
    if !list.path.is_ident("cfg_attr") {
        return;
    }
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => {
            for held in nested.iter().skip(1) {
                redirects_in_meta(held, line, found);
            }
        }
        Err(_opaque_cfg_attr) => found.push(SourceRedirect::Opaque { line }),
    }
}

fn tokens_can_manufacture_redirect(tokens: &proc_macro2::TokenStream) -> bool {
    if macro_path_argument_named(tokens, "include") || tokens_import_name(tokens, "include") {
        return true;
    }
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        match tree {
            proc_macro2::TokenTree::Group(group) => {
                if tokens_can_manufacture_redirect(&group.stream()) {
                    return true;
                }
            }
            proc_macro2::TokenTree::Ident(ident)
                if ident == "include"
                    && matches!(trees.get(index.saturating_add(1)), Some(proc_macro2::TokenTree::Punct(bang)) if bang.as_char() == '!')
                    && matches!(
                        trees.get(index.saturating_add(2)),
                        Some(proc_macro2::TokenTree::Group(_))
                    ) =>
            {
                return true;
            }
            proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#' => {
                let Some(proc_macro2::TokenTree::Group(group)) = trees.get(index.saturating_add(1))
                else {
                    continue;
                };
                if group.delimiter() != proc_macro2::Delimiter::Bracket {
                    continue;
                }
                match syn::parse2::<syn::Meta>(group.stream()) {
                    Ok(meta) if meta_contains_path(&meta) => return true,
                    Ok(_) => {}
                    Err(_opaque_generated_attribute) => return true,
                }
            }
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    false
}

fn macro_path_argument_named(tokens: &proc_macro2::TokenStream, name: &str) -> bool {
    let mut argument = proc_macro2::TokenStream::new();
    for token in tokens.clone() {
        if matches!(&token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ',') {
            if token_path_named(&argument, name) {
                return true;
            }
            argument = proc_macro2::TokenStream::new();
        } else {
            argument.extend(std::iter::once(token));
        }
    }
    token_path_named(&argument, name)
}

fn token_path_named(tokens: &proc_macro2::TokenStream, name: &str) -> bool {
    match syn::parse2::<syn::Path>(tokens.clone()) {
        Ok(path) => path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == name),
        Err(_not_one_path) => false,
    }
}

fn tokens_import_name(tokens: &proc_macro2::TokenStream, name: &str) -> bool {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(use_) = tree else {
            continue;
        };
        if use_ != "use" {
            continue;
        }
        for held in trees.iter().skip(index.saturating_add(1)) {
            match held {
                proc_macro2::TokenTree::Ident(ident) if ident == name => return true,
                proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ';' => break,
                proc_macro2::TokenTree::Group(group)
                    if tokens_import_name(&group.stream(), name) =>
                {
                    return true;
                }
                proc_macro2::TokenTree::Group(_)
                | proc_macro2::TokenTree::Ident(_)
                | proc_macro2::TokenTree::Punct(_)
                | proc_macro2::TokenTree::Literal(_) => {}
            }
        }
    }
    false
}

fn meta_contains_path(meta: &syn::Meta) -> bool {
    if meta.path().is_ident("path") {
        return true;
    }
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => nested.iter().skip(1).any(meta_contains_path),
        Err(_opaque_cfg_attr) => true,
    }
}

struct OpaqueProcMacroSynthesis<'a> {
    file: &'a str,
    found: Vec<Finding>,
}

impl OpaqueProcMacroSynthesis<'_> {
    fn note(&mut self, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind: Kind::OpaqueMacroSyntax,
            file: self.file.to_owned(),
            line: span.start().line,
        });
    }
}

impl Visit<'_> for OpaqueProcMacroSynthesis<'_> {
    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        if use_tree_has_rename(&item.tree)
            && let Some(span) = [
                "Ident",
                "Punct",
                "Literal",
                "Group",
                "TokenTree",
                "format_ident",
            ]
            .iter()
            .find_map(|name| use_tree_name_span(&item.tree, name))
        {
            self.note(span);
        }
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_type(&mut self, item: &syn::ItemType) {
        if let syn::Type::Path(path) = item.ty.as_ref()
            && let Some(segment) = path.path.segments.last()
            && matches!(
                segment.ident.to_string().as_str(),
                "Ident" | "Punct" | "Literal" | "Group" | "TokenTree"
            )
        {
            self.note(segment.ident.span());
        }
        syn::visit::visit_item_type(self, item);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if matches!(call.method.to_string().as_str(), "parse" | "collect") {
            self.note(call.method.span());
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let syn::Expr::Path(path) = &*call.func
            && let Some(last) = path.path.segments.last()
        {
            let method = last.ident.to_string();
            let owner = path
                .path
                .segments
                .iter()
                .rev()
                .nth(1)
                .map(|segment| segment.ident.to_string());
            let primitive = matches!(
                owner.as_deref(),
                Some("Ident" | "Punct" | "Literal" | "Group" | "TokenTree")
            );
            if method == "parse_str"
                || method == "from_str"
                || (method == "new" && primitive)
                || (method == "new_raw" && owner.as_deref() == Some("Ident"))
            {
                self.note(last.ident.span());
            }
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if path.path.segments.iter().any(|segment| {
            matches!(
                segment.ident.to_string().as_str(),
                "Ident" | "Punct" | "Literal" | "Group" | "TokenTree"
            )
        }) {
            let span = path
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(span);
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, macro_: &syn::Macro) {
        let name = macro_
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string());
        if name.as_deref() == Some("format_ident") {
            let span = macro_
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(span);
        }
        if ["proc_macro", "proc_macro_attribute", "proc_macro_derive"]
            .iter()
            .any(|attribute| macro_tokens_name(&macro_.tokens, attribute))
        {
            let span = macro_
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(span);
        }
        if [
            "Ident",
            "Punct",
            "Literal",
            "Group",
            "TokenTree",
            "format_ident",
        ]
        .iter()
        .any(|constructor| macro_tokens_name(&macro_.tokens, constructor))
        {
            let span = macro_
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(span);
        }
        if matches!(name.as_deref(), Some("quote" | "quote_spanned"))
            && quote_has_interpolation(&macro_.tokens)
            && !all_variants_quote(&macro_.tokens)
        {
            let span = macro_
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(span);
        }
        syn::visit::visit_macro(self, macro_);
    }
}

fn quote_has_interpolation(tokens: &proc_macro2::TokenStream) -> bool {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (index, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree
            && quote_has_interpolation(&group.stream())
        {
            return true;
        }
        if !matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#') {
            continue;
        }
        let interpolated = match trees.get(index.saturating_add(1)) {
            Some(proc_macro2::TokenTree::Ident(_)) => true,
            Some(proc_macro2::TokenTree::Group(group)) => {
                group.delimiter() == proc_macro2::Delimiter::Parenthesis
            }
            Some(proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_)) | None => {
                false
            }
        };
        if interpolated {
            return true;
        }
    }
    false
}

fn all_variants_quote(tokens: &proc_macro2::TokenStream) -> bool {
    const TEMPLATE: &str = r"
        impl #ident {
            /// Every variant, in declaration order.
            pub const ALL: [Self; #count] = [#(Self::#variants),*];
        }
    ";
    match TEMPLATE.parse::<proc_macro2::TokenStream>() {
        Ok(expected) => expected.to_string() == tokens.to_string(),
        Err(_invalid_static_template) => false,
    }
}

fn broad_expectations(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    for attribute in &parsed.attrs {
        if broadly_expects(attribute) {
            found.push(Finding {
                kind: Kind::BroadExpectation,
                file: file.to_owned(),
                line: attribute.pound_token.span.start().line,
            });
        }
    }
    let mut modules = ModuleExpectations {
        file,
        found: &mut found,
    };
    modules.visit_file(parsed);
    found
}

struct ModuleExpectations<'a> {
    file: &'a str,
    found: &'a mut Vec<Finding>,
}

impl<'ast> Visit<'ast> for ModuleExpectations<'_> {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        for attribute in &item.attrs {
            if broadly_expects(attribute) {
                self.found.push(Finding {
                    kind: Kind::BroadExpectation,
                    file: self.file.to_owned(),
                    line: attribute.pound_token.span.start().line,
                });
            }
        }
        syn::visit::visit_item_mod(self, item);
    }
}

fn broadly_expects(attribute: &syn::Attribute) -> bool {
    meta_broadly_expects(&attribute.meta)
}

fn meta_broadly_expects(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("expect") {
        return ["dead_code", "unsafe_code"]
            .iter()
            .any(|wanted| macro_tokens_name(&list.tokens, wanted));
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| nested.iter().skip(1).any(meta_broadly_expects))
}

/// Every enum `source` declares, by name.
///
/// The question a catch-all has to answer is whether the values can be listed
/// from this repository's own source. An enum declared here can; `syn::Expr`
/// and `toml::Value` cannot, and a catch-all over one of those is the
/// handling rather than a default. So the gate is told what is ours before it
/// is asked what is refused.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn declared_enums(source: &str) -> Result<Vec<String>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let mut found = Vec::new();
    let mut named = Named { found: &mut found };
    named.visit_file(&parsed);
    Ok(found)
}

/// The visitor that collects enum names, including the ones nested in a module or a function.
struct Named<'a> {
    found: &'a mut Vec<String>,
}

impl<'ast> Visit<'ast> for Named<'_> {
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.found.push(item.ident.to_string());
        syn::visit::visit_item_enum(self, item);
    }
}

/// The enums of `source` that say they may grow, which the compiler makes anybody outside the crate leave a place for.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn open_enums(source: &str) -> Result<Vec<String>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    Ok(open_enum_declarations(&parsed)
        .into_iter()
        .map(|(name, _line)| name)
        .collect())
}

/// Every line of `source` where a match over one of `ours` ends in a catch-all.
///
/// Read from the arms rather than from the scrutinee, because the scrutinee is
/// an expression whose type this gate cannot know: an arm spelling
/// `Decision::Tests` says what is being matched, and nothing else has to be
/// resolved to know it.
///
/// # Errors
/// The source is not Rust this compiler version can parse.
#[cfg(feature = "testkit")]
pub fn wildcards(source: &str, ours: &[String]) -> Result<Vec<usize>, syn::Error> {
    let mut found: Vec<usize> = wildcards_over(source, ours)?
        .into_iter()
        .map(|one| one.line)
        .collect();
    found.sort_unstable();
    found.dedup();
    Ok(found)
}

/// A catch-all over a set this repository closes, said as what it is rather than where it is.
///
/// A line number is a coordinate, and a coordinate is not a place. The arm
/// standing at one can be swapped for a catch-all over a different set
/// without the number moving — change `Message::BuildFinished` to
/// `Decision::Tests` on the line above and a ledger keyed by the coordinate
/// waives the second having reviewed the first, with no diff for anybody to
/// read. That was measured rather than supposed: the gate exited 0. The item
/// and the set change exactly when what is being waived changes, and a
/// renamed binding or a reformatted body leaves both alone.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Wildcard {
    /// The items enclosing it, outermost first, joined by `::`. Empty at the top of a file.
    pub item: String,
    /// The enum whose remaining variants it absorbs.
    pub over: String,
    /// The line it is on, for the sentence a person reads. Never part of the key.
    pub line: usize,
}

impl Wildcard {
    /// How the ledger names the group of catch-all arms that share this item and this set.
    ///
    /// `many` is part of the name because several arms absorbing one set in
    /// one item are one claim — *the rest of this set, here* — and one more
    /// is a claim nobody read. Leaving the count out would let a group's
    /// waiver cover an arm written after it was granted, which is the defect
    /// this key exists to remove, one layer down.
    #[must_use]
    pub fn key(&self, file: &str, many: usize) -> String {
        let arms = if many == 1 { "arm" } else { "arms" };
        if self.item.is_empty() {
            format!("{file} over {}, {many} {arms}", self.over)
        } else {
            format!("{file}::{} over {}, {many} {arms}", self.item, self.over)
        }
    }
}

/// The same arms, each with the item it sits in and the enum whose arms told this gate what was being matched.
///
/// The name is what lets a caller ask the question this walk cannot: whether
/// the arm could have been left out at all. An enum that says it may grow,
/// read from another crate, forces one — and a gate asking for a waiver
/// against something the compiler requires is asking for a decision nobody
/// made (ADR 0023).
///
/// # Errors
/// The source is not Rust this compiler version can parse.
pub fn wildcards_over(source: &str, ours: &[String]) -> Result<Vec<Wildcard>, syn::Error> {
    let parsed = syn::parse_file(source)?;
    let resolver = EnumResolver::of(&parsed, ours);
    let mut found = Vec::new();
    let mut scan = Catching {
        ours,
        resolver,
        bindings: Vec::new(),
        within: Vec::new(),
        found: &mut found,
    };
    scan.visit_file(&parsed);
    found.sort();
    found.dedup();
    Ok(found)
}

/// The visitor that refuses a catch-all where the arms name a set this repository closes.
struct Catching<'a> {
    ours: &'a [String],
    resolver: EnumResolver,
    bindings: Vec<BTreeMap<String, String>>,
    within: Vec<String>,
    found: &'a mut Vec<Wildcard>,
}

impl Catching<'_> {
    /// The items this one sits inside, outermost first.
    fn place(&self) -> String {
        self.within.join("::")
    }

    /// The head of a type, which is what an `impl` block is named by.
    fn head(held: &syn::Type) -> String {
        match held {
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .map_or_else(|| "impl".to_owned(), |last| last.ident.to_string()),
            _ => "impl".to_owned(),
        }
    }

    fn enter_function(&mut self, signature: &syn::Signature) {
        let mut bindings = BTreeMap::new();
        for input in &signature.inputs {
            let syn::FnArg::Typed(input) = input else {
                continue;
            };
            let syn::Pat::Ident(binding) = input.pat.as_ref() else {
                continue;
            };
            if let Some(over) = self.resolver.enum_type(&input.ty) {
                bindings.insert(binding.ident.to_string(), over);
            }
        }
        self.bindings.push(bindings);
    }

    fn leave_function(&mut self) {
        let bindings = self.bindings.pop();
        debug_assert!(
            bindings.is_some(),
            "every function exit follows one function entry"
        );
    }

    fn scrutinee_enum(&self, expression: &syn::Expr) -> Option<String> {
        let syn::Expr::Path(path) = expression else {
            return None;
        };
        let variable = path.path.segments.last()?.ident.to_string();
        self.bindings
            .iter()
            .rev()
            .find_map(|bindings| bindings.get(&variable).cloned())
    }

    fn named_variant(&self, pattern: &syn::Pat) -> Option<String> {
        self.resolver.named_variant(pattern)
    }

    fn forced_by_guards(arms: &[syn::Arm]) -> bool {
        let mut naming = arms
            .iter()
            .filter(|arm| catches_everything(&arm.pat).is_none());
        let mut any = false;
        naming.all(|arm| {
            any = true;
            matches!(arm.pat, syn::Pat::Guard(_))
        }) && any
    }
}

#[derive(Default)]
struct EnumResolver {
    types: BTreeMap<String, String>,
    variants: BTreeMap<String, String>,
    declared_variants: BTreeMap<String, Vec<String>>,
    globbed_variants: BTreeSet<String>,
}

impl EnumResolver {
    fn of(parsed: &syn::File, ours: &[String]) -> Self {
        let mut resolver = Self::default();
        for own in ours {
            resolver.types.insert(own.clone(), own.clone());
        }
        let mut declarations = EnumVariants {
            ours,
            found: &mut resolver.declared_variants,
        };
        declarations.visit_file(parsed);
        let mut imports = EnumImports {
            resolver: &mut resolver,
        };
        imports.visit_file(parsed);
        resolver
    }

    fn resolved_type_name(&self, name: &str) -> Option<String> {
        self.types.get(name).cloned()
    }

    fn enum_type(&self, held: &syn::Type) -> Option<String> {
        match held {
            syn::Type::Group(group) => self.enum_type(&group.elem),
            syn::Type::Paren(paren) => self.enum_type(&paren.elem),
            syn::Type::Reference(reference) => self.enum_type(&reference.elem),
            syn::Type::Path(path) => path
                .path
                .segments
                .last()
                .and_then(|segment| self.resolved_type_name(&segment.ident.to_string())),
            _ => None,
        }
    }

    fn named_variant(&self, pattern: &syn::Pat) -> Option<String> {
        match pattern {
            syn::Pat::Or(or) => or.cases.iter().find_map(|case| self.named_variant(case)),
            syn::Pat::Paren(paren) => self.named_variant(&paren.pat),
            syn::Pat::Guard(guard) => self.named_variant(&guard.pat),
            syn::Pat::Path(path) => self.path_variant(&path.path),
            syn::Pat::TupleStruct(tuple) => self.path_variant(&tuple.path),
            syn::Pat::Struct(struct_) => self.path_variant(&struct_.path),
            _ => None,
        }
    }

    fn path_variant(&self, path: &syn::Path) -> Option<String> {
        let segments: Vec<_> = path.segments.iter().collect();
        if let [segment] = segments.as_slice() {
            return self
                .variants
                .get(&segment.ident.to_string())
                .cloned()
                .or_else(|| {
                    (self.globbed_variants.len() == 1)
                        .then(|| self.globbed_variants.iter().next().cloned())
                        .and_then(std::convert::identity)
                });
        }
        let owner = segments.get(segments.len().saturating_sub(2))?;
        self.resolved_type_name(&owner.ident.to_string())
    }

    fn import(&mut self, path: &[String], tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(next) => {
                let mut nested = path.to_vec();
                nested.push(next.ident.to_string());
                self.import(&nested, &next.tree);
            }
            syn::UseTree::Name(name) => {
                let local = name.ident.to_string();
                if let Some(owner) = path.last().and_then(|head| self.resolved_type_name(head)) {
                    self.variants.insert(local, owner);
                } else if let Some(owner) = self.resolved_type_name(&local) {
                    self.types.insert(local, owner);
                }
            }
            syn::UseTree::Rename(rename) => {
                let source = rename.ident.to_string();
                let local = rename.rename.to_string();
                if let Some(owner) = path.last().and_then(|head| self.resolved_type_name(head)) {
                    self.variants.insert(local, owner);
                } else if let Some(owner) = self.resolved_type_name(&source) {
                    self.types.insert(local, owner);
                }
            }
            syn::UseTree::Glob(_) => {
                let Some(owner) = path.last().and_then(|head| self.resolved_type_name(head)) else {
                    return;
                };
                self.globbed_variants.insert(owner.clone());
                if let Some(variants) = self.declared_variants.get(&owner) {
                    for variant in variants {
                        self.variants.insert(variant.clone(), owner.clone());
                    }
                }
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.import(path, item);
                }
            }
        }
    }
}

struct EnumVariants<'a> {
    ours: &'a [String],
    found: &'a mut BTreeMap<String, Vec<String>>,
}

impl<'ast> Visit<'ast> for EnumVariants<'_> {
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        let name = item.ident.to_string();
        if self.ours.contains(&name) {
            self.found.insert(
                name,
                item.variants
                    .iter()
                    .map(|variant| variant.ident.to_string())
                    .collect(),
            );
        }
        syn::visit::visit_item_enum(self, item);
    }
}

struct EnumImports<'a> {
    resolver: &'a mut EnumResolver,
}

impl Visit<'_> for EnumImports<'_> {
    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        self.resolver.import(&[], &item.tree);
    }

    fn visit_item_type(&mut self, item: &syn::ItemType) {
        if let syn::Type::Path(path) = item.ty.as_ref()
            && let Some(source) = path.path.segments.last()
            && let Some(owner) = self.resolver.resolved_type_name(&source.ident.to_string())
        {
            self.resolver.types.insert(item.ident.to_string(), owner);
        }
        syn::visit::visit_item_type(self, item);
    }
}

/// The enum an arm names, where the pattern is a path with one before the variant.
///
/// Free rather than private to the walk, because the body-shape hint has to
/// speak about exactly the lines this gate names. Two answers to *which lines
/// catch everything left* would disagree with each other about the question instead
/// of about the code.
pub(crate) fn named_variant(pattern: &syn::Pat) -> Option<String> {
    let path = match pattern {
        syn::Pat::Or(or) => return or.cases.iter().find_map(named_variant),
        syn::Pat::Paren(paren) => return named_variant(&paren.pat),
        syn::Pat::Path(held) => &held.path,
        syn::Pat::TupleStruct(held) => &held.path,
        syn::Pat::Struct(held) => &held.path,
        syn::Pat::Guard(held) => return named_variant(&held.pat),
        _ => return None,
    };
    let mut segments = path.segments.iter().rev();
    Some(segments.nth(1)?.ident.to_string())
}

/// Whether the compiler asks for an arm catching everything left because no variant is covered unconditionally.
///
/// A guard makes an arm conditional, so a match whose every variant-naming
/// arm carries one is not exhaustive however many variants it lists, and the
/// arm that catches the rest is required rather than chosen. Asking for a
/// reviewed waiver against something the compiler demands is asking somebody
/// to decide what they could not have decided (ADR 0023).
///
/// Conservative on purpose: one unguarded naming arm and this says no, which
/// costs a ledger line rather than a blind spot.
/// Where an arm catches everything left, which is a bare `_` or a name bound to the whole.
///
/// Deliberately not looked through a guard, which is the opposite of what
/// [`named_variant`] does with one. `_ if ready()` catches nothing on its own
/// and the compiler still asks for the rest, so it is not the arm that absorbs
/// a new variant; `Decision::Tests if ready()` still says what is being
/// matched, which is all that one is read for.
pub(crate) fn catches_everything(pattern: &syn::Pat) -> Option<proc_macro2::Span> {
    match pattern {
        syn::Pat::Wild(held) => Some(held.underscore_token.span),
        syn::Pat::Ident(held) if held.subpat.is_none() => Some(held.ident.span()),
        _ => None,
    }
}

impl<'ast> Visit<'ast> for Catching<'_> {
    fn visit_item_mod(&mut self, one: &'ast syn::ItemMod) {
        let depth = self.within.len();
        self.within.push(one.ident.to_string());
        syn::visit::visit_item_mod(self, one);
        self.within.truncate(depth);
    }

    fn visit_item_impl(&mut self, one: &'ast syn::ItemImpl) {
        let depth = self.within.len();
        self.within.push(Self::head(&one.self_ty));
        syn::visit::visit_item_impl(self, one);
        self.within.truncate(depth);
    }

    fn visit_item_trait(&mut self, one: &'ast syn::ItemTrait) {
        let depth = self.within.len();
        self.within.push(one.ident.to_string());
        syn::visit::visit_item_trait(self, one);
        self.within.truncate(depth);
    }

    fn visit_item_fn(&mut self, one: &'ast syn::ItemFn) {
        let depth = self.within.len();
        self.within.push(one.sig.ident.to_string());
        self.enter_function(&one.sig);
        syn::visit::visit_item_fn(self, one);
        self.leave_function();
        self.within.truncate(depth);
    }

    fn visit_impl_item_fn(&mut self, one: &'ast syn::ImplItemFn) {
        let depth = self.within.len();
        self.within.push(one.sig.ident.to_string());
        self.enter_function(&one.sig);
        syn::visit::visit_impl_item_fn(self, one);
        self.leave_function();
        self.within.truncate(depth);
    }

    fn visit_trait_item_fn(&mut self, one: &'ast syn::TraitItemFn) {
        let depth = self.within.len();
        self.within.push(one.sig.ident.to_string());
        self.enter_function(&one.sig);
        syn::visit::visit_trait_item_fn(self, one);
        self.leave_function();
        self.within.truncate(depth);
    }

    fn visit_expr_match(&mut self, matching: &'ast syn::ExprMatch) {
        let over = matching
            .arms
            .iter()
            .find_map(|arm| self.named_variant(&arm.pat))
            .or_else(|| self.scrutinee_enum(&matching.expr))
            .filter(|name| self.ours.contains(name))
            .filter(|_name| !Self::forced_by_guards(&matching.arms));
        if let Some(over) = over {
            let item = self.place();
            for arm in &matching.arms {
                if let Some(span) = catches_everything(&arm.pat) {
                    self.found.push(Wildcard {
                        item: item.clone(),
                        over: over.clone(),
                        line: span.start().line,
                    });
                }
            }
        }
        syn::visit::visit_expr_match(self, matching);
    }
}

#[derive(Clone)]
struct EnumShape {
    variants: BTreeSet<String>,
    derives_all_variants: bool,
}

fn enum_shapes(parsed: &syn::File) -> BTreeMap<String, Vec<EnumShape>> {
    let mut found = BTreeMap::new();
    let mut declarations = EnumShapes { found: &mut found };
    declarations.visit_file(parsed);
    found
}

struct EnumShapes<'a> {
    found: &'a mut BTreeMap<String, Vec<EnumShape>>,
}

impl Visit<'_> for EnumShapes<'_> {
    fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
        self.found
            .entry(item.ident.to_string())
            .or_default()
            .push(EnumShape {
                variants: item
                    .variants
                    .iter()
                    .map(|variant| variant.ident.to_string())
                    .collect(),
                derives_all_variants: item.attrs.iter().any(derives_all_variants),
            });
        syn::visit::visit_item_enum(self, item);
    }
}

fn derives_all_variants(attribute: &syn::Attribute) -> bool {
    meta_derives_all_variants(&attribute.meta)
}

fn meta_derives_all_variants(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return match list.parse_args_with(
            syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
        ) {
            Ok(paths) => paths.iter().any(|path| {
                path.segments
                    .last()
                    .is_some_and(|segment| segment.ident == "AllVariants")
            }),
            Err(_unrecognised_derive) => false,
        };
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => nested.iter().skip(1).any(meta_derives_all_variants),
        Err(_unrecognised_cfg_attr) => false,
    }
}

fn manual_variant_lists(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let shapes = enum_shapes(parsed);
    manual_lists_in(parsed, file, &shapes)
}

fn manual_lists_in(
    parsed: &syn::File,
    file: &str,
    shapes: &BTreeMap<String, Vec<EnumShape>>,
) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut lists = ManualVariantLists {
        file,
        shapes,
        found: &mut found,
    };
    lists.visit_file(parsed);
    found
}

struct ManualVariantLists<'a> {
    file: &'a str,
    shapes: &'a BTreeMap<String, Vec<EnumShape>>,
    found: &'a mut Vec<Finding>,
}

impl Visit<'_> for ManualVariantLists<'_> {
    fn visit_item_impl(&mut self, item: &syn::ItemImpl) {
        let syn::Type::Path(type_) = item.self_ty.as_ref() else {
            syn::visit::visit_item_impl(self, item);
            return;
        };
        let Some(name) = type_.path.segments.last() else {
            syn::visit::visit_item_impl(self, item);
            return;
        };
        let Some(shapes) = self.shapes.get(&name.ident.to_string()) else {
            syn::visit::visit_item_impl(self, item);
            return;
        };
        for associated in &item.items {
            let (ident, arrays) = match associated {
                syn::ImplItem::Const(constant) => {
                    let mut arrays = Arrays { found: Vec::new() };
                    arrays.visit_expr(&constant.expr);
                    (&constant.ident, arrays.found)
                }
                syn::ImplItem::Fn(function)
                    if matches!(&function.sig.output,
                        syn::ReturnType::Type(_, returned) if fixed_self_sequence(returned)) =>
                {
                    let mut arrays = Arrays { found: Vec::new() };
                    arrays.visit_block(&function.block);
                    (&function.sig.ident, arrays.found)
                }
                _ => continue,
            };
            let explicitly_all = ident == WHOLE_LIST;
            let is_manual = shapes.iter().any(|shape| {
                if explicitly_all && shape.derives_all_variants {
                    return false;
                }
                explicitly_all
                    || arrays.iter().any(|array| {
                        array_variants(array).is_some_and(|listed| listed == shape.variants)
                    })
            });
            if is_manual {
                self.found.push(Finding {
                    kind: Kind::ManualVariantList,
                    file: self.file.to_owned(),
                    line: ident.span().start().line,
                });
            }
        }
        syn::visit::visit_item_impl(self, item);
    }
}

struct Arrays<'ast> {
    found: Vec<&'ast syn::ExprArray>,
}

impl<'ast> Visit<'ast> for Arrays<'ast> {
    fn visit_expr_array(&mut self, array: &'ast syn::ExprArray) {
        self.found.push(array);
    }
}

fn array_variants(array: &syn::ExprArray) -> Option<BTreeSet<String>> {
    let variants: Option<BTreeSet<_>> = array.elems.iter().map(self_variant).collect();
    variants.filter(|variants| !variants.is_empty())
}

fn self_variant(expression: &syn::Expr) -> Option<String> {
    let path = match expression {
        syn::Expr::Path(path) => &path.path,
        syn::Expr::Struct(struct_) => &struct_.path,
        syn::Expr::Call(call) => match call.func.as_ref() {
            syn::Expr::Path(path) => &path.path,
            _ => return None,
        },
        syn::Expr::Group(group) => return self_variant(&group.expr),
        syn::Expr::Paren(paren) => return self_variant(&paren.expr),
        _ => return None,
    };
    let mut segments = path.segments.iter().rev();
    let variant = segments.next()?;
    let owner = segments.next()?;
    (owner.ident == "Self").then(|| variant.ident.to_string())
}

/// Hand-maintained whole-enum lists, including an enum and its impl split across files.
///
/// # Errors
/// A source file that this compiler version cannot parse.
pub fn manual_variant_lists_across<'a>(
    sources: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> Result<Vec<Finding>, syn::Error> {
    let mut parsed = Vec::new();
    let mut shapes: BTreeMap<String, BTreeMap<String, Vec<EnumShape>>> = BTreeMap::new();
    for (scope, file, source) in sources {
        let syntax = syn::parse_file(source)?;
        for (name, declarations) in enum_shapes(&syntax) {
            shapes
                .entry(scope.to_owned())
                .or_default()
                .entry(name)
                .or_default()
                .extend(declarations);
        }
        parsed.push((scope.to_owned(), file.to_owned(), syntax));
    }
    let mut found = Vec::new();
    for (scope, file, syntax) in &parsed {
        if let Some(scope_shapes) = shapes.get(scope) {
            found.extend(manual_lists_in(syntax, file, scope_shapes));
        }
    }
    found.sort();
    found.dedup();
    Ok(found)
}

/// Every enum of `parsed` that publishes its whole list and also says the list is open.
///
/// Read from the syntax rather than spelled, because what makes this a
/// contradiction is two declarations about one type rather than any text.
fn open_and_closed(parsed: &syn::File, file: &str) -> Vec<Finding> {
    let open = open_enum_declarations(parsed);
    let listed = listed_enum_declarations(parsed);
    let errors = error_enum_declarations(parsed);
    open.into_iter()
        .filter(|(named, _at)| {
            !errors.contains(named) && listed.iter().any(|(listed, _line)| listed == named)
        })
        .map(|(_named, line)| Finding {
            kind: Kind::OpenAndClosed,
            file: file.to_owned(),
            line,
        })
        .collect()
}

/// Contradictory open and closed declarations, including declarations split across source files.
///
/// `scope` is the compiler unit that owns the files. Matching names in
/// different crates are unrelated, while an enum and an inherent impl in two
/// modules of one crate can name the same type through a path or import.
///
/// # Errors
/// A source file that this compiler version cannot parse.
pub fn open_and_closed_across<'a>(
    sources: impl IntoIterator<Item = (&'a str, &'a str, &'a str)>,
) -> Result<Vec<Finding>, syn::Error> {
    let mut open = Vec::new();
    let mut listed = BTreeSet::new();
    let mut errors = BTreeSet::new();
    for (scope, file, source) in sources {
        let parsed = syn::parse_file(source)?;
        open.extend(
            open_enum_declarations(&parsed)
                .into_iter()
                .map(|(name, line)| (scope.to_owned(), file.to_owned(), name, line)),
        );
        listed.extend(
            listed_enum_declarations(&parsed)
                .into_iter()
                .map(|(name, _line)| (scope.to_owned(), name)),
        );
        errors.extend(
            error_enum_declarations(&parsed)
                .into_iter()
                .map(|name| (scope.to_owned(), name)),
        );
    }
    let mut found: Vec<_> = open
        .into_iter()
        .filter(|(scope, _file, name, _line)| {
            let declaration = (scope.clone(), name.clone());
            listed.contains(&declaration) && !errors.contains(&declaration)
        })
        .map(|(_scope, file, _name, line)| Finding {
            kind: Kind::OpenAndClosed,
            file,
            line,
        })
        .collect();
    found.sort();
    found.dedup();
    Ok(found)
}

fn error_enum_declarations(parsed: &syn::File) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let mut declarations = ErrorDeclarations { found: &mut found };
    declarations.visit_file(parsed);
    found
}

struct ErrorDeclarations<'a> {
    found: &'a mut BTreeSet<String>,
}

impl Visit<'_> for ErrorDeclarations<'_> {
    fn visit_item_enum(&mut self, enum_: &syn::ItemEnum) {
        let derives_error = enum_.attrs.iter().any(|attribute| {
            let syn::Meta::List(list) = &attribute.meta else {
                return false;
            };
            list.path.is_ident("derive")
                && list
                    .parse_args_with(
                        syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
                    )
                    .is_ok_and(|paths| {
                        paths.iter().any(|path| {
                            path.segments
                                .last()
                                .is_some_and(|segment| segment.ident == "Error")
                        })
                    })
        });
        if derives_error {
            self.found.insert(enum_.ident.to_string());
        }
        syn::visit::visit_item_enum(self, enum_);
    }

    fn visit_item_impl(&mut self, impl_: &syn::ItemImpl) {
        let implements_error = impl_.trait_.as_ref().is_some_and(|(trait_, _for)| {
            trait_
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "Error")
        });
        if implements_error
            && let syn::Type::Path(type_) = impl_.self_ty.as_ref()
            && let Some(name) = type_.path.segments.last()
        {
            self.found.insert(name.ident.to_string());
        }
        syn::visit::visit_item_impl(self, impl_);
    }
}

fn open_enum_declarations(parsed: &syn::File) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    let mut visitor = OpenDeclarations { found: &mut found };
    visitor.visit_file(parsed);
    found
}

struct OpenDeclarations<'a> {
    found: &'a mut Vec<(String, usize)>,
}

impl<'ast> Visit<'ast> for OpenDeclarations<'_> {
    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        if item.attrs.iter().any(opens_set) {
            self.found
                .push((item.ident.to_string(), item.ident.span().start().line));
        }
        syn::visit::visit_item_enum(self, item);
    }
}

fn listed_enum_declarations(parsed: &syn::File) -> Vec<(String, usize)> {
    let mut found = Vec::new();
    let mut visitor = ListedDeclarations { found: &mut found };
    visitor.visit_file(parsed);
    found
}

struct ListedDeclarations<'a> {
    found: &'a mut Vec<(String, usize)>,
}

impl<'ast> Visit<'ast> for ListedDeclarations<'_> {
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let named = match item.self_ty.as_ref() {
            syn::Type::Path(path) => path.path.segments.last(),
            _ => None,
        };
        if let Some(named) = named
            && item.items.iter().any(publishes_whole_set)
        {
            self.found
                .push((named.ident.to_string(), named.ident.span().start().line));
        }
        syn::visit::visit_item_impl(self, item);
    }
}

fn publishes_whole_set(item: &syn::ImplItem) -> bool {
    match item {
        syn::ImplItem::Const(constant) => {
            matches!(constant.vis, syn::Visibility::Public(_))
                && (exhaustive_name(&constant.ident) || fixed_self_sequence(&constant.ty))
        }
        syn::ImplItem::Fn(function) => {
            has_total_self_match(&function.block)
                || (matches!(function.vis, syn::Visibility::Public(_))
                    && !function
                        .sig
                        .inputs
                        .iter()
                        .any(|input| matches!(input, syn::FnArg::Receiver(_)))
                    && (exhaustive_name(&function.sig.ident)
                        || matches!(&function.sig.output,
                            syn::ReturnType::Type(_, returned) if fixed_self_sequence(returned))))
        }
        syn::ImplItem::Type(_) | syn::ImplItem::Macro(_) | syn::ImplItem::Verbatim(_) | _ => false,
    }
}

fn has_total_self_match(block: &syn::Block) -> bool {
    let mut matches = TotalSelfMatch { found: false };
    matches.visit_block(block);
    matches.found
}

struct TotalSelfMatch {
    found: bool,
}

impl Visit<'_> for TotalSelfMatch {
    fn visit_expr_match(&mut self, expression: &syn::ExprMatch) {
        if self_expression(&expression.expr)
            && expression.arms.len() > 1
            && expression.arms.iter().all(|arm| {
                catches_everything(&arm.pat).is_none() && pattern_mentions_self(&arm.pat)
            })
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_match(self, expression);
    }
}

fn self_expression(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Path(path) => path.path.is_ident("self"),
        syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
            self_expression(&unary.expr)
        }
        syn::Expr::Group(group) => self_expression(&group.expr),
        syn::Expr::Paren(paren) => self_expression(&paren.expr),
        _ => false,
    }
}

fn pattern_mentions_self(pattern: &syn::Pat) -> bool {
    let mut mention = SelfPattern { found: false };
    mention.visit_pat(pattern);
    mention.found
}

struct SelfPattern {
    found: bool,
}

impl Visit<'_> for SelfPattern {
    fn visit_path(&mut self, path: &syn::Path) {
        if path
            .segments
            .first()
            .is_some_and(|segment| segment.ident == "Self")
        {
            self.found = true;
            return;
        }
        syn::visit::visit_path(self, path);
    }
}

fn exhaustive_name(name: &syn::Ident) -> bool {
    let name = name.to_string().to_ascii_lowercase();
    name == WHOLE_LIST.to_ascii_lowercase()
        || name == "variants"
        || name == "all_variants"
        || name == "every"
        || name.starts_with("every_")
}

fn fixed_self_sequence(type_: &syn::Type) -> bool {
    match unwrapped(type_) {
        syn::Type::Array(array) => type_path_is(&array.elem, &["Self"]),
        syn::Type::Reference(reference) => {
            matches!(unwrapped(&reference.elem), syn::Type::Slice(slice)
                if type_path_is(&slice.elem, &["Self"]))
        }
        _ => false,
    }
}

fn opens_set(attribute: &syn::Attribute) -> bool {
    meta_opens_set(&attribute.meta)
}

fn meta_opens_set(meta: &syn::Meta) -> bool {
    if meta.path().is_ident(OPEN) {
        return true;
    }
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| nested.iter().skip(1).any(meta_opens_set))
}

/// The attribute that says a type may grow without breaking anybody.
const OPEN: &str = "non_exhaustive";

/// The constant by which a type publishes every one of its variants.
///
/// Public, because that is when the promise is made. A crate keeping its own
/// list of its own enum has told nobody anything, and a gate that refused
/// that would be refusing somebody for knowing what they wrote.
const WHOLE_LIST: &str = "ALL";

/// Every place `source` writes a terminal escape out by hand.
///
/// Spelled rather than parsed, because the thing being refused is a sequence
/// of bytes and a program that meant to write one can reach it through a
/// literal, a constant, a macro argument or a format string. What matters is
/// that the bytes are in the file at all.
///
/// A test may spell one: what it is doing is reading what the painter wrote,
/// and a test that asserted on a style rather than on the bytes would be
/// asserting that the code does what it does.
fn painted(file: &str, source: &str) -> Vec<Finding> {
    if file.contains(PAINTER)
        || file.contains("/tests/")
        || PAINT_RULE.iter().any(|one| file.ends_with(one))
    {
        return Vec::new();
    }
    source
        .lines()
        .enumerate()
        .filter(|(_at, line)| ESCAPES.iter().any(|escape| line.contains(escape)))
        .map(|(at, _line)| Finding {
            kind: Kind::HandPainted,
            file: file.to_owned(),
            line: at.saturating_add(1),
        })
        .collect()
}

/// Every exported `&str` constant of `source`, by name, with the line it is on.
///
/// The cross-file pass needs these because what makes a constant a layout is
/// not how it is spelled — `"rust-mutants/explain"` is a document type and
/// `"reports/runs"` is a structure, and they look the same — but that more
/// than one module joins it onto a path.
#[must_use]
pub fn exported_strings(source: &str) -> Vec<(String, usize)> {
    declared(source)
        .filter(|(_at, _name, value)| value.contains('/'))
        .map(|(at, name, _value)| (name.to_owned(), at.saturating_add(1)))
        .collect()
}

/// Every `&str` constant a file declares, whatever its visibility, as line, name and value.
fn declared(source: &str) -> impl Iterator<Item = (usize, &str, &str)> {
    source.lines().enumerate().filter_map(|(at, line)| {
        let rest = line.trim_start();
        let rest = rest
            .split_once("const ")
            .filter(|(before, _rest)| before.is_empty() || before.starts_with("pub"))
            .map(|(_before, rest)| rest)?;
        let (name, value) = rest.split_once(": &str = ")?;
        Some((
            at,
            name.trim(),
            value.trim().trim_matches(|it| it == ';' || it == '"'),
        ))
    })
}

/// The first path segment of every directory the configuration is allowed to move.
///
/// A default a configuration field falls back to is a directory somebody can
/// rename, so a test that writes it down decides it for them. Reading the
/// defaults rather than a list here means a directory added later is gated
/// the day its default is written.
#[must_use]
pub fn configured_directories(source: &str) -> Vec<String> {
    declared(source)
        .filter(|(_at, name, _value)| {
            name.starts_with("DEFAULT_") && (name.ends_with("_DIRECTORY") || name.ends_with("_DIR"))
        })
        .filter_map(|(_at, _name, value)| value.split('/').next())
        .filter(|head| !head.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Every line of `source` that spells one of `directories` as the head of a path literal.
///
/// A directory the configuration can move is one no file may write down. The
/// literal is what makes it immovable, whether it is joined onto a root, asked
/// to exist, or handed to the engine as the place this tool writes.
#[must_use]
pub fn spelled(source: &str, directories: &[String]) -> Vec<usize> {
    let mut found = Vec::new();
    for (at, line) in source.lines().enumerate() {
        let start = line.trim_start();
        if start.starts_with("///") || start.starts_with("//!") || start.starts_with("//") {
            continue;
        }
        for literal in literals(line) {
            let structure = literal.contains('/')
                && directories
                    .iter()
                    .any(|head| literal.split('/').next() == Some(head.as_str()));
            let joined = directories.iter().any(|head| head == literal)
                && line.contains(&format!(".join(\"{literal}\")"));
            if structure || joined {
                found.push(at.saturating_add(1));
                break;
            }
        }
    }
    found
}

/// Every double-quoted literal on one line, which is close enough for a line of Rust that holds no escaped quote.
fn literals(line: &str) -> Vec<&str> {
    line.split('"')
        .skip(1)
        .step_by(2)
        .filter(|it| !it.is_empty())
        .collect()
}

/// Whether `source` joins `name` onto a path, which is what makes holding it a layout decision.
#[must_use]
pub fn joins(source: &str, name: &str) -> bool {
    source.contains(&format!(".join({name})"))
        || source.contains(&format!("{{{name}}}/"))
        || source.contains(&format!(".join(&{name})"))
}

/// Which module `source` imports `name` from, when it imports it by name.
///
/// A bare `FILE_NAME` is four different constants in this tree, and only one
/// of them spells a structure. Reading the import is what tells them apart,
/// and a name nothing imports is one this cannot speak about.
#[must_use]
pub fn imported_from(source: &str, name: &str) -> Option<String> {
    qualified(source, name).or_else(|| by_use(source, name))
}

/// The module of a name written out in full at the point it is used.
fn qualified(source: &str, name: &str) -> Option<String> {
    let split = source.split_once(&format!("::{name}"))?;
    let before = split.0;
    let module = before.rsplit("::").next()?;
    module
        .chars()
        .all(|it| it.is_ascii_lowercase() || it.is_ascii_digit() || it == '_')
        .then(|| module.to_owned())
}

fn by_use(source: &str, name: &str) -> Option<String> {
    source
        .lines()
        .filter(|line| line.trim_start().starts_with("use "))
        .find(|line| {
            line.contains(&format!("::{name}"))
                || line.contains(&format!("{{{name}")) && line.contains("::")
                || line.contains(&format!(" {name},"))
                || line.contains(&format!(", {name}"))
        })
        .and_then(|line| {
            let path = line
                .trim_start()
                .strip_prefix("use ")?
                .trim_end_matches(';');
            let head = path.split_once('{').map_or(path, |(head, _rest)| head);
            let head = head.trim().trim_end_matches("::");
            let last = head.rsplit("::").next()?;
            if last == name {
                head.trim_end_matches(name)
                    .trim_end_matches("::")
                    .rsplit("::")
                    .next()
                    .map(str::to_owned)
            } else {
                Some(last.to_owned())
            }
        })
}

/// Every format string that hands a reader a command with an identity in it.
///
/// An identity is a function of the whole file, so the edit a reader makes
/// next — the test that closes the survivor, in the file the survivor is in —
/// re-mints it. A command printed with one in it stops working the moment it
/// is followed, and a configuration record written with one stops naming
/// anything. This finds them by the shape they have: a string that tells
/// somebody what to type, built in the same expression as an identity.
fn handles(file: &str, source: &str) -> Vec<Finding> {
    if file.contains("/tests/") || file.contains("/testkit/") {
        return Vec::new();
    }
    source
        .lines()
        .enumerate()
        .filter(|(_at, line)| {
            HANDED_OUT.iter().any(|said| line.contains(said))
                && PERISHABLE.iter().any(|name| line.contains(name))
        })
        .map(|(at, _line)| Finding {
            kind: Kind::PerishableHandle,
            file: file.to_owned(),
            line: at.saturating_add(1),
        })
        .collect()
}

/// The prefix of a comment that is an instruction to this engine rather than an account of the code beside it.
const ANNOTATION: &str = "rust-mutants:";

/// The one directive an annotation may carry, which is every word the engine reads after that prefix.
const DIRECTIVE: &str = "skip";

/// The two lines of the licence header every file of this repository carries, in order.
const HEADER: [&str; 2] = [
    "SPDX-FileCopyrightText: 2026 njutest contributors",
    "SPDX-License-Identifier: MIT OR Apache-2.0",
];

/// Every comment in `source` that is neither documentation, the licence header, nor an annotation the engine reads.
fn comments(file: &str, source: &str) -> Vec<Finding> {
    let bytes: Vec<char> = source.chars().collect();
    let mut found = Vec::new();
    let mut at = 0;
    let mut line: usize = 1;
    while let Some(rest) = bytes.get(at..).filter(|rest| !rest.is_empty()) {
        let stepped = |width: usize| newlines(rest.get(..width).unwrap_or(rest));
        if let Some(width) = string_at(rest) {
            line = line.saturating_add(stepped(width));
            at = at.saturating_add(width);
            continue;
        }
        if let Some((width, text, doc)) = comment_at(rest) {
            if !doc && !is_the_header(&text, line) && !names_the_engine(&text) {
                found.push(Finding {
                    kind: Kind::Comment,
                    file: file.to_owned(),
                    line,
                });
            }
            line = line.saturating_add(stepped(width));
            at = at.saturating_add(width);
            continue;
        }
        if bytes.get(at) == Some(&'\n') {
            line = line.saturating_add(1);
        }
        at = at.saturating_add(1);
    }
    found
}

/// Whether the comment is one of the engine's own annotations, which is a thing it reads rather than a thing a person tells another person.
///
/// The prefix alone was the whole test, so a paragraph opening with it was accepted anywhere in any file, and the engine reads one directive.
fn names_the_engine(text: &str) -> bool {
    text.trim_start()
        .strip_prefix(ANNOTATION)
        .map(str::trim_start)
        .and_then(|rest| rest.strip_prefix(DIRECTIVE))
        .is_some_and(|after| after.starts_with(char::is_whitespace))
}

/// Whether the comment is one of the two licence header lines, where that file's header is.
///
/// The prefix alone was the whole test, so any comment opening `SPDX-` was accepted at any depth in any file.
fn is_the_header(text: &str, line: usize) -> bool {
    let said = text.trim();
    HEADER
        .iter()
        .enumerate()
        .any(|(at, held)| line == at.saturating_add(1) && said == *held)
}

/// How many lines a stretch of source covers past the first.
fn newlines(chars: &[char]) -> usize {
    chars.iter().filter(|one| **one == '\n').count()
}

/// The width of the literal starting here, when one does: a string, a raw string of any hash count, a byte or C string of either kind, or a character.
fn string_at(rest: &[char]) -> Option<usize> {
    let mut at = 0;
    while matches!(rest.get(at), Some('b' | 'c')) {
        at = at.saturating_add(1);
    }
    if rest.get(at) == Some(&'r') {
        let mut hashes: usize = 0;
        let mut after = at.saturating_add(1);
        while rest.get(after) == Some(&'#') {
            hashes = hashes.saturating_add(1);
            after = after.saturating_add(1);
        }
        if rest.get(after) == Some(&'"') {
            return Some(raw_string(rest, after.saturating_add(1), hashes));
        }
        return None;
    }
    match rest.get(at) {
        Some('"') => Some(quoted(rest, at.saturating_add(1), '"')),
        Some('\'') if at == 0 => character(rest),
        _ => None,
    }
}

/// The width of a quoted literal whose body starts at `from`, escapes included.
fn quoted(rest: &[char], from: usize, close: char) -> usize {
    let mut at = from;
    while at < rest.len() {
        match rest.get(at) {
            Some('\\') => at = at.saturating_add(2),
            Some(one) if *one == close => return at.saturating_add(1),
            _ => at = at.saturating_add(1),
        }
    }
    rest.len()
}

/// The width of a raw string whose body starts at `from` and closes on `hashes` hashes.
fn raw_string(rest: &[char], from: usize, hashes: usize) -> usize {
    let mut at = from;
    while at < rest.len() {
        if rest.get(at) == Some(&'"')
            && (1..=hashes).all(|step| rest.get(at.saturating_add(step)) == Some(&'#'))
        {
            return at.saturating_add(hashes).saturating_add(1);
        }
        at = at.saturating_add(1);
    }
    rest.len()
}

/// The width of a character literal starting here, or nothing where the quote opens a lifetime.
fn character(rest: &[char]) -> Option<usize> {
    if rest.get(1) == Some(&'\\') {
        let width = quoted(rest, 1, '\'');
        return (width > 1).then_some(width);
    }
    (rest.get(2) == Some(&'\'')).then_some(3)
}

/// The width of the comment starting here, its text, and whether it is documentation.
fn comment_at(rest: &[char]) -> Option<(usize, String, bool)> {
    if rest.first() != Some(&'/') {
        return None;
    }
    match rest.get(1) {
        Some('/') => {
            let doc = matches!(rest.get(2), Some('/' | '!'));
            let end = rest
                .iter()
                .position(|one| *one == '\n')
                .unwrap_or(rest.len());
            let text: String = rest.get(2..end).unwrap_or_default().iter().collect();
            Some((end, text.trim_start_matches(['/', '!']).to_owned(), doc))
        }
        Some('*') => {
            let doc = matches!(rest.get(2), Some('*' | '!'));
            let mut depth = 1usize;
            let mut at = 2;
            while at < rest.len() && depth > 0 {
                if rest.get(at) == Some(&'/') && rest.get(at.saturating_add(1)) == Some(&'*') {
                    depth = depth.saturating_add(1);
                    at = at.saturating_add(2);
                } else if rest.get(at) == Some(&'*') && rest.get(at.saturating_add(1)) == Some(&'/')
                {
                    depth = depth.saturating_sub(1);
                    at = at.saturating_add(2);
                } else {
                    at = at.saturating_add(1);
                }
            }
            let text: String = rest
                .get(2..at.saturating_sub(2))
                .unwrap_or_default()
                .iter()
                .collect();
            Some((at, text, doc))
        }
        _ => None,
    }
}

/// The part of a type alias that matters to the two type-shape rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TypeMeaning {
    /// `String`, `str`, or an alias of either.
    StringError,
    /// A `dyn Trait` value whose owner must stay visible at the use site.
    TraitObject,
    /// `Box`, `Rc`, or `Arc`, including a renamed import or transparent generic alias.
    Owned,
    /// `Cow`, including a renamed import or transparent generic alias.
    TextOwner,
    /// `Result`, including a renamed import or transparent generic alias.
    Result,
    /// The unit type, including a transparent alias.
    Unit,
}

/// One source-level alias whose meaning can be recovered without type checking.
#[derive(Debug, Clone)]
struct TypeDeclaration {
    name: String,
    parameters: BTreeSet<String>,
    held: syn::Type,
}

/// One imported name. Macro and type namespaces are both considered because
/// `Default` inhabits each of them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Rename {
    source: String,
    local: String,
}

/// The declarations needed to resolve aliases within one parsed source file.
#[derive(Default)]
struct AliasDeclarations {
    types: Vec<TypeDeclaration>,
    renames: Vec<Rename>,
    concrete_types: BTreeSet<String>,
    json_values: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for AliasDeclarations {
    fn visit_item_type(&mut self, item: &'ast syn::ItemType) {
        let parameters = generic_parameters(&item.generics);
        self.types.push(TypeDeclaration {
            name: item.ident.to_string(),
            parameters,
            held: item.ty.as_ref().clone(),
        });
        syn::visit::visit_item_type(self, item);
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        imported_renames(&item.tree, &mut Vec::new(), &mut self.renames);
        imported_json_values(&item.tree, &mut Vec::new(), &mut self.json_values);
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_struct(&mut self, item: &'ast syn::ItemStruct) {
        self.concrete_types.insert(item.ident.to_string());
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &'ast syn::ItemEnum) {
        self.concrete_types.insert(item.ident.to_string());
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &'ast syn::ItemUnion) {
        self.concrete_types.insert(item.ident.to_string());
        syn::visit::visit_item_union(self, item);
    }
}

/// Meanings a syntax-only walk can recover without pretending to be name
/// resolution. Ambiguous names are deliberately forgotten rather than guessed.
#[derive(Default)]
struct Aliases {
    types: BTreeMap<String, TypeMeaning>,
    ambiguous_types: BTreeSet<String>,
    default_derives: BTreeSet<String>,
    deserialize_derives: BTreeSet<String>,
    from_traits: BTreeSet<String>,
    drop_functions: BTreeSet<String>,
    json_values: BTreeSet<String>,
    option_types: BTreeSet<String>,
    boolean_types: BTreeSet<String>,
}

impl Aliases {
    fn of(file: &syn::File) -> Self {
        let mut declarations = AliasDeclarations::default();
        declarations.visit_file(file);
        let mut aliases = Self::default();
        aliases.ambiguous_types.extend(declarations.concrete_types);
        aliases.json_values.extend(
            declarations
                .json_values
                .iter()
                .filter(|name| !aliases.ambiguous_types.contains(*name))
                .cloned(),
        );

        let rounds = declarations
            .types
            .len()
            .saturating_add(declarations.renames.len())
            .saturating_add(1);
        for _round in 0..rounds {
            let mut changed = false;
            for rename in &declarations.renames {
                if let Some(meaning) = aliases.meaning(&rename.source) {
                    changed |= aliases.insert(&rename.local, meaning);
                }
                if rename.source == "Default" || aliases.default_derives.contains(&rename.source) {
                    changed |= aliases.default_derives.insert(rename.local.clone());
                }
                if rename.source == "Deserialize"
                    || aliases.deserialize_derives.contains(&rename.source)
                {
                    changed |= aliases.deserialize_derives.insert(rename.local.clone());
                }
                if rename.source == "From" || aliases.from_traits.contains(&rename.source) {
                    changed |= aliases.from_traits.insert(rename.local.clone());
                }
                if rename.source == "drop" || aliases.drop_functions.contains(&rename.source) {
                    changed |= aliases.drop_functions.insert(rename.local.clone());
                }
                if rename.source == "Option" || aliases.option_types.contains(&rename.source) {
                    changed |= aliases.option_types.insert(rename.local.clone());
                }
            }
            for declaration in &declarations.types {
                if aliases.json_value(&declaration.held) {
                    changed |= aliases.json_values.insert(declaration.name.clone());
                }
                if let Some(meaning) = aliases.alias_meaning(declaration) {
                    changed |= aliases.insert(&declaration.name, meaning);
                }
                if aliases.boolean(&declaration.held) {
                    changed |= aliases.boolean_types.insert(declaration.name.clone());
                }
                if aliases.forwards_option(declaration) {
                    changed |= aliases.option_types.insert(declaration.name.clone());
                }
            }
            if !changed {
                break;
            }
        }
        aliases
    }

    fn insert(&mut self, name: &str, meaning: TypeMeaning) -> bool {
        if self.ambiguous_types.contains(name) {
            return false;
        }
        match self.types.get(name).copied() {
            Some(held) if held == meaning => false,
            Some(_) => {
                self.types.remove(name);
                self.ambiguous_types.insert(name.to_owned())
            }
            None => self.types.insert(name.to_owned(), meaning).is_none(),
        }
    }

    fn meaning(&self, name: &str) -> Option<TypeMeaning> {
        match name {
            "String" | "str" => Some(TypeMeaning::StringError),
            "Box" | "Rc" | "Arc" => Some(TypeMeaning::Owned),
            "Cow" => Some(TypeMeaning::TextOwner),
            "Result" => Some(TypeMeaning::Result),
            _ => self.types.get(name).copied(),
        }
    }

    fn alias_meaning(&self, declaration: &TypeDeclaration) -> Option<TypeMeaning> {
        self.alias_meanings(declaration).into_iter().next()
    }

    fn alias_meanings(&self, declaration: &TypeDeclaration) -> Vec<TypeMeaning> {
        let mut meanings = Vec::new();
        if self.string_error(&declaration.held) {
            meanings.push(TypeMeaning::StringError);
        }
        if self.trait_object(&declaration.held) {
            meanings.push(TypeMeaning::TraitObject);
        }
        if self.unit(&declaration.held) {
            meanings.push(TypeMeaning::Unit);
        }
        for meaning in [
            TypeMeaning::Owned,
            TypeMeaning::TextOwner,
            TypeMeaning::Result,
        ] {
            let mut forwarded = ConstructorUse {
                aliases: self,
                parameters: &declaration.parameters,
                meaning,
                found: false,
            };
            forwarded.visit_type(&declaration.held);
            if forwarded.found {
                meanings.push(meaning);
            }
        }
        meanings
    }

    fn string_error(&self, held: &syn::Type) -> bool {
        match unwrapped(held) {
            syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
                match self.meaning(&segment.ident.to_string()) {
                    Some(TypeMeaning::StringError) => true,
                    Some(TypeMeaning::Owned | TypeMeaning::TextOwner) => type_arguments(segment)
                        .iter()
                        .any(|argument| self.string_error(argument)),
                    Some(TypeMeaning::Result | TypeMeaning::TraitObject | TypeMeaning::Unit)
                    | None => false,
                }
            }),
            syn::Type::Reference(reference) => self.string_error(&reference.elem),
            _ => false,
        }
    }

    fn trait_object(&self, held: &syn::Type) -> bool {
        match unwrapped(held) {
            syn::Type::TraitObject(_) => true,
            syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
                self.meaning(&segment.ident.to_string()) == Some(TypeMeaning::TraitObject)
            }),
            _ => false,
        }
    }

    fn owned(&self, name: &str) -> bool {
        self.meaning(name) == Some(TypeMeaning::Owned)
    }

    fn unit(&self, held: &syn::Type) -> bool {
        match unwrapped(held) {
            syn::Type::Tuple(tuple) => tuple.elems.is_empty(),
            syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
                self.meaning(&segment.ident.to_string()) == Some(TypeMeaning::Unit)
            }),
            _ => false,
        }
    }

    fn is_from_trait(&self, name: &str) -> bool {
        name == "From" || self.from_traits.contains(name)
    }

    fn result(&self, name: &str) -> bool {
        self.meaning(name) == Some(TypeMeaning::Result)
    }

    fn boolean(&self, held: &syn::Type) -> bool {
        match unwrapped(held) {
            syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
                segment.ident == "bool" || self.boolean_types.contains(&segment.ident.to_string())
            }),
            _ => false,
        }
    }

    fn forwards_option(&self, declaration: &TypeDeclaration) -> bool {
        let syn::Type::Path(path) = unwrapped(&declaration.held) else {
            return false;
        };
        path.path.segments.last().is_some_and(|segment| {
            (segment.ident == "Option" || self.option_types.contains(&segment.ident.to_string()))
                && type_arguments(segment).first().is_some_and(|argument| {
                    matches!(unwrapped(argument), syn::Type::Path(parameter)
                    if parameter.qself.is_none()
                        && parameter.path.segments.len() == 1
                        && parameter.path.segments.first().is_some_and(|segment| {
                            declaration.parameters.contains(&segment.ident.to_string())
                        }))
                })
        })
    }

    fn tri_state_bool(&self, held: &syn::Type) -> bool {
        match unwrapped(held) {
            syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
                (segment.ident == "Option"
                    || self.option_types.contains(&segment.ident.to_string()))
                    && type_arguments(segment)
                        .first()
                        .is_some_and(|argument| self.boolean(argument))
            }),
            _ => false,
        }
    }

    fn drops(&self, path: &syn::Path) -> bool {
        path.segments.last().is_some_and(|segment| {
            segment.ident == "drop" || self.drop_functions.contains(&segment.ident.to_string())
        })
    }

    fn default_derive(&self, path: &syn::Path) -> bool {
        path.segments.last().is_some_and(|segment| {
            segment.ident == "Default" || self.default_derives.contains(&segment.ident.to_string())
        })
    }

    fn deserialize_derive(&self, path: &syn::Path) -> bool {
        path.segments.last().is_some_and(|segment| {
            segment.ident == "Deserialize"
                || self
                    .deserialize_derives
                    .contains(&segment.ident.to_string())
        })
    }

    fn json_value(&self, held: &syn::Type) -> bool {
        match unwrapped(held) {
            syn::Type::Path(path) => {
                let explicitly_json = path
                    .path
                    .segments
                    .iter()
                    .any(|segment| segment.ident == "serde_json")
                    && path
                        .path
                        .segments
                        .last()
                        .is_some_and(|segment| segment.ident == "Value");
                explicitly_json
                    || path.path.segments.last().is_some_and(|segment| {
                        self.json_values.contains(&segment.ident.to_string())
                    })
            }
            _ => false,
        }
    }

    fn json_value_owner(&self, path: &syn::Path) -> bool {
        path.segments.iter().rev().nth(1).is_some_and(|segment| {
            segment.ident == "Value" && path.segments.iter().any(|part| part.ident == "serde_json")
                || self.json_values.contains(&segment.ident.to_string())
        })
    }
}

fn imported_renames(tree: &syn::UseTree, path: &mut Vec<String>, found: &mut Vec<Rename>) {
    match tree {
        syn::UseTree::Path(one) => {
            let depth = path.len();
            path.push(one.ident.to_string());
            imported_renames(&one.tree, path, found);
            path.truncate(depth);
        }
        syn::UseTree::Rename(one) => found.push(Rename {
            source: one.ident.to_string(),
            local: one.rename.to_string(),
        }),
        syn::UseTree::Group(group) => {
            for item in &group.items {
                imported_renames(item, path, found);
            }
        }
        syn::UseTree::Name(_) | syn::UseTree::Glob(_) => {}
    }
}

fn imported_json_values(tree: &syn::UseTree, path: &mut Vec<String>, found: &mut BTreeSet<String>) {
    match tree {
        syn::UseTree::Path(one) => {
            let depth = path.len();
            path.push(one.ident.to_string());
            imported_json_values(&one.tree, path, found);
            path.truncate(depth);
        }
        syn::UseTree::Name(one)
            if path.first().is_some_and(|root| root == "serde_json") && one.ident == "Value" =>
        {
            found.insert(one.ident.to_string());
        }
        syn::UseTree::Rename(one)
            if path.first().is_some_and(|root| root == "serde_json") && one.ident == "Value" =>
        {
            found.insert(one.rename.to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                imported_json_values(item, path, found);
            }
        }
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
    }
}

fn unwrapped(mut held: &syn::Type) -> &syn::Type {
    loop {
        held = match held {
            syn::Type::Group(group) => &group.elem,
            syn::Type::Paren(paren) => &paren.elem,
            _ => return held,
        };
    }
}

fn scalar_type(held: &syn::Type) -> bool {
    match unwrapped(held) {
        syn::Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
            let name = segment.ident.to_string();
            ["String", "str", "Path", "PathBuf", "OsStr", "OsString"].contains(&name.as_str())
                || (["Box", "Rc", "Arc", "Cow", "Vec"].contains(&name.as_str())
                    && type_arguments(segment).iter().any(|held| scalar_type(held)))
                || (name == "u8" && path.path.segments.len() == 1)
        }),
        syn::Type::Reference(reference) => scalar_type(&reference.elem),
        syn::Type::Slice(slice) => scalar_type(&slice.elem),
        syn::Type::Array(array) => scalar_type(&array.elem),
        _ => false,
    }
}

fn generic_parameters(generics: &syn::Generics) -> BTreeSet<String> {
    generics
        .params
        .iter()
        .filter_map(|parameter| match parameter {
            syn::GenericParam::Type(parameter) => Some(parameter.ident.to_string()),
            syn::GenericParam::Lifetime(_) | syn::GenericParam::Const(_) => None,
        })
        .collect()
}

fn type_arguments(segment: &syn::PathSegment) -> Vec<&syn::Type> {
    let syn::PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return Vec::new();
    };
    arguments
        .args
        .iter()
        .filter_map(|argument| match argument {
            syn::GenericArgument::Type(held) => Some(held),
            _ => None,
        })
        .collect()
}

/// A constructor in an alias that receives one of the alias's type parameters.
struct ConstructorUse<'a> {
    aliases: &'a Aliases,
    parameters: &'a BTreeSet<String>,
    meaning: TypeMeaning,
    found: bool,
}

impl<'ast> Visit<'ast> for ConstructorUse<'_> {
    fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
        if self.found {
            return;
        }
        for segment in &path.path.segments {
            if self.aliases.meaning(&segment.ident.to_string()) != Some(self.meaning) {
                continue;
            }
            let arguments = type_arguments(segment);
            let receives_parameter = match self.meaning {
                TypeMeaning::Owned | TypeMeaning::TextOwner => arguments
                    .iter()
                    .any(|argument| uses_parameter(argument, self.parameters)),
                TypeMeaning::Result => arguments
                    .get(1)
                    .is_some_and(|argument| uses_parameter(argument, self.parameters)),
                TypeMeaning::StringError | TypeMeaning::TraitObject | TypeMeaning::Unit => false,
            };
            if receives_parameter {
                self.found = true;
                return;
            }
        }
        syn::visit::visit_type_path(self, path);
    }
}

fn uses_parameter(held: &syn::Type, parameters: &BTreeSet<String>) -> bool {
    let mut use_ = ParameterUse {
        parameters,
        found: false,
    };
    use_.visit_type(held);
    use_.found
}

struct ParameterUse<'a> {
    parameters: &'a BTreeSet<String>,
    found: bool,
}

impl Visit<'_> for ParameterUse<'_> {
    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if path
            .path
            .segments
            .iter()
            .any(|segment| self.parameters.contains(&segment.ident.to_string()))
        {
            self.found = true;
            return;
        }
        syn::visit::visit_type_path(self, path);
    }
}

fn derives_default(attribute: &syn::Attribute, aliases: &Aliases) -> bool {
    meta_derives_default(&attribute.meta, aliases)
}

fn meta_derives_default(meta: &syn::Meta, aliases: &Aliases) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| paths.iter().any(|path| aliases.default_derive(path)));
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| {
            nested
                .iter()
                .skip(1)
                .any(|held| meta_derives_default(held, aliases))
        })
}

fn default_marker(attribute: &syn::Attribute) -> bool {
    meta_marks_default(&attribute.meta)
}

fn meta_marks_default(meta: &syn::Meta) -> bool {
    if meta.path().is_ident("default") {
        return true;
    }
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| nested.iter().skip(1).any(meta_marks_default))
}

fn derives_deserialize(attribute: &syn::Attribute, aliases: &Aliases) -> bool {
    meta_derives_deserialize(&attribute.meta, aliases)
}

fn meta_derives_deserialize(meta: &syn::Meta, aliases: &Aliases) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| paths.iter().any(|path| aliases.deserialize_derive(path)));
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| {
            nested
                .iter()
                .skip(1)
                .any(|held| meta_derives_deserialize(held, aliases))
        })
}

fn serde_option(attribute: &syn::Attribute, wanted: &str) -> bool {
    meta_has_serde_option(&attribute.meta, wanted)
}

fn meta_has_serde_option(meta: &syn::Meta, wanted: &str) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("serde") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|nested| nested.iter().any(|held| held.path().is_ident(wanted)));
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| {
            nested
                .iter()
                .skip(1)
                .any(|held| meta_has_serde_option(held, wanted))
        })
}

#[derive(Clone, Copy)]
enum DeserializationShape {
    Scalar,
    Map,
    ExternalCapture,
}

fn open_deserialization(
    attrs: &[syn::Attribute],
    aliases: &Aliases,
    shape: DeserializationShape,
) -> bool {
    let deserializes = attrs
        .iter()
        .any(|attribute| derives_deserialize(attribute, aliases));
    let tagged = attrs.iter().any(|attribute| serde_option(attribute, "tag"));
    let transparent = attrs
        .iter()
        .any(|attribute| serde_option(attribute, "transparent"));
    let closed = attrs
        .iter()
        .any(|attribute| serde_option(attribute, "deny_unknown_fields"));
    let input_has_named_fields = matches!(shape, DeserializationShape::Map);
    let captures_external = matches!(shape, DeserializationShape::ExternalCapture);
    deserializes
        && (input_has_named_fields || tagged)
        && !transparent
        && !closed
        && !captures_external
}

const PERMISSIVE_INPUT_OPTIONS: [&str; 5] = ["flatten", "other", "untagged", "default", "alias"];

fn strict_owned_input(file: &str) -> bool {
    file.contains("/src/trace/")
        || file.ends_with("/src/trace.rs")
        || file.contains("/src/report/")
        || file.ends_with("/src/report.rs")
        || file.contains("/src/wire/")
        || file.ends_with("/src/wire.rs")
        || file.ends_with("/src/provider.rs")
}

fn permissive_input_attr(attrs: &[syn::Attribute]) -> bool {
    PERMISSIVE_INPUT_OPTIONS.iter().any(|option| {
        attrs
            .iter()
            .any(|attribute| serde_option(attribute, option))
    })
}

fn struct_has_permissive_input(item: &syn::ItemStruct, aliases: &Aliases, file: &str) -> bool {
    derives_deserialize_in(&item.attrs, aliases)
        && strict_owned_input(file)
        && (permissive_input_attr(&item.attrs)
            || item
                .fields
                .iter()
                .any(|field| permissive_input_attr(&field.attrs)))
}

fn enum_has_permissive_input(item: &syn::ItemEnum, aliases: &Aliases, file: &str) -> bool {
    derives_deserialize_in(&item.attrs, aliases)
        && strict_owned_input(file)
        && (permissive_input_attr(&item.attrs)
            || item.variants.iter().any(|variant| {
                permissive_input_attr(&variant.attrs)
                    || variant
                        .fields
                        .iter()
                        .any(|field| permissive_input_attr(&field.attrs))
            }))
}

fn derives_deserialize_in(attrs: &[syn::Attribute], aliases: &Aliases) -> bool {
    attrs
        .iter()
        .any(|attribute| derives_deserialize(attribute, aliases))
}

fn captures_external_fields(attrs: &[syn::Attribute], fields: &syn::Fields) -> bool {
    if attrs
        .iter()
        .any(|attribute| serde_option(attribute, "deny_unknown_fields"))
    {
        return false;
    }
    let syn::Fields::Named(fields) = fields else {
        return false;
    };
    let flattened: Vec<_> = fields
        .named
        .iter()
        .filter(|field| {
            field
                .attrs
                .iter()
                .any(|attribute| serde_option(attribute, "flatten"))
        })
        .collect();
    let [field] = flattened.as_slice() else {
        return false;
    };
    field
        .ident
        .as_ref()
        .is_some_and(|ident| ident == "external_fields")
        && matches!(field.vis, syn::Visibility::Inherited)
        && exact_flatten_attribute(&field.attrs)
        && external_fields_type(&field.ty)
}

fn exact_flatten_attribute(attrs: &[syn::Attribute]) -> bool {
    let serde: Vec<_> = attrs
        .iter()
        .filter(|attribute| attribute.path().is_ident("serde"))
        .collect();
    let [attribute] = serde.as_slice() else {
        return false;
    };
    let syn::Meta::List(list) = &attribute.meta else {
        return false;
    };
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|options| {
            options.len() == 1
                && options
                    .first()
                    .is_some_and(|option| option.path().is_ident("flatten"))
        })
}

fn external_fields_type(held: &syn::Type) -> bool {
    let syn::Type::Path(map) = unwrapped(held) else {
        return false;
    };
    let names: Vec<_> = map
        .path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect();
    if names != ["std", "collections", "BTreeMap"] {
        return false;
    }
    let Some(map) = map.path.segments.last() else {
        return false;
    };
    let arguments = type_arguments(map);
    let [key, value] = arguments.as_slice() else {
        return false;
    };
    type_path_is(key, &["String"]) && type_path_is(value, &["serde_json", "Value"])
}

fn type_path_is(held: &syn::Type, wanted: &[&str]) -> bool {
    let syn::Type::Path(path) = unwrapped(held) else {
        return false;
    };
    path.path
        .segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .eq(wanted.iter().copied())
}

fn external_capture_allowed(file: &str, item: &str) -> bool {
    const ALLOWED: &[(&str, &[&str])] = &[
        (
            "crates/rust-mutants/src/cargo/messages.rs",
            &[
                "BuildScript",
                "Artifact",
                "Profile",
                "CompilerMessage",
                "Diagnostic",
                "DiagnosticSpan",
            ],
        ),
        (
            "crates/rust-mutants/src/cargo/metadata.rs",
            &[
                "Metadata",
                "Resolve",
                "Node",
                "NodeDep",
                "DepKind",
                "Package",
                "Dependency",
                "Target",
            ],
        ),
        (
            "crates/rust-mutants/src/coverage.rs",
            &["Export", "Datum", "Function"],
        ),
        ("xtask/src/sbom.rs", &["CargoMetadata", "CargoPackage"]),
    ];
    ALLOWED
        .iter()
        .any(|(allowed_file, items)| file == *allowed_file && items.contains(&item))
}

fn manual_default_allowed(file: &str, item: &str) -> bool {
    const ALLOWED: &[(&str, &[&str])] = &[
        ("crates/njutest-devkit/src/fake_cargo.rs", &["Script"]),
        ("crates/njutest-devkit/src/repo.rs", &["Repo"]),
        ("crates/njutest-cli/src/app/lsp.rs", &["Encoding"]),
        ("crates/njutest-cli/src/build.rs", &["Selection"]),
        ("crates/njutest-cli/src/cli.rs", &["Format", "Ui"]),
        (
            "crates/njutest-cli/src/config.rs",
            &[
                "Cache",
                "Config",
                "Contract",
                "Execution",
                "Fuzz",
                "Reports",
            ],
        ),
        ("crates/njutest-cli/src/kept.rs", &["Ledger"]),
        (
            "crates/njutest-cli/src/presentation/mod.rs",
            &["Glyphs", "Reader", "Terminal", "Wanted"],
        ),
        (
            "crates/njutest-cli/src/report/mod.rs",
            &["Tool", "Toolchain"],
        ),
        ("crates/njutest-cli/src/trace/mod.rs", &["Recorder"]),
        ("crates/njutest-cli/src/wire/mod.rs", &["Wire"]),
        (
            "crates/rust-mutants-cli/src/config.rs",
            &["Config", "Execution", "Mutation", "Reports", "Stryker"],
        ),
        ("crates/rust-mutants-cli/src/kept.rs", &["Ledger"]),
        ("crates/rust-mutants-cli/src/tui.rs", &["Pane"]),
        ("crates/rust-mutants-cli/src/ui.rs", &["Color", "Ui"]),
        (
            "crates/rust-mutants/src/cargo/compile.rs",
            &["CompileOptions"],
        ),
        ("crates/rust-mutants/src/catalog.rs", &["Builder"]),
        ("crates/rust-mutants/src/count.rs", &["Count"]),
        ("crates/rust-mutants/src/interval.rs", &["Forest"]),
        (
            "crates/rust-mutants/src/session/mod.rs",
            &["PrepareOptions"],
        ),
        ("crates/rust-mutants/src/trace/mod.rs", &["Recorder"]),
        ("crates/rust-mutants/src/validate.rs", &["ValidateOptions"]),
    ];
    ALLOWED
        .iter()
        .any(|(allowed_file, items)| file == *allowed_file && items.contains(&item))
}

fn implemented_type_name(held: &syn::Type) -> Option<String> {
    let syn::Type::Path(path) = unwrapped(held) else {
        return None;
    };
    path.path
        .segments
        .last()
        .map(|segment| segment.ident.to_string())
}

type SourcePoint = (usize, usize);

fn source_point(span: proc_macro2::Span) -> SourcePoint {
    let start = span.start();
    (start.line, start.column)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpawnResource {
    Child,
    Thread,
    ScopedThread,
}

#[derive(Clone)]
struct SpawnOwner {
    field: SpawnField,
    resource: SpawnResource,
    direct_resource: bool,
}

#[derive(Clone)]
enum SpawnField {
    Named(String),
    Unnamed(u32),
}

impl SpawnField {
    fn matches(&self, member: &syn::Member) -> bool {
        match (self, member) {
            (Self::Named(expected), syn::Member::Named(actual)) => actual == expected,
            (Self::Unnamed(expected), syn::Member::Unnamed(actual)) => actual.index == *expected,
            (Self::Named(_), syn::Member::Unnamed(_))
            | (Self::Unnamed(_), syn::Member::Named(_)) => false,
        }
    }
}

struct SpawnOwnerDeclarations<'a> {
    concrete_types: &'a BTreeSet<String>,
    found: BTreeMap<String, Vec<SpawnOwner>>,
}

impl Visit<'_> for SpawnOwnerDeclarations<'_> {
    fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
        let Some(owner) = spawn_owner(item, self.concrete_types) else {
            syn::visit::visit_item_struct(self, item);
            return;
        };
        self.found
            .entry(item.ident.to_string())
            .or_default()
            .push(owner);
        syn::visit::visit_item_struct(self, item);
    }
}

fn spawn_owner(item: &syn::ItemStruct, concrete_types: &BTreeSet<String>) -> Option<SpawnOwner> {
    if !matches!(item.vis, syn::Visibility::Inherited) {
        return None;
    }
    let fields: Vec<(SpawnField, &syn::Field)> = match &item.fields {
        syn::Fields::Named(fields) => fields
            .named
            .iter()
            .filter_map(|field| Some((SpawnField::Named(field.ident.as_ref()?.to_string()), field)))
            .collect(),
        syn::Fields::Unnamed(fields) => fields
            .unnamed
            .iter()
            .enumerate()
            .map(|(index, field)| match u32::try_from(index) {
                Ok(index) => Some((SpawnField::Unnamed(index), field)),
                Err(_unrepresentable_field_index) => None,
            })
            .collect::<Option<Vec<_>>>()?,
        syn::Fields::Unit => return None,
    };
    if fields
        .iter()
        .any(|(_member, field)| !matches!(field.vis, syn::Visibility::Inherited))
    {
        return None;
    }
    let mut resources = fields.into_iter().filter_map(|(member, field)| {
        let resource = spawn_resource(&field.ty, concrete_types)?;
        let direct_resource = direct_spawn_resource(&field.ty, concrete_types) == Some(resource);
        Some((member, resource, direct_resource))
    });
    let (field, resource, direct_resource) = resources.next()?;
    if resources.next().is_some() {
        return None;
    }
    Some(SpawnOwner {
        field,
        resource,
        direct_resource,
    })
}

fn spawn_resource(held: &syn::Type, concrete_types: &BTreeSet<String>) -> Option<SpawnResource> {
    match held {
        syn::Type::Group(group) => spawn_resource(&group.elem, concrete_types),
        syn::Type::Paren(paren) => spawn_resource(&paren.elem, concrete_types),
        syn::Type::Path(path) if path.qself.is_none() => {
            let segment = path.path.segments.last()?;
            let name = segment.ident.to_string();
            if name == "Option" {
                return type_arguments(segment)
                    .first()
                    .and_then(|inner| spawn_resource(inner, concrete_types));
            }
            let standard = path.path.segments.iter().map(|part| part.ident.to_string());
            let path_names: Vec<String> = standard.collect();
            let externally_named = path_names.len() == 1 && !concrete_types.contains(&name);
            match name.as_str() {
                "Child"
                    if externally_named
                        || path_names == ["std", "process", "Child"]
                        || path_names == ["process", "Child"] =>
                {
                    Some(SpawnResource::Child)
                }
                "JoinHandle"
                    if externally_named
                        || path_names == ["std", "thread", name.as_str()]
                        || path_names == ["thread", name.as_str()] =>
                {
                    Some(SpawnResource::Thread)
                }
                "ScopedJoinHandle"
                    if externally_named
                        || path_names == ["std", "thread", name.as_str()]
                        || path_names == ["thread", name.as_str()] =>
                {
                    Some(SpawnResource::ScopedThread)
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn direct_spawn_resource(
    held: &syn::Type,
    concrete_types: &BTreeSet<String>,
) -> Option<SpawnResource> {
    match held {
        syn::Type::Group(group) => direct_spawn_resource(&group.elem, concrete_types),
        syn::Type::Paren(paren) => direct_spawn_resource(&paren.elem, concrete_types),
        syn::Type::Path(path)
            if path.qself.is_none()
                && path
                    .path
                    .segments
                    .last()
                    .is_some_and(|part| part.ident != "Option") =>
        {
            spawn_resource(held, concrete_types)
        }
        _ => None,
    }
}

#[derive(Default)]
struct SpawnCalls {
    points: Vec<SourcePoint>,
}

impl Visit<'_> for SpawnCalls {
    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref()
            && let Some(method) = path.path.segments.last()
            && method.ident == "spawn"
        {
            self.points.push(source_point(method.ident.span()));
        }
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if call.method == "spawn" {
            self.points.push(source_point(call.method.span()));
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

fn expression_spawns(expression: &syn::Expr) -> bool {
    let mut calls = SpawnCalls::default();
    calls.visit_expr(expression);
    !calls.points.is_empty()
}

#[derive(Default)]
struct PatternBindings {
    names: BTreeSet<String>,
}

impl Visit<'_> for PatternBindings {
    fn visit_pat_ident(&mut self, pattern: &syn::PatIdent) {
        self.names.insert(pattern.ident.to_string());
        syn::visit::visit_pat_ident(self, pattern);
    }
}

fn pattern_bindings(pattern: &syn::Pat) -> BTreeSet<String> {
    let mut bindings = PatternBindings::default();
    bindings.visit_pat(pattern);
    bindings.names
}

struct SpawnResultBindings {
    names: BTreeSet<String>,
}

impl Visit<'_> for SpawnResultBindings {
    fn visit_local(&mut self, local: &syn::Local) {
        if local
            .init
            .as_ref()
            .is_some_and(|init| expression_spawns(&init.expr))
        {
            self.names.extend(pattern_bindings(&local.pat));
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_expr_match(&mut self, expression: &syn::ExprMatch) {
        if expression_spawns(&expression.expr) {
            for arm in &expression.arms {
                self.names.extend(pattern_bindings(&arm.pat));
            }
        }
        syn::visit::visit_expr_match(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if call.method == "map" && expression_spawns(&call.receiver) {
            for argument in &call.args {
                if let syn::Expr::Closure(closure) = argument {
                    for input in &closure.inputs {
                        self.names.extend(pattern_bindings(input));
                    }
                }
            }
        }
        syn::visit::visit_expr_method_call(self, call);
    }
}

struct NamedExpression<'a> {
    names: &'a BTreeSet<String>,
    found: bool,
}

impl Visit<'_> for NamedExpression<'_> {
    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if path.qself.is_none()
            && path.path.segments.len() == 1
            && path
                .path
                .segments
                .first()
                .is_some_and(|part| self.names.contains(&part.ident.to_string()))
        {
            self.found = true;
        }
        syn::visit::visit_expr_path(self, path);
    }
}

fn expression_names_any(expression: &syn::Expr, names: &BTreeSet<String>) -> bool {
    let mut finder = NamedExpression {
        names,
        found: false,
    };
    finder.visit_expr(expression);
    finder.found
}

struct OwnerInitializer<'a> {
    owner: &'a str,
    field: &'a SpawnField,
    bindings: &'a BTreeSet<String>,
    found: bool,
}

impl Visit<'_> for OwnerInitializer<'_> {
    fn visit_expr_struct(&mut self, expression: &syn::ExprStruct) {
        let is_owner = expression
            .path
            .segments
            .last()
            .is_some_and(|part| part.ident == "Self" || part.ident == self.owner);
        if is_owner
            && expression.fields.iter().any(|field| {
                self.field.matches(&field.member)
                    && (expression_spawns(&field.expr)
                        || expression_names_any(&field.expr, self.bindings))
            })
        {
            self.found = true;
        }
        syn::visit::visit_expr_struct(self, expression);
    }

    fn visit_expr_call(&mut self, expression: &syn::ExprCall) {
        let syn::Expr::Path(path) = expression.func.as_ref() else {
            syn::visit::visit_expr_call(self, expression);
            return;
        };
        let is_owner = path
            .path
            .segments
            .last()
            .is_some_and(|part| part.ident == "Self" || part.ident == self.owner);
        let Some(index) = (match self.field {
            SpawnField::Unnamed(index) => match usize::try_from(*index) {
                Ok(index) => Some(index),
                Err(_unrepresentable_field_index) => None,
            },
            SpawnField::Named(_) => None,
        }) else {
            syn::visit::visit_expr_call(self, expression);
            return;
        };
        if is_owner
            && expression.args.get(index).is_some_and(|argument| {
                expression_spawns(argument) || expression_names_any(argument, self.bindings)
            })
        {
            self.found = true;
        }
        syn::visit::visit_expr_call(self, expression);
    }
}

fn spawn_reaches_owner_field(block: &syn::Block, owner: &str, field: &SpawnField) -> bool {
    let mut bindings = SpawnResultBindings {
        names: BTreeSet::new(),
    };
    bindings.visit_block(block);
    let mut initializer = OwnerInitializer {
        owner,
        field,
        bindings: &bindings.names,
        found: false,
    };
    initializer.visit_block(block);
    initializer.found
}

#[derive(Default)]
struct MethodBodyFacts {
    field_referenced: bool,
    field_reassigned_or_mutably_borrowed: bool,
    calls: BTreeSet<String>,
    self_calls: BTreeSet<String>,
    reaps_or_terminates: bool,
}

impl MethodBodyFacts {
    fn of(block: &syn::Block, field: &SpawnField) -> Self {
        let mut facts = Self::default();
        let mut visitor = MethodBodyVisitor {
            field,
            facts: &mut facts,
        };
        visitor.visit_block(block);
        facts
    }
}

struct MethodBodyVisitor<'a> {
    field: &'a SpawnField,
    facts: &'a mut MethodBodyFacts,
}

impl Visit<'_> for MethodBodyVisitor<'_> {
    fn visit_expr_field(&mut self, expression: &syn::ExprField) {
        if self.field.matches(&expression.member) && expression_is_self(&expression.base) {
            self.facts.field_referenced = true;
        }
        syn::visit::visit_expr_field(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        self.facts.calls.insert(call.method.to_string());
        if expression_is_self(&call.receiver) {
            self.facts.self_calls.insert(call.method.to_string());
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_assign(&mut self, assignment: &syn::ExprAssign) {
        if expression_is_owner_field(&assignment.left, self.field) {
            self.facts.field_reassigned_or_mutably_borrowed = true;
        }
        syn::visit::visit_expr_assign(self, assignment);
    }

    fn visit_expr_reference(&mut self, reference: &syn::ExprReference) {
        if reference.mutability.is_some() && expression_is_owner_field(&reference.expr, self.field)
        {
            self.facts.field_reassigned_or_mutably_borrowed = true;
        }
        syn::visit::visit_expr_reference(self, reference);
    }
}

fn expression_is_owner_field(expression: &syn::Expr, field: &SpawnField) -> bool {
    match unwrap_expression(expression) {
        syn::Expr::Field(access) => {
            field.matches(&access.member) && expression_is_self(&access.base)
        }
        _ => false,
    }
}

fn expression_is_self(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Group(group) => expression_is_self(&group.expr),
        syn::Expr::Paren(paren) => expression_is_self(&paren.expr),
        syn::Expr::Path(path) => {
            path.qself.is_none()
                && path.path.segments.len() == 1
                && path
                    .path
                    .segments
                    .first()
                    .is_some_and(|part| part.ident == "self")
        }
        syn::Expr::Reference(reference) => expression_is_self(&reference.expr),
        _ => false,
    }
}

struct TypeName<'a> {
    wanted: &'a str,
    found: bool,
}

impl Visit<'_> for TypeName<'_> {
    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if path
            .path
            .segments
            .iter()
            .any(|part| part.ident == self.wanted)
        {
            self.found = true;
        }
        syn::visit::visit_type_path(self, path);
    }
}

fn return_type_names(signature: &syn::Signature, wanted: &str) -> bool {
    let syn::ReturnType::Type(_arrow, held) = &signature.output else {
        return false;
    };
    let mut name = TypeName {
        wanted,
        found: false,
    };
    name.visit_type(held);
    name.found
}

fn local_abort_functions(parsed: &syn::File) -> BTreeSet<String> {
    parsed
        .items
        .iter()
        .filter_map(|item| {
            let syn::Item::Fn(function) = item else {
                return None;
            };
            if !matches!(function.vis, syn::Visibility::Inherited)
                || !matches!(
                    &function.sig.output,
                    syn::ReturnType::Type(_arrow, output)
                        if matches!(output.as_ref(), syn::Type::Never(_))
                )
                || !block_is_process_abort(&function.block)
            {
                return None;
            }
            Some(function.sig.ident.to_string())
        })
        .collect()
}

fn block_is_process_abort(block: &syn::Block) -> bool {
    let [statement] = block.stmts.as_slice() else {
        return false;
    };
    let syn::Stmt::Expr(expression, _semicolon) = statement else {
        return false;
    };
    let syn::Expr::Call(call) = unwrap_expression(expression) else {
        return false;
    };
    if !call.args.is_empty() {
        return false;
    }
    let syn::Expr::Path(path) = unwrap_expression(&call.func) else {
        return false;
    };
    path.qself.is_none()
        && path
            .path
            .segments
            .iter()
            .map(|part| part.ident.to_string())
            .eq(["std", "process", "abort"].map(str::to_owned))
}

fn unwrap_expression(mut expression: &syn::Expr) -> &syn::Expr {
    loop {
        match expression {
            syn::Expr::Group(group) => expression = &group.expr,
            syn::Expr::Paren(paren) => expression = &paren.expr,
            _ => return expression,
        }
    }
}

fn drop_reaps_or_terminates(
    block: &syn::Block,
    field: &SpawnField,
    local_abort_functions: &BTreeSet<String>,
) -> bool {
    let [statement] = block.stmts.as_slice() else {
        return false;
    };
    let syn::Stmt::Expr(syn::Expr::Match(reap), _semicolon) = statement else {
        return false;
    };
    if !field_try_wait(&reap.expr, field) || reap.arms.len() != 2 {
        return false;
    }
    let successful = reap.arms.iter().filter(|arm| {
        ok_some_pattern(&arm.pat)
            && matches!(unwrap_expression(&arm.body), syn::Expr::Block(block) if block.block.stmts.is_empty())
    });
    let terminal = reap.arms.iter().filter(|arm| {
        none_or_error_pattern(&arm.pat)
            && expression_calls_local_abort(&arm.body, local_abort_functions)
    });
    successful.count() == 1 && terminal.count() == 1
}

fn field_try_wait(expression: &syn::Expr, field: &SpawnField) -> bool {
    let syn::Expr::MethodCall(call) = unwrap_expression(expression) else {
        return false;
    };
    call.method == "try_wait"
        && call.args.is_empty()
        && expression_is_owner_field(&call.receiver, field)
}

fn ok_some_pattern(pattern: &syn::Pat) -> bool {
    let syn::Pat::TupleStruct(ok) = pattern else {
        return false;
    };
    let Some(some) = exactly_one(ok.elems.iter()) else {
        return false;
    };
    if !one_segment_path(&ok.path, "Ok") {
        return false;
    }
    let syn::Pat::TupleStruct(some) = some else {
        return false;
    };
    let Some(status) = exactly_one(some.elems.iter()) else {
        return false;
    };
    one_segment_path(&some.path, "Some") && irrefutable_binding(status)
}

fn none_or_error_pattern(pattern: &syn::Pat) -> bool {
    let syn::Pat::Or(alternatives) = pattern else {
        return false;
    };
    let Some((left, right)) = exactly_two(alternatives.cases.iter()) else {
        return false;
    };
    ok_none_pattern(left) && error_pattern(right) || error_pattern(left) && ok_none_pattern(right)
}

fn ok_none_pattern(pattern: &syn::Pat) -> bool {
    let syn::Pat::TupleStruct(ok) = pattern else {
        return false;
    };
    let Some(none) = exactly_one(ok.elems.iter()) else {
        return false;
    };
    one_segment_path(&ok.path, "Ok")
        && matches!(none, syn::Pat::Ident(ident) if ident.ident == "None" && ident.subpat.is_none())
}

fn error_pattern(pattern: &syn::Pat) -> bool {
    let syn::Pat::TupleStruct(error) = pattern else {
        return false;
    };
    let Some(source) = exactly_one(error.elems.iter()) else {
        return false;
    };
    one_segment_path(&error.path, "Err") && irrefutable_binding(source)
}

const fn irrefutable_binding(pattern: &syn::Pat) -> bool {
    matches!(pattern, syn::Pat::Wild(_))
        || matches!(pattern, syn::Pat::Ident(binding) if binding.subpat.is_none())
}

fn one_segment_path(path: &syn::Path, wanted: &str) -> bool {
    path.segments.len() == 1
        && path
            .segments
            .first()
            .is_some_and(|part| part.ident == wanted)
}

fn exactly_one<'a, T>(mut values: impl Iterator<Item = &'a T>) -> Option<&'a T> {
    let first = values.next()?;
    if values.next().is_none() {
        Some(first)
    } else {
        None
    }
}

fn exactly_two<'a, T>(mut values: impl Iterator<Item = &'a T>) -> Option<(&'a T, &'a T)> {
    let first = values.next()?;
    let second = values.next()?;
    if values.next().is_none() {
        Some((first, second))
    } else {
        None
    }
}

fn expression_calls_local_abort(
    expression: &syn::Expr,
    local_abort_functions: &BTreeSet<String>,
) -> bool {
    let syn::Expr::Call(call) = unwrap_expression(expression) else {
        return false;
    };
    if !call.args.is_empty() {
        return false;
    }
    let syn::Expr::Path(path) = unwrap_expression(&call.func) else {
        return false;
    };
    path.qself.is_none()
        && path.path.segments.len() == 1
        && path
            .path
            .segments
            .first()
            .is_some_and(|part| local_abort_functions.contains(&part.ident.to_string()))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpawnBoundaryVisibility {
    Private,
    Exposed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SpawnReceiver {
    Associated,
    Consuming,
    Borrowing,
}

struct SpawnMethod {
    name: String,
    visibility: SpawnBoundaryVisibility,
    receiver: SpawnReceiver,
    returns_owner: bool,
    spawn_points: Vec<SourcePoint>,
    spawn_reaches_field: bool,
    body: MethodBodyFacts,
}

impl SpawnMethod {
    fn of(method: &syn::ImplItemFn, owner: &str, field: &SpawnField) -> Self {
        let mut spawns = SpawnCalls::default();
        spawns.visit_block(&method.block);
        let receiver = method.sig.receiver();
        Self {
            name: method.sig.ident.to_string(),
            visibility: if matches!(method.vis, syn::Visibility::Inherited) {
                SpawnBoundaryVisibility::Private
            } else {
                SpawnBoundaryVisibility::Exposed
            },
            receiver: match receiver.map(|receiver| &receiver.kind) {
                None => SpawnReceiver::Associated,
                Some(syn::ReceiverKind::Value) => SpawnReceiver::Consuming,
                Some(syn::ReceiverKind::Reference(..) | syn::ReceiverKind::Typed(..)) => {
                    SpawnReceiver::Borrowing
                }
                Some(_) => SpawnReceiver::Borrowing,
            },
            returns_owner: return_type_names(&method.sig, "Self")
                || return_type_names(&method.sig, owner),
            spawn_points: spawns.points,
            spawn_reaches_field: spawn_reaches_owner_field(&method.block, owner, field),
            body: MethodBodyFacts::of(&method.block, field),
        }
    }
}

#[derive(Default)]
struct SpawnOwnerImpls {
    methods: BTreeMap<String, Vec<SpawnMethod>>,
    drops: BTreeMap<String, Vec<MethodBodyFacts>>,
}

struct SpawnImplCollector<'a> {
    owners: &'a BTreeMap<String, Vec<SpawnOwner>>,
    local_abort_functions: &'a BTreeSet<String>,
    found: SpawnOwnerImpls,
}

impl Visit<'_> for SpawnImplCollector<'_> {
    fn visit_item_impl(&mut self, item: &syn::ItemImpl) {
        let Some(owner_name) = implemented_type_name(&item.self_ty) else {
            syn::visit::visit_item_impl(self, item);
            return;
        };
        let Some([owner]) = self.owners.get(&owner_name).map(Vec::as_slice) else {
            syn::visit::visit_item_impl(self, item);
            return;
        };
        let trait_name = item
            .trait_
            .as_ref()
            .and_then(|(path, _for)| path.segments.last())
            .map(|part| part.ident.to_string());
        if trait_name.as_deref() == Some("Drop") {
            for method in &item.items {
                if let syn::ImplItem::Fn(method) = method
                    && method.sig.ident == "drop"
                {
                    let mut body = MethodBodyFacts::of(&method.block, &owner.field);
                    body.reaps_or_terminates = drop_reaps_or_terminates(
                        &method.block,
                        &owner.field,
                        self.local_abort_functions,
                    );
                    self.found
                        .drops
                        .entry(owner_name.clone())
                        .or_default()
                        .push(body);
                }
            }
        } else if trait_name.is_none() {
            let methods = self.found.methods.entry(owner_name.clone()).or_default();
            for method in &item.items {
                if let syn::ImplItem::Fn(method) = method {
                    methods.push(SpawnMethod::of(method, &owner_name, &owner.field));
                }
            }
        }
        syn::visit::visit_item_impl(self, item);
    }
}

fn owner_cleanup_proven(
    owner: &SpawnOwner,
    methods: &[SpawnMethod],
    drops: &[MethodBodyFacts],
) -> bool {
    match owner.resource {
        SpawnResource::Thread => thread_cleanup_proven(methods, drops, true),
        SpawnResource::ScopedThread => thread_cleanup_proven(methods, drops, false),
        SpawnResource::Child => child_cleanup_proven(owner, methods, drops),
    }
}

fn thread_cleanup_proven(
    methods: &[SpawnMethod],
    drops: &[MethodBodyFacts],
    drop_required: bool,
) -> bool {
    let mut cleanup: BTreeSet<String> = methods
        .iter()
        .filter(|method| {
            method.visibility == SpawnBoundaryVisibility::Private
                && method.receiver != SpawnReceiver::Associated
                && method.body.field_referenced
                && method.body.calls.contains("join")
                && (method.receiver == SpawnReceiver::Consuming
                    || method.body.calls.contains("take"))
        })
        .map(|method| method.name.clone())
        .collect();
    loop {
        let before = cleanup.len();
        for method in methods {
            if method.visibility == SpawnBoundaryVisibility::Private
                && method.receiver != SpawnReceiver::Associated
                && method
                    .body
                    .self_calls
                    .iter()
                    .any(|called| cleanup.contains(called))
            {
                cleanup.insert(method.name.clone());
            }
        }
        if cleanup.len() == before {
            break;
        }
    }
    if cleanup.is_empty() {
        return false;
    }
    !drop_required
        || drops.iter().any(|drop_| {
            drop_.field_referenced && drop_.calls.contains("take") && drop_.calls.contains("join")
                || drop_
                    .self_calls
                    .iter()
                    .any(|called| cleanup.contains(called))
        })
}

fn child_cleanup_proven(
    owner: &SpawnOwner,
    methods: &[SpawnMethod],
    drops: &[MethodBodyFacts],
) -> bool {
    let fail_closed = owner.direct_resource
        && methods
            .iter()
            .all(|method| !method.body.field_reassigned_or_mutably_borrowed)
        && matches!(drops, [drop_body] if drop_body.reaps_or_terminates);
    if fail_closed {
        return true;
    }

    let mut cleanup: BTreeSet<String> = methods
        .iter()
        .filter(|method| {
            method.visibility == SpawnBoundaryVisibility::Private
                && method.body.field_referenced
                && method.body.calls.contains("wait")
        })
        .map(|method| method.name.clone())
        .collect();
    loop {
        let before = cleanup.len();
        for method in methods {
            if method.visibility == SpawnBoundaryVisibility::Private
                && method
                    .body
                    .self_calls
                    .iter()
                    .any(|called| cleanup.contains(called))
            {
                cleanup.insert(method.name.clone());
            }
        }
        if cleanup.len() == before {
            break;
        }
    }
    drops.iter().any(|drop_| {
        drop_.field_referenced && drop_.calls.contains("wait")
            || drop_
                .self_calls
                .iter()
                .any(|called| cleanup.contains(called))
    })
}

fn owned_spawn_boundaries(parsed: &syn::File) -> BTreeSet<SourcePoint> {
    let mut declarations = AliasDeclarations::default();
    declarations.visit_file(parsed);
    let mut owners = SpawnOwnerDeclarations {
        concrete_types: &declarations.concrete_types,
        found: BTreeMap::new(),
    };
    owners.visit_file(parsed);
    let local_abort_functions = local_abort_functions(parsed);
    let mut implementations = SpawnImplCollector {
        owners: &owners.found,
        local_abort_functions: &local_abort_functions,
        found: SpawnOwnerImpls::default(),
    };
    implementations.visit_file(parsed);
    let mut allowed = BTreeSet::new();
    for (name, declarations) in &owners.found {
        let [owner] = declarations.as_slice() else {
            continue;
        };
        let methods = match implementations.found.methods.get(name) {
            Some(methods) => methods.as_slice(),
            None => &[],
        };
        let drops = match implementations.found.drops.get(name) {
            Some(drops) => drops.as_slice(),
            None => &[],
        };
        if !owner_cleanup_proven(owner, methods, drops) {
            continue;
        }
        for method in methods {
            if method.visibility == SpawnBoundaryVisibility::Private
                && method.receiver == SpawnReceiver::Associated
                && method.returns_owner
                && method.spawn_reaches_field
                && let [point] = method.spawn_points.as_slice()
            {
                allowed.insert(*point);
            }
        }
    }
    allowed
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SourcePolicy {
    Reclaimer,
    StrictJsonReader,
    OverflowSensitive,
    StrictConversions,
}

struct Scan {
    file: String,
    found: Vec<Finding>,
    /// How many loop bodies the walk is inside, which is what makes a removal unbounded.
    looping: usize,
    /// Aliases whose meaning this file itself declares unambiguously.
    aliases: Aliases,
    /// Exact capabilities and stricter checks selected for this source file.
    policies: BTreeSet<SourcePolicy>,
    /// Raw spawn expressions whose surrounding owner proves cleanup structurally.
    owned_spawn_boundaries: BTreeSet<SourcePoint>,
}

impl Scan {
    fn has_policy(&self, policy: SourcePolicy) -> bool {
        self.policies.contains(&policy)
    }

    fn note_each(&mut self, kind: Kind, spans: impl IntoIterator<Item = proc_macro2::Span>) {
        for span in spans {
            self.note(kind, span);
        }
    }

    /// Walks a loop body, counting it, so a removal inside one is seen as inside one.
    fn within_a_loop(&mut self, walk: impl FnOnce(&mut Self)) {
        self.looping = self.looping.saturating_add(1);
        walk(self);
        self.looping = self.looping.saturating_sub(1);
    }

    fn note(&mut self, kind: Kind, span: proc_macro2::Span) {
        self.found.push(Finding {
            kind,
            file: self.file.clone(),
            line: span.start().line,
        });
    }

    fn note_type_declaration(
        &mut self,
        name: &syn::Ident,
        held: &syn::Type,
        parameters: BTreeSet<String>,
    ) {
        let declaration = TypeDeclaration {
            name: name.to_string(),
            parameters,
            held: held.clone(),
        };
        for meaning in self.aliases.alias_meanings(&declaration) {
            self.note(alias_kind(meaning), name.span());
        }
    }

    fn type_forwards_owned(&self, held: &syn::Type, parameters: &BTreeSet<String>) -> bool {
        let mut forwarded = ConstructorUse {
            aliases: &self.aliases,
            parameters,
            meaning: TypeMeaning::Owned,
            found: false,
        };
        forwarded.visit_type(held);
        forwarded.found
    }

    fn raw_spawn_boundary(&self, span: proc_macro2::Span) -> bool {
        self.owned_spawn_boundaries.contains(&source_point(span))
    }

    fn scan_concurrency_imports(&mut self, tree: &syn::UseTree) {
        self.note_each(Kind::UnownedSpawn, imported_function_spans(tree, "spawn"));
        self.note_each(
            Kind::UnboundedChannel,
            imported_function_spans(tree, "channel"),
        );
        for method in ["clear_poison", "into_inner", "get_ref", "get_mut"] {
            self.note_each(
                Kind::PoisonRecovery,
                imported_poison_method_spans(tree, method),
            );
        }
        self.note_each(Kind::PoisonRecovery, renamed_use_spans(tree, "PoisonError"));
    }

    fn scan_erasure_imports(&mut self, tree: &syn::UseTree) {
        self.note_each(Kind::DiscardedResult, renamed_fallible_iterators(tree));
        self.note_each(Kind::TriStateBool, renamed_use_spans(tree, "Option"));
        for erased in ["Deref", "AsRef", "Borrow", "Into"] {
            self.note_each(Kind::ImplicitScalarErasure, renamed_use_spans(tree, erased));
        }
    }

    fn scan_arithmetic_imports(&mut self, tree: &syn::UseTree) {
        for method in [
            "fetch_add",
            "fetch_sub",
            "wrapping_add",
            "wrapping_add_signed",
            "wrapping_sub",
            "wrapping_sub_signed",
            "wrapping_mul",
            "wrapping_div",
            "wrapping_div_euclid",
            "wrapping_rem",
            "wrapping_rem_euclid",
            "wrapping_pow",
            "wrapping_shl",
            "wrapping_shr",
            "wrapping_neg",
            "wrapping_abs",
        ] {
            self.note_each(Kind::WrappingCounter, renamed_use_spans(tree, method));
        }
        if self.has_policy(SourcePolicy::OverflowSensitive) {
            for method in [
                "saturating_add",
                "saturating_sub",
                "saturating_mul",
                "unwrap_or",
            ] {
                self.note_each(Kind::FabricatedOverflow, renamed_use_spans(tree, method));
            }
        }
    }

    fn scan_lossy_text_imports(&mut self, tree: &syn::UseTree) {
        for method in ["from_utf8_lossy", "to_string_lossy"] {
            self.note_each(Kind::LossyText, renamed_use_spans(tree, method));
        }
    }

    fn scan_forget_imports(&mut self, tree: &syn::UseTree) {
        self.note_each(Kind::ForgottenValue, renamed_use_spans(tree, "forget"));
        self.note_each(
            Kind::ForgottenValue,
            renamed_use_spans(tree, "ManuallyDrop"),
        );
    }

    fn scan_json_imports(&mut self, item: &syn::ItemUse) {
        if self.has_policy(SourcePolicy::StrictJsonReader) {
            return;
        }
        self.note_each(Kind::DirectJsonInput, direct_json_import_spans(&item.tree));
        self.note_each(
            Kind::DirectJsonInput,
            json_value_boundary_spans(&item.tree, !matches!(item.vis, syn::Visibility::Inherited)),
        );
    }

    fn scan_concurrency_call(&mut self, call: &syn::ExprCall) {
        let syn::Expr::Path(path) = call.func.as_ref() else {
            return;
        };
        let Some(method) = path.path.segments.last() else {
            return;
        };
        if method.ident == "spawn" && !self.raw_spawn_boundary(method.ident.span()) {
            self.note(Kind::UnownedSpawn, method.ident.span());
        }
        if unbounded_channel_path(&path.path) {
            self.note(Kind::UnboundedChannel, method.ident.span());
        }
        if poison_recovery_path(&path.path) {
            self.note(Kind::PoisonRecovery, method.ident.span());
        }
        if wrapping_counter_method(&method.ident) {
            self.note(Kind::WrappingCounter, method.ident.span());
        }
        if self.has_policy(SourcePolicy::OverflowSensitive)
            && (overflow_method(&method.ident)
                || (method.ident == "unwrap_or" && call.args.iter().any(fabricated_fallback)))
        {
            self.note(Kind::FabricatedOverflow, method.ident.span());
        }
    }

    fn scan_result_call(&mut self, call: &syn::ExprCall) {
        let syn::Expr::Path(path) = call.func.as_ref() else {
            return;
        };
        let Some(method) = path.path.segments.last() else {
            return;
        };
        if ["ok", "err"]
            .iter()
            .any(|name| result_path_method(&path.path, name))
        {
            self.note(Kind::DiscardedResult, method.ident.span());
        }
        if ["unwrap_or_else", "map_or_else", "or_else"]
            .iter()
            .any(|name| result_path_method(&path.path, name))
            && call.args.iter().nth(1).is_some_and(closure_ignores_input)
        {
            self.note(Kind::DiscardedResult, method.ident.span());
        }
        if result_path_method(&path.path, "into_iter") {
            self.note(Kind::DiscardedResult, method.ident.span());
        }
        if self.aliases.drops(&path.path) && call.args.iter().any(dropped_computation) {
            self.note(Kind::DroppedComputation, method.ident.span());
        }
    }

    fn scan_repository_call(&mut self, call: &syn::ExprCall) {
        let syn::Expr::Path(path) = call.func.as_ref() else {
            return;
        };
        let Some(method) = path.path.segments.last() else {
            return;
        };
        if method.ident == "from_utf8_lossy" || method.ident == "to_string_lossy" {
            self.note(Kind::LossyText, method.ident.span());
        }
        if method.ident == "forget" {
            self.note(Kind::ForgottenValue, method.ident.span());
        }
        if self.looping > 0
            && !self.has_policy(SourcePolicy::Reclaimer)
            && method.ident == RAW_REMOVAL
        {
            self.note(Kind::UnboundedRemoval, method.ident.span());
        }
        let is_iterator_flatten = method.ident == "flatten"
            && path
                .path
                .segments
                .iter()
                .rev()
                .nth(1)
                .is_some_and(|segment| segment.ident == "Iterator");
        if is_iterator_flatten {
            self.note(Kind::DiscardedResult, method.ident.span());
        }
    }

    fn scan_macro_runtime(&mut self, macro_: &syn::Macro) {
        let tokens = &macro_.tokens;
        self.note_each(
            Kind::UnownedSpawn,
            identifier_spans_in_tokens(tokens, "spawn"),
        );
        self.note_each(
            Kind::UnboundedChannel,
            identifier_spans_in_tokens(tokens, "channel"),
        );
        self.note_each(
            Kind::PoisonRecovery,
            poison_recovery_spans_in_tokens(tokens),
        );
        for method in ["from_utf8_lossy", "to_string_lossy"] {
            self.note_each(Kind::LossyText, identifier_spans_in_tokens(tokens, method));
        }
        self.note_each(
            Kind::ForgottenValue,
            identifier_spans_in_tokens(tokens, "forget"),
        );
        self.note_each(
            Kind::ForgottenValue,
            identifier_spans_in_tokens(tokens, "ManuallyDrop"),
        );
        let tri_state = tri_state_bools_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::TriStateBool, tri_state);
        let tri_state_aliases = macro_type_aliases(tokens, |held| {
            self.aliases.boolean(held) || self.aliases.tri_state_bool(held)
        });
        self.note_each(Kind::TriStateBool, tri_state_aliases);
        self.note_each(
            Kind::UnownedSpawn,
            renamed_identifiers_in_tokens(tokens, "spawn"),
        );
        self.note_each(
            Kind::UnboundedChannel,
            renamed_identifiers_in_tokens(tokens, "channel"),
        );
        self.note_each(
            Kind::WrappingCounter,
            wrapping_counter_spans_in_tokens(tokens),
        );
        if self.has_policy(SourcePolicy::OverflowSensitive) {
            self.note_each(
                Kind::FabricatedOverflow,
                fabricated_overflow_spans_in_tokens(tokens),
            );
        }
        if self.has_policy(SourcePolicy::StrictConversions) {
            self.note_each(Kind::UncheckedCast, unchecked_cast_spans_in_tokens(tokens));
        }
    }

    fn scan_macro_json(&mut self, macro_: &syn::Macro) {
        if self.has_policy(SourcePolicy::StrictJsonReader) {
            return;
        }
        let spans = direct_json_spans_in_tokens(&macro_.tokens, &self.aliases);
        self.note_each(Kind::DirectJsonInput, spans);
        if macro_.path.is_ident("macro_rules") && macro_tokens_name(&macro_.tokens, "parse") {
            let span = macro_
                .path
                .segments
                .first()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::DirectJsonInput, span);
        }
    }

    fn scan_macro_attributes(&mut self, macro_: &syn::Macro) {
        let tokens = &macro_.tokens;
        let constant_cfg_macro = if macro_
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "cfg")
        {
            match syn::parse2::<syn::Meta>(tokens.clone()) {
                Ok(condition) => cfg_constant(&condition) != CfgTruth::Variable,
                Err(_opaque_condition) => false,
            }
        } else {
            false
        };
        if constant_cfg_macro {
            let span = macro_
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::VacuousCfg, span);
        }
        if macro_path_argument_named(tokens, "cfg") || tokens_import_name(tokens, "cfg") {
            let span = macro_
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::VacuousCfg, span);
        }
        self.note_each(Kind::VacuousCfg, vacuous_cfg_macros_in_tokens(tokens));
        self.note_each(
            Kind::OpaqueMacroSyntax,
            opaque_macro_attribute_spans(tokens),
        );
        let mut metas = Vec::new();
        macro_attribute_metas(tokens, &mut metas);
        self.note_each(
            Kind::AllowAttribute,
            metas
                .iter()
                .flat_map(|(meta, _span)| allow_attribute_spans(meta)),
        );
        self.note_each(
            Kind::VacuousCfg,
            metas
                .iter()
                .flat_map(|(meta, _span)| vacuous_cfg_spans(meta)),
        );
        if macro_tokens_name(tokens, "enum") {
            let spans = macro_default_spans(tokens, &self.aliases);
            self.note_each(Kind::DerivedEnumDefault, spans);
        }
        if macro_tokens_name(tokens, "impl") && macro_tokens_name(tokens, "Default") {
            let span = macro_
                .path
                .segments
                .first()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::SemanticDefault, span);
        }
        let scalar_erasure = macro_tokens_name(tokens, "impl")
            && ["Deref", "AsRef", "Borrow", "Into", "From"]
                .iter()
                .any(|name| macro_tokens_name(tokens, name))
            && [
                "str", "String", "Path", "PathBuf", "OsStr", "OsString", "Cow", "Box", "Rc", "Arc",
                "Vec", "u8",
            ]
            .iter()
            .any(|name| macro_tokens_name(tokens, name));
        if scalar_erasure {
            let span = macro_
                .path
                .segments
                .first()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::ImplicitScalarErasure, span);
        }
    }

    fn scan_macro_types(&mut self, macro_: &syn::Macro) {
        let tokens = &macro_.tokens;
        let owned = owned_trait_objects_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::OwnedTraitObject, owned);
        let pointers = owned_pointer_aliases_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::OwnedPointerAlias, pointers);
        let unit_conversions = unit_domain_conversions_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::UnitDomainConversion, unit_conversions);
        let unit_aliases = unit_aliases_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::UnitDomainConversion, unit_aliases);
        let trait_aliases = trait_object_aliases_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::TraitObjectAlias, trait_aliases);
        let string_aliases = string_aliases_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::StringAlias, string_aliases);
        let string_errors = string_errors_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::StringError, string_errors);
        let unit_errors = unit_errors_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::UnitError, unit_errors);
    }

    fn scan_macro_computations(&mut self, macro_: &syn::Macro) {
        let tokens = &macro_.tokens;
        self.note_each(
            Kind::DiscardedResult,
            iterator_flatten_spans_in_tokens(tokens),
        );
        self.note_each(
            Kind::DiscardedResult,
            result_into_iter_spans_in_tokens(tokens),
        );
        self.note_each(
            Kind::DiscardedResult,
            result_erasure_spans_in_tokens(tokens),
        );
        self.note_each(
            Kind::DiscardedResult,
            ignored_result_if_lets_in_tokens(tokens),
        );
        let dropped = dropped_computations_in_tokens(tokens, &self.aliases);
        self.note_each(Kind::DroppedComputation, dropped);
        self.note_each(
            Kind::IgnoredComputation,
            ignored_computations_in_tokens(tokens),
        );
        let open =
            open_deserializations_in_tokens(tokens, &self.aliases, strict_owned_input(&self.file));
        self.note_each(Kind::OpenDeserialization, open);
        self.note_each(
            Kind::DeserializeDeriveAlias,
            renamed_identifiers_in_tokens(tokens, "Deserialize"),
        );
        self.note_each(
            Kind::DroppedComputation,
            renamed_identifiers_in_tokens(tokens, "drop"),
        );
        for erased in ["Deref", "AsRef", "Borrow", "Into"] {
            self.note_each(
                Kind::ImplicitScalarErasure,
                renamed_identifiers_in_tokens(tokens, erased),
            );
        }
    }
}

impl Visit<'_> for Scan {
    fn visit_local(&mut self, local: &syn::Local) {
        if let Some(init) = &local.init
            && ignored_computation(&init.expr)
            && let Some(span) = ignored_binding_span(&local.pat)
        {
            self.note(Kind::IgnoredComputation, span);
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_item_type(&mut self, item: &syn::ItemType) {
        if self.aliases.tri_state_bool(&item.ty)
            || self.aliases.option_types.contains(&item.ident.to_string())
            || self.aliases.boolean_types.contains(&item.ident.to_string())
        {
            self.note(Kind::TriStateBool, item.ident.span());
        }
        if !self.has_policy(SourcePolicy::StrictJsonReader) && self.aliases.json_value(&item.ty) {
            self.note(Kind::DirectJsonInput, item.ident.span());
        }
        self.note_type_declaration(&item.ident, &item.ty, generic_parameters(&item.generics));
        syn::visit::visit_item_type(self, item);
    }

    fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
        let map_shaped = match &item.fields {
            syn::Fields::Named(fields) => !fields.named.is_empty(),
            syn::Fields::Unnamed(fields) => fields.unnamed.len() > 1,
            syn::Fields::Unit => false,
        };
        let is_external_protocol = external_capture_allowed(&self.file, &item.ident.to_string());
        let captures_external =
            is_external_protocol && captures_external_fields(&item.attrs, &item.fields);
        let shape = if captures_external {
            DeserializationShape::ExternalCapture
        } else if map_shaped {
            DeserializationShape::Map
        } else {
            DeserializationShape::Scalar
        };
        if open_deserialization(&item.attrs, &self.aliases, shape) {
            self.note(Kind::OpenDeserialization, item.ident.span());
        }
        if is_external_protocol
            && derives_deserialize_in(&item.attrs, &self.aliases)
            && !captures_external
        {
            self.note(Kind::OpenDeserialization, item.ident.span());
        }
        if struct_has_permissive_input(item, &self.aliases, &self.file) {
            self.note(Kind::OpenDeserialization, item.ident.span());
        }
        let parameters = generic_parameters(&item.generics);
        if !parameters.is_empty()
            && item
                .fields
                .iter()
                .any(|field| self.type_forwards_owned(&field.ty, &parameters))
        {
            self.note(Kind::OwnedTraitObject, item.ident.span());
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_enum(&mut self, item: &syn::ItemEnum) {
        let map_shaped = item.variants.iter().any(|variant| match &variant.fields {
            syn::Fields::Named(fields) => !fields.named.is_empty(),
            syn::Fields::Unnamed(fields) => fields.unnamed.len() > 1,
            syn::Fields::Unit => false,
        });
        let shape = if map_shaped {
            DeserializationShape::Map
        } else {
            DeserializationShape::Scalar
        };
        if open_deserialization(&item.attrs, &self.aliases, shape) {
            self.note(Kind::OpenDeserialization, item.ident.span());
        }
        if enum_has_permissive_input(item, &self.aliases, &self.file) {
            self.note(Kind::OpenDeserialization, item.ident.span());
        }
        let derived = item
            .attrs
            .iter()
            .any(|attribute| derives_default(attribute, &self.aliases));
        let marked = item
            .variants
            .iter()
            .any(|variant| variant.attrs.iter().any(default_marker));
        if derived || marked {
            self.note(Kind::DerivedEnumDefault, item.ident.span());
        }
        let parameters = generic_parameters(&item.generics);
        if !parameters.is_empty()
            && item
                .variants
                .iter()
                .flat_map(|variant| &variant.fields)
                .any(|field| self.type_forwards_owned(&field.ty, &parameters))
        {
            self.note(Kind::OwnedTraitObject, item.ident.span());
        }
        syn::visit::visit_item_enum(self, item);
    }

    fn visit_item_union(&mut self, item: &syn::ItemUnion) {
        let parameters = generic_parameters(&item.generics);
        if !parameters.is_empty()
            && item
                .fields
                .named
                .iter()
                .any(|field| self.type_forwards_owned(&field.ty, &parameters))
        {
            self.note(Kind::OwnedTraitObject, item.ident.span());
        }
        syn::visit::visit_item_union(self, item);
    }

    fn visit_item_impl(&mut self, item: &syn::ItemImpl) {
        if let Some((path, _for)) = &item.trait_
            && let Some(segment) = path.segments.last()
        {
            let name = segment.ident.to_string();
            let erases = name == "Deref"
                || (["AsRef", "Borrow", "Into"].contains(&name.as_str())
                    && type_arguments(segment).iter().any(|held| scalar_type(held)))
                || (name == "From"
                    && scalar_type(&item.self_ty)
                    && !type_arguments(segment).is_empty());
            if erases {
                self.note(Kind::ImplicitScalarErasure, segment.ident.span());
            }
        }
        if let Some((path, _for)) = &item.trait_
            && self.aliases.default_derive(path)
            && implemented_type_name(&item.self_ty)
                .is_none_or(|name| !manual_default_allowed(&self.file, &name))
        {
            let at = path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::SemanticDefault, at);
        }
        if let Some((path, _for)) = &item.trait_
            && let Some(segment) = path.segments.last()
            && self.aliases.is_from_trait(&segment.ident.to_string())
            && type_arguments(segment)
                .first()
                .is_some_and(|held| self.aliases.unit(held))
        {
            self.note(Kind::UnitDomainConversion, segment.ident.span());
        }
        let outer = generic_parameters(&item.generics);
        for associated in &item.items {
            if let syn::ImplItem::Type(associated) = associated {
                let mut parameters = outer.clone();
                parameters.extend(generic_parameters(&associated.generics));
                self.note_type_declaration(&associated.ident, &associated.ty, parameters);
            }
        }
        syn::visit::visit_item_impl(self, item);
    }

    fn visit_trait_item_type(&mut self, item: &syn::TraitItemType) {
        if let Some((_equals, held)) = &item.default {
            self.note_type_declaration(&item.ident, held, generic_parameters(&item.generics));
        }
        syn::visit::visit_trait_item_type(self, item);
    }

    fn visit_item_use(&mut self, item: &syn::ItemUse) {
        if let Some(span) = use_tree_name_span(&item.tree, "cfg") {
            self.note(Kind::VacuousCfg, span);
        }
        self.glob_imports(&item.tree, &mut Vec::new());
        self.renamed_aliases(&item.tree);
        self.scan_erasure_imports(&item.tree);
        self.scan_concurrency_imports(&item.tree);
        self.scan_arithmetic_imports(&item.tree);
        self.scan_lossy_text_imports(&item.tree);
        self.scan_forget_imports(&item.tree);
        self.scan_json_imports(item);
        syn::visit::visit_item_use(self, item);
    }

    fn visit_item_extern_crate(&mut self, item: &syn::ItemExternCrate) {
        if !self.has_policy(SourcePolicy::StrictJsonReader)
            && item.ident == "serde_json"
            && item.rename.is_some()
        {
            self.note(Kind::DirectJsonInput, item.ident.span());
        }
        syn::visit::visit_item_extern_crate(self, item);
    }

    fn visit_expr_for_loop(&mut self, loop_: &syn::ExprForLoop) {
        if pattern_names_variant(&loop_.pat, "Ok") {
            self.note(Kind::DiscardedResult, loop_.for_token.span);
        }
        self.within_a_loop(|scan| syn::visit::visit_expr_for_loop(scan, loop_));
    }

    fn visit_expr_while(&mut self, loop_: &syn::ExprWhile) {
        if let syn::Expr::Let(let_) = loop_.cond.as_ref()
            && pattern_names_variant(&let_.pat, "Ok")
        {
            self.note(Kind::DiscardedResult, loop_.while_token.span);
        }
        self.within_a_loop(|scan| syn::visit::visit_expr_while(scan, loop_));
    }

    fn visit_expr_if(&mut self, if_: &syn::ExprIf) {
        for span in result_let_patterns(&if_.cond) {
            self.note(Kind::DiscardedResult, span);
        }
        syn::visit::visit_expr_if(self, if_);
    }

    fn visit_expr_loop(&mut self, loop_: &syn::ExprLoop) {
        self.within_a_loop(|scan| syn::visit::visit_expr_loop(scan, loop_));
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        self.scan_concurrency_call(call);
        self.scan_result_call(call);
        self.scan_repository_call(call);
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if call.method == "spawn" && !self.raw_spawn_boundary(call.method.span()) {
            self.note(Kind::UnownedSpawn, call.method.span());
        }
        if call.method == "clear_poison"
            || (["into_inner", "get_ref", "get_mut"].contains(&call.method.to_string().as_str())
                && expression_names_poison(&call.receiver))
        {
            self.note(Kind::PoisonRecovery, call.method.span());
        }
        if wrapping_counter_method(&call.method) {
            self.note(Kind::WrappingCounter, call.method.span());
        }
        if call.method == "from_utf8_lossy" || call.method == "to_string_lossy" {
            self.note(Kind::LossyText, call.method.span());
        }
        if self.has_policy(SourcePolicy::OverflowSensitive)
            && (overflow_method(&call.method)
                || (call.method == "unwrap_or" && call.args.iter().any(fabricated_fallback)))
        {
            self.note(Kind::FabricatedOverflow, call.method.span());
        }
        if !self.has_policy(SourcePolicy::StrictJsonReader)
            && call.method == "parse"
            && call.turbofish.as_ref().is_none_or(|arguments| {
                arguments.args.iter().any(|argument| {
                    matches!(argument, syn::GenericArgument::Type(type_) if self.aliases.json_value(type_))
                }) || arguments.args.is_empty()
            })
        {
            self.note(Kind::DirectJsonInput, call.method.span());
        }
        if ["unwrap_or_else", "map_or_else", "or_else"].contains(&call.method.to_string().as_str())
            && call.args.first().is_some_and(closure_ignores_input)
        {
            self.note(Kind::DiscardedResult, call.method.span());
        }
        if call.method == "ok" || call.method == "err" {
            self.note(Kind::DiscardedResult, call.method.span());
        }
        if (call.method == "filter_map" || call.method == "map_while")
            && call.args.iter().any(result_to_option)
        {
            self.note(Kind::DiscardedResult, call.method.span());
        }
        if call.method == "flat_map" && call.args.iter().any(result_into_iter) {
            self.note(Kind::DiscardedResult, call.method.span());
        }
        if call.method == "flatten" {
            self.note(Kind::DiscardedResult, call.method.span());
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_cast(&mut self, cast: &syn::ExprCast) {
        if self.has_policy(SourcePolicy::StrictConversions) {
            self.note(Kind::UncheckedCast, cast.as_token.span);
        }
        syn::visit::visit_expr_cast(self, cast);
    }

    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if let Some(segment) = path
            .path
            .segments
            .iter()
            .find(|segment| segment.ident == "ManuallyDrop")
        {
            self.note(Kind::ForgottenValue, segment.ident.span());
        }
        if poison_recovery_path(&path.path) {
            self.note(
                Kind::PoisonRecovery,
                path.path
                    .segments
                    .last()
                    .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span()),
            );
        }
        if !self.has_policy(SourcePolicy::StrictJsonReader)
            && direct_json_path(&path.path, path.qself.as_ref(), &self.aliases)
        {
            let span = path
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::DirectJsonInput, span);
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, macro_: &syn::Macro) {
        self.scan_macro_runtime(macro_);
        self.scan_macro_json(macro_);
        self.scan_macro_attributes(macro_);
        self.scan_macro_types(macro_);
        self.scan_macro_computations(macro_);
        syn::visit::visit_macro(self, macro_);
    }

    fn visit_attribute(&mut self, attribute: &syn::Attribute) {
        if opaque_sensitive_meta(&attribute.meta) {
            self.note(Kind::OpaqueMacroSyntax, attribute.pound_token.span);
        }
        self.note_each(Kind::AllowAttribute, allow_attribute_spans(&attribute.meta));
        self.note_each(Kind::VacuousCfg, vacuous_cfg_spans(&attribute.meta));
        if let syn::Meta::List(list) = &attribute.meta {
            for span in owned_trait_objects_in_tokens(&list.tokens, &self.aliases) {
                self.note(Kind::OwnedTraitObject, span);
            }
            for span in unit_domain_conversions_in_tokens(&list.tokens, &self.aliases) {
                self.note(Kind::UnitDomainConversion, span);
            }
            for span in unit_aliases_in_tokens(&list.tokens, &self.aliases) {
                self.note(Kind::UnitDomainConversion, span);
            }
        }
        syn::visit::visit_attribute(self, attribute);
    }

    fn visit_type_path(&mut self, path: &syn::TypePath) {
        if let Some(segment) = path
            .path
            .segments
            .iter()
            .find(|segment| segment.ident == "ManuallyDrop")
        {
            self.note(Kind::ForgottenValue, segment.ident.span());
        }
        if self.aliases.tri_state_bool(&syn::Type::Path(path.clone())) {
            let span = path
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::TriStateBool, span);
        }
        if !self.has_policy(SourcePolicy::StrictJsonReader) && json_deserializer_path(&path.path) {
            let span = path
                .path
                .segments
                .last()
                .map_or_else(proc_macro2::Span::call_site, |segment| segment.ident.span());
            self.note(Kind::DirectJsonInput, span);
        }
        if let Some(segment) = path.path.segments.last()
            && type_arguments(segment)
                .iter()
                .any(|held| contains_unborrowed_trait_object(held))
        {
            self.note(Kind::OwnedTraitObject, segment.ident.span());
        }
        if let Some(segment) = path.path.segments.last()
            && self.aliases.owned(&segment.ident.to_string())
            && type_arguments(segment)
                .iter()
                .any(|held| self.aliases.trait_object(held))
        {
            self.note(Kind::OwnedTraitObject, segment.ident.span());
        }
        if let Some(segment) = path.path.segments.last()
            && self.aliases.result(&segment.ident.to_string())
            && type_arguments(segment)
                .get(1)
                .is_some_and(|held| self.aliases.string_error(held))
        {
            self.note(Kind::StringError, segment.ident.span());
        }
        if let Some(segment) = path.path.segments.last()
            && self.aliases.result(&segment.ident.to_string())
            && type_arguments(segment)
                .get(1)
                .is_some_and(|held| self.aliases.unit(held))
        {
            self.note(Kind::UnitError, segment.ident.span());
        }
        syn::visit::visit_type_path(self, path);
    }
}

fn contains_unborrowed_trait_object(type_: &syn::Type) -> bool {
    match unwrapped(type_) {
        syn::Type::TraitObject(_) => true,
        syn::Type::Reference(reference) => {
            !matches!(unwrapped(&reference.elem), syn::Type::TraitObject(_))
                && contains_unborrowed_trait_object(&reference.elem)
        }
        syn::Type::Path(path) => path
            .path
            .segments
            .iter()
            .flat_map(type_arguments)
            .any(contains_unborrowed_trait_object),
        syn::Type::Array(array) => contains_unborrowed_trait_object(&array.elem),
        syn::Type::Slice(slice) => contains_unborrowed_trait_object(&slice.elem),
        syn::Type::Tuple(tuple) => tuple.elems.iter().any(contains_unborrowed_trait_object),
        syn::Type::Ptr(pointer) => contains_unborrowed_trait_object(&pointer.elem),
        syn::Type::FnPtr(function) => {
            function
                .inputs
                .iter()
                .any(|input| contains_unborrowed_trait_object(&input.ty))
                || matches!(&function.output,
                    syn::ReturnType::Type(_, returned)
                        if contains_unborrowed_trait_object(returned))
        }
        syn::Type::ImplTrait(impl_) => impl_.bounds.iter().any(|bound| {
            matches!(bound, syn::TypeParamBound::Trait(trait_) if trait_.path.segments.iter().flat_map(type_arguments).any(contains_unborrowed_trait_object))
        }),
        syn::Type::Group(_)
        | syn::Type::Paren(_)
        | syn::Type::Infer(_)
        | syn::Type::Macro(_)
        | syn::Type::Never(_)
        | syn::Type::Verbatim(_)
        | _ => false,
    }
}

impl Scan {
    fn glob_imports(&mut self, tree: &syn::UseTree, path: &mut Vec<String>) {
        match tree {
            syn::UseTree::Path(one) => {
                let depth = path.len();
                path.push(one.ident.to_string());
                self.glob_imports(&one.tree, path);
                path.truncate(depth);
            }
            syn::UseTree::Name(_) | syn::UseTree::Rename(_) => {}
            syn::UseTree::Glob(star) => {
                if path.last().is_none_or(|segment| segment != "prelude") {
                    self.note(Kind::GlobImport, star.star_token.span);
                }
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.glob_imports(item, path);
                }
            }
        }
    }

    fn renamed_aliases(&mut self, tree: &syn::UseTree) {
        match tree {
            syn::UseTree::Path(one) => self.renamed_aliases(&one.tree),
            syn::UseTree::Rename(one) => {
                let local = one.rename.to_string();
                if let Some(meaning) = self.aliases.types.get(&local).copied() {
                    self.note(alias_kind(meaning), one.rename.span());
                }
                if self.aliases.default_derives.contains(&local) {
                    self.note(Kind::DefaultDeriveAlias, one.rename.span());
                }
                if local != "_" && self.aliases.deserialize_derives.contains(&local) {
                    self.note(Kind::DeserializeDeriveAlias, one.rename.span());
                }
                if local != "_" && self.aliases.drop_functions.contains(&local) {
                    self.note(Kind::DroppedComputation, one.rename.span());
                }
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    self.renamed_aliases(item);
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Glob(_) => {}
        }
    }
}

const fn alias_kind(meaning: TypeMeaning) -> Kind {
    match meaning {
        TypeMeaning::StringError | TypeMeaning::TextOwner => Kind::StringAlias,
        TypeMeaning::TraitObject => Kind::TraitObjectAlias,
        TypeMeaning::Owned => Kind::OwnedPointerAlias,
        TypeMeaning::Result => Kind::ResultAlias,
        TypeMeaning::Unit => Kind::UnitDomainConversion,
    }
}

fn result_method(expression: &syn::Expr, wanted: &str) -> bool {
    let syn::Expr::Path(path) = expression else {
        return false;
    };
    result_path_method(&path.path, wanted)
}

fn result_path_method(path: &syn::Path, wanted: &str) -> bool {
    let mut segments = path.segments.iter().rev();
    segments
        .next()
        .is_some_and(|segment| segment.ident == wanted)
        && segments
            .next()
            .is_some_and(|segment| segment.ident == "Result")
}

fn overflow_method(name: &syn::Ident) -> bool {
    matches!(
        name.to_string().as_str(),
        "saturating_add" | "saturating_sub" | "saturating_mul"
    )
}

fn wrapping_counter_method(name: &syn::Ident) -> bool {
    matches!(
        name.to_string().as_str(),
        "fetch_add"
            | "fetch_sub"
            | "wrapping_add"
            | "wrapping_sub"
            | "wrapping_mul"
            | "wrapping_div"
            | "wrapping_rem"
            | "wrapping_pow"
            | "wrapping_shl"
            | "wrapping_shr"
            | "wrapping_neg"
    )
}

fn fabricated_fallback(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Group(group) => fabricated_fallback(&group.expr),
        syn::Expr::Paren(paren) => fabricated_fallback(&paren.expr),
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Int(integer),
            ..
        }) => integer.base10_digits() == "0",
        syn::Expr::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "MAX" || segment.ident == "MIN"),
        _ => false,
    }
}

fn closure_ignores_input(expression: &syn::Expr) -> bool {
    let syn::Expr::Closure(closure) = expression else {
        return false;
    };
    if closure.inputs.len() != 1 {
        return false;
    }
    let mut bindings = ClosureBindings::default();
    for input in &closure.inputs {
        bindings.visit_pat(input);
    }
    if bindings.names.is_empty() {
        return true;
    }
    let mut uses = ClosureUses {
        names: &bindings.names,
        found: false,
    };
    uses.visit_expr(&closure.body);
    !uses.found
}

#[derive(Default)]
struct ClosureBindings {
    names: BTreeSet<String>,
}

impl Visit<'_> for ClosureBindings {
    fn visit_pat_ident(&mut self, pattern: &syn::PatIdent) {
        self.names.insert(pattern.ident.to_string());
        syn::visit::visit_pat_ident(self, pattern);
    }
}

struct ClosureUses<'a> {
    names: &'a BTreeSet<String>,
    found: bool,
}

impl Visit<'_> for ClosureUses<'_> {
    fn visit_expr_path(&mut self, path: &syn::ExprPath) {
        if path
            .path
            .get_ident()
            .is_some_and(|ident| self.names.contains(&ident.to_string()))
        {
            self.found = true;
            return;
        }
        syn::visit::visit_expr_path(self, path);
    }

    fn visit_macro(&mut self, macro_: &syn::Macro) {
        if tokens_name_one_of(&macro_.tokens, self.names) {
            self.found = true;
            return;
        }
        syn::visit::visit_macro(self, macro_);
    }
}

fn tokens_name_one_of(tokens: &proc_macro2::TokenStream, names: &BTreeSet<String>) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        proc_macro2::TokenTree::Ident(ident) => names.contains(&ident.to_string()),
        proc_macro2::TokenTree::Group(group) => tokens_name_one_of(&group.stream(), names),
        proc_macro2::TokenTree::Literal(literal) => {
            syn::parse_str::<syn::LitStr>(&literal.to_string())
                .is_ok_and(|literal| format_string_names_one_of(&literal.value(), names))
        }
        proc_macro2::TokenTree::Punct(_) => false,
    })
}

fn format_string_names_one_of(format: &str, names: &BTreeSet<String>) -> bool {
    let mut characters = format.chars().peekable();
    while let Some(character) = characters.next() {
        if character != '{' {
            continue;
        }
        if characters.next_if_eq(&'{').is_some() {
            continue;
        }
        let mut name = String::new();
        while let Some(character) =
            characters.next_if(|candidate| candidate.is_ascii_alphanumeric() || *candidate == '_')
        {
            name.push(character);
        }
        if !name.is_empty()
            && characters
                .peek()
                .is_some_and(|next| matches!(next, '}' | ':'))
            && names.contains(&name)
        {
            return true;
        }
    }
    false
}

fn result_to_option(expression: &syn::Expr) -> bool {
    result_method(expression, "ok")
}

fn result_into_iter(expression: &syn::Expr) -> bool {
    result_method(expression, "into_iter")
}

/// The binding that suppresses the compiler's unused-value diagnostics, if a
/// pattern has one. A wildcard is the most direct spelling; a named binding
/// beginning with `_` also opts out of the diagnostic and can be nested in a
/// destructuring pattern.
fn ignored_binding_span(pattern: &syn::Pat) -> Option<proc_macro2::Span> {
    if let syn::Pat::Wild(wild) = pattern {
        return Some(wild.underscore_token.span);
    }
    ignored_named_binding_span(pattern)
}

/// Finds a named opt-out anywhere in a destructuring pattern. A nested
/// wildcard discards one component after the initializer itself was consumed;
/// it does not suppress the initializer's `must_use` diagnostic as `let _ =`
/// does.
fn ignored_named_binding_span(pattern: &syn::Pat) -> Option<proc_macro2::Span> {
    match pattern {
        syn::Pat::Ident(ident) if ident.ident.to_string().starts_with('_') => {
            Some(ident.ident.span())
        }
        syn::Pat::Or(or) => or.cases.iter().find_map(ignored_named_binding_span),
        syn::Pat::Paren(paren) => ignored_named_binding_span(&paren.pat),
        syn::Pat::Reference(reference) => ignored_named_binding_span(&reference.pat),
        syn::Pat::Slice(slice) => slice.elems.iter().find_map(ignored_named_binding_span),
        syn::Pat::Struct(struct_) => struct_
            .fields
            .iter()
            .find_map(|field| ignored_named_binding_span(&field.pat)),
        syn::Pat::Tuple(tuple) => tuple.elems.iter().find_map(ignored_named_binding_span),
        syn::Pat::TupleStruct(tuple) => tuple.elems.iter().find_map(ignored_named_binding_span),
        syn::Pat::Type(type_) => ignored_named_binding_span(&type_.pat),
        _ => None,
    }
}

/// Whether an initializer performs work whose `must_use` result an ignored
/// binding could hide. Wrappers preserve that property; a path or literal
/// does not become work merely by being ignored.
fn ignored_computation(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Call(_) | syn::Expr::MethodCall(_) | syn::Expr::Macro(_) => true,
        syn::Expr::Await(await_) => ignored_computation(&await_.base),
        syn::Expr::Group(group) => ignored_computation(&group.expr),
        syn::Expr::Paren(paren) => ignored_computation(&paren.expr),
        syn::Expr::Try(try_) => ignored_computation(&try_.expr),
        _ => false,
    }
}

#[derive(Default)]
struct IgnoredBindings {
    found: Vec<proc_macro2::Span>,
}

impl Visit<'_> for IgnoredBindings {
    fn visit_local(&mut self, local: &syn::Local) {
        if let Some(init) = &local.init
            && ignored_computation(&init.expr)
            && let Some(span) = ignored_binding_span(&local.pat)
        {
            self.found.push(span);
        }
        syn::visit::visit_local(self, local);
    }
}

fn ignored_computations_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    ignored_computations_in_tokens_with(tokens, &mut found);
    found
}

fn ignored_computations_in_tokens_with(
    tokens: &proc_macro2::TokenStream,
    found: &mut Vec<proc_macro2::Span>,
) {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for tree in &trees {
        let proc_macro2::TokenTree::Group(group) = tree else {
            continue;
        };
        if group.delimiter() == proc_macro2::Delimiter::Brace {
            let wrapped: proc_macro2::TokenStream =
                std::iter::once(proc_macro2::TokenTree::Group(group.clone())).collect();
            match syn::parse2::<syn::Block>(wrapped) {
                Ok(block) => {
                    let mut ignored = IgnoredBindings::default();
                    ignored.visit_block(&block);
                    found.append(&mut ignored.found);
                }
                Err(_not_a_literal_block) => {
                    if let Some(span) = opaque_ignored_binding(&group.stream()) {
                        found.push(span);
                    }
                }
            }
        }
        ignored_computations_in_tokens_with(&group.stream(), found);
    }
}

/// An unparsable macro body that can choose the binding and initializer at
/// expansion time is rejected rather than assumed to produce a used value.
fn opaque_ignored_binding(tokens: &proc_macro2::TokenStream) -> Option<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(let_) = tree else {
            continue;
        };
        if let_ != "let" {
            continue;
        }
        if matches!(
            trees.get(..at).and_then(|before| before.last()),
            Some(proc_macro2::TokenTree::Ident(prefix)) if prefix == "if" || prefix == "while"
        ) {
            continue;
        }
        let ignored = match trees.get(at.checked_add(1)?) {
            Some(proc_macro2::TokenTree::Ident(name)) => name.to_string().starts_with('_'),
            Some(proc_macro2::TokenTree::Punct(dollar)) => dollar.as_char() == '$',
            _ => false,
        };
        let Some(after_binding) = at.checked_add(2).and_then(|start| trees.get(start..)) else {
            continue;
        };
        if ignored
            && after_binding.iter().any(|token| {
                matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '=')
            })
        {
            return Some(let_.span());
        }
    }
    None
}

/// Whether `drop(expression)` computes a fresh value instead of ending the
/// lifetime of a value already named by the program.
fn dropped_computation(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Group(group) => dropped_computation(&group.expr),
        syn::Expr::Paren(paren) => dropped_computation(&paren.expr),
        syn::Expr::Path(_) => false,
        syn::Expr::Field(field) => dropped_computation(&field.base),
        _ => true,
    }
}

fn dropped_computations_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let mut drop_names = aliases.drop_functions.clone();
    drop_names.insert("drop".to_owned());
    macro_drop_aliases(tokens, &mut drop_names);
    let mut found = Vec::new();
    dropped_computations_in_tokens_with(tokens, &drop_names, &mut found);
    found
}

fn macro_drop_aliases(tokens: &proc_macro2::TokenStream, names: &mut BTreeSet<String>) {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            macro_drop_aliases(&group.stream(), names);
        }
        let proc_macro2::TokenTree::Ident(source) = token else {
            continue;
        };
        let Some(proc_macro2::TokenTree::Ident(as_)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        let Some(proc_macro2::TokenTree::Ident(local)) = trees.get(at.saturating_add(2)) else {
            continue;
        };
        if as_ == "as" && (source == "drop" || names.contains(&source.to_string())) {
            names.insert(local.to_string());
        }
    }
}

fn dropped_computations_in_tokens_with(
    tokens: &proc_macro2::TokenStream,
    drop_names: &BTreeSet<String>,
    found: &mut Vec<proc_macro2::Span>,
) {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            dropped_computations_in_tokens_with(&group.stream(), drop_names, found);
        }

        let invoked_metavariable = trees
            .get(..at)
            .and_then(|before| before.last())
            .is_some_and(|tree| matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '$'));
        let proc_macro2::TokenTree::Ident(function) = token else {
            continue;
        };
        if !invoked_metavariable && !drop_names.contains(&function.to_string()) {
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(arguments)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        if arguments.delimiter() != proc_macro2::Delimiter::Parenthesis {
            continue;
        }
        let is_computation = match syn::parse2::<syn::Expr>(arguments.stream()) {
            Ok(expression) => dropped_computation(&expression),
            Err(_opaque_argument) => true,
        };
        if is_computation {
            found.push(function.span());
        }
    }
}

fn open_deserializations_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
    strict_input: bool,
) -> Vec<proc_macro2::Span> {
    let mut local = aliases.deserialize_derives.clone();
    macro_renamed_identifiers(tokens, "Deserialize", &mut local);
    let mut metas = Vec::new();
    macro_attribute_metas(tokens, &mut metas);
    let derived = metas
        .iter()
        .find(|(meta, _span)| meta_derives_named(meta, aliases, &local));
    let Some((_derive, span)) = derived else {
        return Vec::new();
    };
    let map_shaped = macro_tokens_name(tokens, "struct") || macro_tokens_name(tokens, "enum");
    let tagged = metas
        .iter()
        .any(|(meta, _span)| meta_has_serde_option(meta, "tag"));
    let transparent = metas
        .iter()
        .any(|(meta, _span)| meta_has_serde_option(meta, "transparent"));
    let closed = metas
        .iter()
        .any(|(meta, _span)| meta_has_serde_option(meta, "deny_unknown_fields"));
    let permissive = strict_input
        && PERMISSIVE_INPUT_OPTIONS.iter().any(|option| {
            metas
                .iter()
                .any(|(meta, _span)| meta_has_serde_option(meta, option))
        });
    if ((map_shaped || tagged) && !transparent && !closed) || permissive {
        vec![*span]
    } else {
        Vec::new()
    }
}

fn macro_renamed_identifiers(
    tokens: &proc_macro2::TokenStream,
    original: &str,
    names: &mut BTreeSet<String>,
) {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            macro_renamed_identifiers(&group.stream(), original, names);
        }
        let proc_macro2::TokenTree::Ident(source) = token else {
            continue;
        };
        let Some(proc_macro2::TokenTree::Ident(as_)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        let Some(proc_macro2::TokenTree::Ident(local)) = trees.get(at.saturating_add(2)) else {
            continue;
        };
        if as_ == "as" && (source == original || names.contains(&source.to_string())) {
            names.insert(local.to_string());
        }
    }
}

fn renamed_identifiers_in_tokens(
    tokens: &proc_macro2::TokenStream,
    original: &str,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(renamed_identifiers_in_tokens(&group.stream(), original));
        }
        let proc_macro2::TokenTree::Ident(source) = token else {
            continue;
        };
        let Some(proc_macro2::TokenTree::Ident(as_)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        let Some(proc_macro2::TokenTree::Ident(local)) = trees.get(at.saturating_add(2)) else {
            continue;
        };
        if source == original && as_ == "as" && local != "_" {
            found.push(local.span());
        }
    }
    found
}

fn unchecked_cast_spans_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    for token in tokens.clone() {
        match token {
            proc_macro2::TokenTree::Group(group) => {
                found.extend(unchecked_cast_spans_in_tokens(&group.stream()));
            }
            proc_macro2::TokenTree::Ident(ident) if ident == "as" => found.push(ident.span()),
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

fn opaque_macro_attribute_spans(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(opaque_macro_attribute_spans(&group.stream()));
        }
        if !matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#') {
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(group)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        if group.delimiter() != proc_macro2::Delimiter::Bracket {
            continue;
        }
        match syn::parse2::<syn::Meta>(group.stream()) {
            Ok(meta) if opaque_sensitive_meta(&meta) => found.push(group.span()),
            Ok(_) => {}
            Err(_unrecognised_generated_attribute) => found.push(group.span()),
        }
    }
    found
}

fn opaque_sensitive_meta(meta: &syn::Meta) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_err();
    }
    if list.path.is_ident("serde") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated,
            )
            .is_err();
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => nested.iter().skip(1).any(opaque_sensitive_meta),
        Err(_unrecognised_generated_cfg_attr) => true,
    }
}

fn allow_attribute_spans(meta: &syn::Meta) -> Vec<proc_macro2::Span> {
    if meta.path().is_ident("allow") {
        return meta
            .path()
            .segments
            .first()
            .map(|segment| vec![segment.ident.span()])
            .unwrap_or_default();
    }
    let syn::Meta::List(list) = meta else {
        return Vec::new();
    };
    if !list.path.is_ident("cfg_attr") {
        return Vec::new();
    }
    match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => nested
            .iter()
            .skip(1)
            .flat_map(allow_attribute_spans)
            .collect(),
        Err(_opaque_cfg_attr) => Vec::new(),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CfgTruth {
    Always,
    Never,
    Variable,
}

fn cfg_constant(meta: &syn::Meta) -> CfgTruth {
    let syn::Meta::List(list) = meta else {
        return CfgTruth::Variable;
    };
    let arguments = match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(arguments) => arguments,
        Err(_opaque_condition) => return CfgTruth::Variable,
    };
    if list.path.is_ident("all") {
        let mut variable = false;
        for argument in &arguments {
            match cfg_constant(argument) {
                CfgTruth::Never => return CfgTruth::Never,
                CfgTruth::Variable => variable = true,
                CfgTruth::Always => {}
            }
        }
        return if variable {
            CfgTruth::Variable
        } else {
            CfgTruth::Always
        };
    }
    if list.path.is_ident("any") {
        let mut variable = false;
        for argument in &arguments {
            match cfg_constant(argument) {
                CfgTruth::Always => return CfgTruth::Always,
                CfgTruth::Variable => variable = true,
                CfgTruth::Never => {}
            }
        }
        return if variable {
            CfgTruth::Variable
        } else {
            CfgTruth::Never
        };
    }
    if !list.path.is_ident("not") || arguments.len() != 1 {
        return CfgTruth::Variable;
    }
    match arguments.first().map(cfg_constant) {
        Some(CfgTruth::Always) => CfgTruth::Never,
        Some(CfgTruth::Never) => CfgTruth::Always,
        Some(CfgTruth::Variable) | None => CfgTruth::Variable,
    }
}

fn vacuous_cfg_spans(meta: &syn::Meta) -> Vec<proc_macro2::Span> {
    let syn::Meta::List(list) = meta else {
        return Vec::new();
    };
    if list.path.is_ident("cfg") {
        return match list.parse_args::<syn::Meta>() {
            Ok(condition) if cfg_constant(&condition) != CfgTruth::Variable => condition
                .path()
                .segments
                .first()
                .map(|segment| vec![segment.ident.span()])
                .unwrap_or_default(),
            Ok(_) | Err(_) => Vec::new(),
        };
    }
    if !list.path.is_ident("cfg_attr") {
        return Vec::new();
    }
    let nested = match list
        .parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
    {
        Ok(nested) => nested,
        Err(_opaque_cfg_attr) => return Vec::new(),
    };
    let mut found = Vec::new();
    if let Some(condition) = nested.first()
        && cfg_constant(condition) != CfgTruth::Variable
        && let Some(segment) = condition.path().segments.first()
    {
        found.push(segment.ident.span());
    }
    for attribute in nested.iter().skip(1) {
        found.extend(vacuous_cfg_spans(attribute));
    }
    found
}

fn vacuous_cfg_macros_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for tree in &trees {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(vacuous_cfg_macros_in_tokens(&group.stream()));
        }
    }
    for window in trees.windows(3) {
        let [
            proc_macro2::TokenTree::Ident(cfg),
            proc_macro2::TokenTree::Punct(bang),
            proc_macro2::TokenTree::Group(arguments),
        ] = window
        else {
            continue;
        };
        if cfg != "cfg"
            || bang.as_char() != '!'
            || arguments.delimiter() != proc_macro2::Delimiter::Parenthesis
        {
            continue;
        }
        match syn::parse2::<syn::Meta>(arguments.stream()) {
            Ok(condition) if cfg_constant(&condition) != CfgTruth::Variable => {
                found.push(cfg.span());
            }
            Ok(_) | Err(_) => {}
        }
    }
    found
}

fn macro_attribute_metas(
    tokens: &proc_macro2::TokenStream,
    found: &mut Vec<(syn::Meta, proc_macro2::Span)>,
) {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            macro_attribute_metas(&group.stream(), found);
        }
        if !matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#') {
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(group)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        if group.delimiter() != proc_macro2::Delimiter::Bracket {
            continue;
        }
        match syn::parse2::<syn::Meta>(group.stream()) {
            Ok(meta) => found.push((meta, group.span())),
            Err(_unrecognised_generated_attribute) => {}
        }
    }
}

fn meta_derives_named(meta: &syn::Meta, aliases: &Aliases, local: &BTreeSet<String>) -> bool {
    let syn::Meta::List(list) = meta else {
        return false;
    };
    if list.path.is_ident("derive") {
        return list
            .parse_args_with(
                syn::punctuated::Punctuated::<syn::Path, syn::Token![,]>::parse_terminated,
            )
            .is_ok_and(|paths| {
                paths.iter().any(|path| {
                    aliases.deserialize_derive(path)
                        || path
                            .segments
                            .last()
                            .is_some_and(|segment| local.contains(&segment.ident.to_string()))
                })
            });
    }
    if !list.path.is_ident("cfg_attr") {
        return false;
    }
    list.parse_args_with(syn::punctuated::Punctuated::<syn::Meta, syn::Token![,]>::parse_terminated)
        .is_ok_and(|nested| {
            nested
                .iter()
                .skip(1)
                .any(|held| meta_derives_named(held, aliases, local))
        })
}

fn result_let_patterns(expression: &syn::Expr) -> Vec<proc_macro2::Span> {
    let mut finder = ResultLetPatterns { found: Vec::new() };
    finder.visit_expr(expression);
    finder.found
}

struct ResultLetPatterns {
    found: Vec<proc_macro2::Span>,
}

impl Visit<'_> for ResultLetPatterns {
    fn visit_expr_let(&mut self, let_: &syn::ExprLet) {
        if pattern_names_variant(&let_.pat, "Ok") {
            self.found.push(let_.let_token.span);
        }
        syn::visit::visit_expr_let(self, let_);
    }
}

fn ignored_result_if_lets_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    match syn::parse2::<syn::Expr>(tokens.clone()) {
        Ok(expression) => {
            let mut finder = IfResultLetPatterns { found: Vec::new() };
            finder.visit_expr(&expression);
            return finder.found;
        }
        Err(_not_a_complete_expression) => {}
    }
    match syn::parse2::<syn::Block>(tokens.clone()) {
        Ok(block) => {
            let mut finder = IfResultLetPatterns { found: Vec::new() };
            finder.visit_block(&block);
            return finder.found;
        }
        Err(_not_a_complete_block) => {}
    }

    let mut found = opaque_result_if_let_spans(tokens);
    for token in tokens.clone() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(ignored_result_if_lets_in_tokens(&group.stream()));
        }
    }
    found
}

fn opaque_result_if_let_spans(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    let mut after_if = false;
    let mut after_let = None;
    for token in &trees {
        match token {
            proc_macro2::TokenTree::Ident(ident) if ident == "if" => {
                after_if = true;
                after_let = None;
            }
            proc_macro2::TokenTree::Ident(ident) if after_if && ident == "let" => {
                after_let = Some(ident.span());
            }
            proc_macro2::TokenTree::Ident(ident) if after_let.is_some() && ident == "Ok" => {
                found.push(ident.span());
                after_if = false;
                after_let = None;
            }
            proc_macro2::TokenTree::Punct(punct)
                if after_let.is_some() && punct.as_char() == '$' =>
            {
                found.push(after_let.unwrap_or_else(|| punct.span()));
                after_if = false;
                after_let = None;
            }
            proc_macro2::TokenTree::Group(group)
                if after_if && group.delimiter() == proc_macro2::Delimiter::Brace =>
            {
                after_if = false;
                after_let = None;
            }
            proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ';' => {
                after_if = false;
                after_let = None;
            }
            proc_macro2::TokenTree::Group(_)
            | proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

struct IfResultLetPatterns {
    found: Vec<proc_macro2::Span>,
}

impl Visit<'_> for IfResultLetPatterns {
    fn visit_expr_if(&mut self, if_: &syn::ExprIf) {
        self.found.extend(result_let_patterns(&if_.cond));
        syn::visit::visit_block(self, &if_.then_branch);
        if let Some((_else, otherwise)) = &if_.else_branch {
            self.visit_expr(otherwise);
        }
    }
}

fn pattern_names_variant(pattern: &syn::Pat, wanted: &str) -> bool {
    let mut finder = PatternVariant {
        wanted,
        found: false,
    };
    finder.visit_pat(pattern);
    finder.found
}

struct PatternVariant<'a> {
    wanted: &'a str,
    found: bool,
}

impl Visit<'_> for PatternVariant<'_> {
    fn visit_path(&mut self, path: &syn::Path) {
        if path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == self.wanted)
        {
            self.found = true;
            return;
        }
        syn::visit::visit_path(self, path);
    }
}

fn renamed_fallible_iterators(tree: &syn::UseTree) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    match tree {
        syn::UseTree::Path(path) => found.extend(renamed_fallible_iterators(&path.tree)),
        syn::UseTree::Rename(rename) if rename.ident == "read_dir" || rename.ident == "WalkDir" => {
            found.push(rename.rename.span());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                found.extend(renamed_fallible_iterators(item));
            }
        }
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
    }
    found
}

fn renamed_use_spans(tree: &syn::UseTree, original: &str) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    match tree {
        syn::UseTree::Path(path) => found.extend(renamed_use_spans(&path.tree, original)),
        syn::UseTree::Rename(rename) if rename.ident == original && rename.rename != "_" => {
            found.push(rename.rename.span());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                found.extend(renamed_use_spans(item, original));
            }
        }
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
    }
    found
}

fn imported_function_spans(tree: &syn::UseTree, wanted: &str) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    match tree {
        syn::UseTree::Path(path) => {
            found.extend(imported_function_spans(&path.tree, wanted));
        }
        syn::UseTree::Name(name) if name.ident == wanted => found.push(name.ident.span()),
        syn::UseTree::Rename(rename) if rename.ident == wanted && rename.rename != "_" => {
            found.push(rename.rename.span());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                found.extend(imported_function_spans(item, wanted));
            }
        }
        syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
    }
    found
}

fn imported_poison_method_spans(tree: &syn::UseTree, wanted: &str) -> Vec<proc_macro2::Span> {
    fn visit(
        tree: &syn::UseTree,
        wanted: &str,
        path: &mut Vec<String>,
        found: &mut Vec<proc_macro2::Span>,
    ) {
        match tree {
            syn::UseTree::Path(one) => {
                let depth = path.len();
                path.push(one.ident.to_string());
                visit(&one.tree, wanted, path, found);
                path.truncate(depth);
            }
            syn::UseTree::Name(name)
                if name.ident == wanted && path.iter().any(|part| part == "PoisonError") =>
            {
                found.push(name.ident.span());
            }
            syn::UseTree::Rename(rename)
                if rename.ident == wanted
                    && rename.rename != "_"
                    && path.iter().any(|part| part == "PoisonError") =>
            {
                found.push(rename.rename.span());
            }
            syn::UseTree::Group(group) => {
                for item in &group.items {
                    visit(item, wanted, path, found);
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
        }
    }

    let mut found = Vec::new();
    visit(tree, wanted, &mut Vec::new(), &mut found);
    found
}

fn unbounded_channel_path(path: &syn::Path) -> bool {
    path.segments
        .last()
        .is_some_and(|segment| segment.ident == "channel")
        && (path.segments.len() == 1 || path.segments.iter().any(|segment| segment.ident == "mpsc"))
}

fn poison_recovery_path(path: &syn::Path) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    last.ident == "clear_poison"
        || (["into_inner", "get_ref", "get_mut"].contains(&last.ident.to_string().as_str())
            && path
                .segments
                .iter()
                .any(|segment| segment.ident == "PoisonError"))
}

fn expression_names_poison(expression: &syn::Expr) -> bool {
    match expression {
        syn::Expr::Group(group) => expression_names_poison(&group.expr),
        syn::Expr::Paren(paren) => expression_names_poison(&paren.expr),
        syn::Expr::Reference(reference) => expression_names_poison(&reference.expr),
        syn::Expr::Path(path) => path.path.segments.last().is_some_and(|segment| {
            segment
                .ident
                .to_string()
                .to_ascii_lowercase()
                .contains("poison")
        }),
        syn::Expr::Field(field) => expression_names_poison(&field.base),
        _ => false,
    }
}

fn invocation_spans_in_tokens(
    tokens: &proc_macro2::TokenStream,
    wanted: &str,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(invocation_spans_in_tokens(&group.stream(), wanted));
        }
        let proc_macro2::TokenTree::Ident(name) = token else {
            continue;
        };
        if name != wanted
            || !matches!(
                trees.get(at.saturating_add(1)),
                Some(proc_macro2::TokenTree::Group(group))
                    if group.delimiter() == proc_macro2::Delimiter::Parenthesis
            )
            || matches!(
                at.checked_sub(1).and_then(|before| trees.get(before)),
                Some(proc_macro2::TokenTree::Ident(prefix)) if prefix == "fn"
            )
        {
            continue;
        }
        found.push(name.span());
    }
    found
}

fn identifier_spans_in_tokens(
    tokens: &proc_macro2::TokenStream,
    wanted: &str,
) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    for token in tokens.clone() {
        match token {
            proc_macro2::TokenTree::Ident(name) if name == wanted => found.push(name.span()),
            proc_macro2::TokenTree::Group(group) => {
                found.extend(identifier_spans_in_tokens(&group.stream(), wanted));
            }
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

fn poison_recovery_spans_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = invocation_spans_in_tokens(tokens, "clear_poison");
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(poison_recovery_spans_in_tokens(&group.stream()));
        }
        let proc_macro2::TokenTree::Ident(name) = token else {
            continue;
        };
        if name == "PoisonError" {
            found.push(name.span());
            continue;
        }
        if !["into_inner", "get_ref", "get_mut"].contains(&name.to_string().as_str())
            || !matches!(trees.get(at.saturating_sub(1)), Some(proc_macro2::TokenTree::Punct(dot)) if dot.as_char() == '.')
        {
            continue;
        }
        if let Some(proc_macro2::TokenTree::Ident(receiver)) = trees.get(at.saturating_sub(2))
            && receiver.to_string().to_ascii_lowercase().contains("poison")
        {
            found.push(name.span());
        }
    }
    found
}

const DIRECT_JSON_FUNCTIONS: [&str; 3] = ["from_reader", "from_slice", "from_str"];

fn direct_json_path(path: &syn::Path, qself: Option<&syn::QSelf>, aliases: &Aliases) -> bool {
    let Some(last) = path.segments.last() else {
        return false;
    };
    DIRECT_JSON_FUNCTIONS.contains(&last.ident.to_string().as_str())
        && (path
            .segments
            .iter()
            .any(|segment| segment.ident == "serde_json")
            || aliases.json_value_owner(path)
            || qself.is_some_and(|qualified| {
                json_deserializer_type(&qualified.ty) || aliases.json_value(&qualified.ty)
            }))
}

fn json_deserializer_type(type_: &syn::Type) -> bool {
    match type_ {
        syn::Type::Path(path) => json_deserializer_path(&path.path),
        syn::Type::Group(group) => json_deserializer_type(&group.elem),
        syn::Type::Paren(paren) => json_deserializer_type(&paren.elem),
        _ => false,
    }
}

fn json_deserializer_path(path: &syn::Path) -> bool {
    path.segments
        .iter()
        .any(|segment| segment.ident == "serde_json")
        && path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "Deserializer")
}

fn direct_json_import_spans(tree: &syn::UseTree) -> Vec<proc_macro2::Span> {
    fn walk(tree: &syn::UseTree, path: &mut Vec<String>, found: &mut Vec<proc_macro2::Span>) {
        match tree {
            syn::UseTree::Path(one) => {
                let depth = path.len();
                path.push(one.ident.to_string());
                walk(&one.tree, path, found);
                path.truncate(depth);
            }
            syn::UseTree::Name(one) => {
                if path.first().is_some_and(|root| root == "serde_json")
                    && (DIRECT_JSON_FUNCTIONS.contains(&one.ident.to_string().as_str())
                        || one.ident == "Deserializer")
                {
                    found.push(one.ident.span());
                }
            }
            syn::UseTree::Rename(one) => {
                let root_alias = path.is_empty() && one.ident == "serde_json";
                let json_child_alias = path.first().is_some_and(|root| root == "serde_json")
                    && (DIRECT_JSON_FUNCTIONS.contains(&one.ident.to_string().as_str())
                        || matches!(
                            one.ident.to_string().as_str(),
                            "self" | "de" | "Deserializer"
                        ));
                if root_alias || json_child_alias {
                    found.push(one.rename.span());
                }
            }
            syn::UseTree::Glob(star) if path.first().is_some_and(|root| root == "serde_json") => {
                found.push(star.star_token.span);
            }
            syn::UseTree::Group(group) => {
                for one in &group.items {
                    walk(one, path, found);
                }
            }
            syn::UseTree::Glob(_) => {}
        }
    }

    let mut found = Vec::new();
    walk(tree, &mut Vec::new(), &mut found);
    found
}

fn json_value_boundary_spans(tree: &syn::UseTree, reject_plain: bool) -> Vec<proc_macro2::Span> {
    fn walk(
        tree: &syn::UseTree,
        path: &mut Vec<String>,
        reject_plain: bool,
        found: &mut Vec<proc_macro2::Span>,
    ) {
        match tree {
            syn::UseTree::Path(one) => {
                let depth = path.len();
                path.push(one.ident.to_string());
                walk(&one.tree, path, reject_plain, found);
                path.truncate(depth);
            }
            syn::UseTree::Name(one)
                if reject_plain
                    && path.first().is_some_and(|root| root == "serde_json")
                    && one.ident == "Value" =>
            {
                found.push(one.ident.span());
            }
            syn::UseTree::Rename(one)
                if path.first().is_some_and(|root| root == "serde_json")
                    && one.ident == "Value" =>
            {
                found.push(one.rename.span());
            }
            syn::UseTree::Group(group) => {
                for one in &group.items {
                    walk(one, path, reject_plain, found);
                }
            }
            syn::UseTree::Name(_) | syn::UseTree::Rename(_) | syn::UseTree::Glob(_) => {}
        }
    }

    let mut found = Vec::new();
    walk(tree, &mut Vec::new(), reject_plain, &mut found);
    found
}

fn direct_json_member_span(window: &[proc_macro2::TokenTree]) -> Option<proc_macro2::Span> {
    let [
        proc_macro2::TokenTree::Ident(json),
        proc_macro2::TokenTree::Punct(first_colon),
        proc_macro2::TokenTree::Punct(second_colon),
        member,
    ] = window
    else {
        return None;
    };
    if json != "serde_json" || first_colon.as_char() != ':' || second_colon.as_char() != ':' {
        return None;
    }
    match member {
        proc_macro2::TokenTree::Ident(member)
            if DIRECT_JSON_FUNCTIONS.contains(&member.to_string().as_str())
                || member == "Deserializer" =>
        {
            Some(member.span())
        }
        proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '$' => Some(json.span()),
        _ => None,
    }
}

fn explicit_parse_type(
    after_parse: &[proc_macro2::TokenTree],
) -> Option<&[proc_macro2::TokenTree]> {
    let [
        proc_macro2::TokenTree::Punct(first_colon),
        proc_macro2::TokenTree::Punct(second_colon),
        proc_macro2::TokenTree::Punct(open),
        type_and_rest @ ..,
    ] = after_parse
    else {
        return None;
    };
    if first_colon.as_char() != ':' || second_colon.as_char() != ':' || open.as_char() != '<' {
        return None;
    }
    let close = type_and_rest.iter().position(
        |tree| matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '>'),
    );
    match close {
        Some(close) => type_and_rest.get(..close),
        None => Some(type_and_rest),
    }
}

fn token_type_is_json_value(type_tokens: &[proc_macro2::TokenTree], aliases: &Aliases) -> bool {
    let names_serde_json = type_tokens
        .iter()
        .any(|tree| matches!(tree, proc_macro2::TokenTree::Ident(ident) if ident == "serde_json"));
    let names_value = type_tokens
        .iter()
        .any(|tree| matches!(tree, proc_macro2::TokenTree::Ident(ident) if ident == "Value"));
    let names_alias = type_tokens.iter().any(|tree| {
        matches!(tree, proc_macro2::TokenTree::Ident(ident) if aliases.json_values.contains(&ident.to_string()))
    });
    (names_serde_json && names_value) || names_alias
}

fn direct_json_spans_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found: Vec<_> = trees
        .windows(4)
        .filter_map(direct_json_member_span)
        .collect();
    for (at, token) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(parse) = token else {
            continue;
        };
        let follows_dot = trees
            .get(..at)
            .and_then(|before| before.last())
            .is_some_and(|tree| matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '.'));
        if parse != "parse" || !follows_dot {
            continue;
        }
        let Some(after_parse) = trees.get(at..).and_then(|from_parse| from_parse.get(1..)) else {
            found.push(parse.span());
            continue;
        };
        let Some(type_tokens) = explicit_parse_type(after_parse) else {
            found.push(parse.span());
            continue;
        };
        if token_type_is_json_value(type_tokens, aliases) {
            found.push(parse.span());
        }
    }
    for token in trees {
        match token {
            proc_macro2::TokenTree::Group(group) => {
                found.extend(direct_json_spans_in_tokens(&group.stream(), aliases));
            }
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

fn macro_attribute_spans(
    tokens: &proc_macro2::TokenStream,
    wanted: &str,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(macro_attribute_spans(&group.stream(), wanted));
        }
        if !matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#') {
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(group)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        if group.delimiter() != proc_macro2::Delimiter::Bracket {
            continue;
        }
        if let Some(proc_macro2::TokenTree::Ident(ident)) = group.stream().into_iter().next()
            && ident == wanted
        {
            found.push(ident.span());
        }
    }
    found
}

fn macro_tokens_name(tokens: &proc_macro2::TokenStream, wanted: &str) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        proc_macro2::TokenTree::Ident(ident) => ident == wanted,
        proc_macro2::TokenTree::Group(group) => macro_tokens_name(&group.stream(), wanted),
        proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => false,
    })
}

fn fabricated_overflow_spans_in_tokens(
    tokens: &proc_macro2::TokenStream,
) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    for token in tokens.clone() {
        match token {
            proc_macro2::TokenTree::Ident(ident)
                if matches!(
                    ident.to_string().as_str(),
                    "saturating_add" | "saturating_sub" | "saturating_mul" | "unwrap_or"
                ) =>
            {
                found.push(ident.span());
            }
            proc_macro2::TokenTree::Group(group) => {
                found.extend(fabricated_overflow_spans_in_tokens(&group.stream()));
            }
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

fn wrapping_counter_spans_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let mut found = Vec::new();
    for token in tokens.clone() {
        match token {
            proc_macro2::TokenTree::Ident(ident) if wrapping_counter_method(&ident) => {
                found.push(ident.span());
            }
            proc_macro2::TokenTree::Group(group) => {
                found.extend(wrapping_counter_spans_in_tokens(&group.stream()));
            }
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

fn macro_default_spans(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let mut found = macro_attribute_spans(tokens, "default");
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    for (at, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(macro_default_spans(&group.stream(), aliases));
        }
        if !matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '#') {
            continue;
        }
        let Some(proc_macro2::TokenTree::Group(group)) = trees.get(at.saturating_add(1)) else {
            continue;
        };
        if group.delimiter() != proc_macro2::Delimiter::Bracket {
            continue;
        }
        let inner: Vec<proc_macro2::TokenTree> = group.stream().into_iter().collect();
        if !matches!(inner.first(), Some(proc_macro2::TokenTree::Ident(ident)) if ident == "derive")
        {
            continue;
        }
        if inner.iter().any(|token| match token {
            proc_macro2::TokenTree::Group(arguments) => arguments
                .stream()
                .into_iter()
                .any(|held| matches!(held, proc_macro2::TokenTree::Ident(ident) if aliases.default_derives.contains(&ident.to_string()) || ident == "Default")),
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => false,
        }) && let Some(proc_macro2::TokenTree::Ident(derive)) = inner.first()
        {
            found.push(derive.span());
        }
    }
    found
}

fn iterator_flatten_spans_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, token) in trees.iter().enumerate() {
        match token {
            proc_macro2::TokenTree::Ident(ident)
                if ident == "flatten"
                    && trees.get(..at).is_some_and(|before| {
                        before.last().is_some_and(|tree| {
                            matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '.')
                        }) || tokens_end_with_qualified(before, "Iterator")
                    }) =>
            {
                found.push(ident.span());
            }
            proc_macro2::TokenTree::Group(group) => {
                found.extend(iterator_flatten_spans_in_tokens(&group.stream()));
            }
            proc_macro2::TokenTree::Ident(_)
            | proc_macro2::TokenTree::Punct(_)
            | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
    found
}

fn result_into_iter_spans_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(result_into_iter_spans_in_tokens(&group.stream()));
        }
        let proc_macro2::TokenTree::Ident(result) = token else {
            continue;
        };
        if result != "Result"
            || !punct_at(&trees, at.saturating_add(1), ':')
            || !punct_at(&trees, at.saturating_add(2), ':')
        {
            continue;
        }
        if let Some(proc_macro2::TokenTree::Ident(method)) = trees.get(at.saturating_add(3))
            && method == "into_iter"
        {
            found.push(method.span());
        }
    }
    found
}

fn result_erasure_spans_in_tokens(tokens: &proc_macro2::TokenStream) -> Vec<proc_macro2::Span> {
    let expressions = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated
        .parse2(tokens.clone());
    match expressions {
        Ok(expressions) => {
            let mut finder = ResultErasures { found: Vec::new() };
            for expression in &expressions {
                finder.visit_expr(expression);
            }
            return finder.found;
        }
        Err(_not_a_comma_separated_expression_list) => {}
    }
    match syn::parse2::<syn::Block>(tokens.clone()) {
        Ok(block) => {
            let mut finder = ResultErasures { found: Vec::new() };
            finder.visit_block(&block);
            return finder.found;
        }
        Err(_not_a_complete_block) => {}
    }

    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, token) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = token {
            found.extend(result_erasure_spans_in_tokens(&group.stream()));
        }
        let proc_macro2::TokenTree::Ident(method) = token else {
            continue;
        };
        if method != "ok" && method != "err" {
            continue;
        }
        let called = matches!(
            trees.get(at.saturating_add(1)),
            Some(proc_macro2::TokenTree::Group(group))
                if group.delimiter() == proc_macro2::Delimiter::Parenthesis
        );
        let before = trees.get(..at);
        let method_call = called
            && before.is_some_and(|before| {
                before.last().is_some_and(|tree| {
                    matches!(tree, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '.')
                })
            });
        let result_ufcs =
            called && before.is_some_and(|before| tokens_end_with_qualified(before, "Result"));
        if method_call || result_ufcs {
            found.push(method.span());
        }
    }
    found
}

struct ResultErasures {
    found: Vec<proc_macro2::Span>,
}

impl Visit<'_> for ResultErasures {
    fn visit_expr_method_call(&mut self, call: &syn::ExprMethodCall) {
        if call.method == "ok" || call.method == "err" {
            self.found.push(call.method.span());
        }
        syn::visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let syn::Expr::Path(path) = call.func.as_ref()
            && ["ok", "err"]
                .iter()
                .any(|method| result_path_method(&path.path, method))
            && let Some(segment) = path.path.segments.last()
        {
            self.found.push(segment.ident.span());
        }
        syn::visit::visit_expr_call(self, call);
    }
}

fn owned_trait_objects_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(owned_trait_objects_in_tokens(&group.stream(), aliases));
            continue;
        }
        let proc_macro2::TokenTree::Ident(ident) = tree else {
            continue;
        };
        let constructor_metavariable = at > 0 && punct_at(&trees, at.saturating_sub(1), '$');
        if !aliases.owned(&ident.to_string()) && !constructor_metavariable {
            continue;
        }
        let mut opening = at.saturating_add(1);
        if punct_at(&trees, opening, ':') && punct_at(&trees, opening.saturating_add(1), ':') {
            opening = opening.saturating_add(2);
        }
        if !punct_at(&trees, opening, '<') {
            continue;
        }
        let Some(argument) = first_macro_type_argument(&trees, opening) else {
            continue;
        };
        if token_stream_has_dollar(&argument)
            || syn::parse2::<syn::Type>(argument).is_ok_and(|held| aliases.trait_object(&held))
        {
            found.push(ident.span());
        }
    }
    found
}

fn tri_state_bools_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(tri_state_bools_in_tokens(&group.stream(), aliases));
            continue;
        }
        let proc_macro2::TokenTree::Ident(constructor) = tree else {
            continue;
        };
        let constructor_metavariable = at > 0 && punct_at(&trees, at.saturating_sub(1), '$');
        if constructor != "Option"
            && !aliases.option_types.contains(&constructor.to_string())
            && !constructor_metavariable
        {
            continue;
        }
        let mut opening = at.saturating_add(1);
        if punct_at(&trees, opening, ':') && punct_at(&trees, opening.saturating_add(1), ':') {
            opening = opening.saturating_add(2);
        }
        if !punct_at(&trees, opening, '<') {
            continue;
        }
        let Some(argument) = first_macro_type_argument(&trees, opening) else {
            continue;
        };
        let literal_option =
            constructor == "Option" || aliases.option_types.contains(&constructor.to_string());
        if (literal_option && token_stream_has_dollar(&argument))
            || syn::parse2::<syn::Type>(argument).is_ok_and(|held| aliases.boolean(&held))
        {
            found.push(constructor.span());
        }
    }
    found
}

fn token_stream_has_dollar(tokens: &proc_macro2::TokenStream) -> bool {
    tokens.clone().into_iter().any(|token| match token {
        proc_macro2::TokenTree::Punct(punct) => punct.as_char() == '$',
        proc_macro2::TokenTree::Group(group) => token_stream_has_dollar(&group.stream()),
        proc_macro2::TokenTree::Ident(_) | proc_macro2::TokenTree::Literal(_) => false,
    })
}

fn unit_domain_conversions_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for tree in &trees {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(unit_domain_conversions_in_tokens(&group.stream(), aliases));
        }
    }
    for (at, tree) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(ident) = tree else {
            continue;
        };
        let named_from = aliases.is_from_trait(&ident.to_string());
        let trait_metavariable = at > 0 && punct_at(&trees, at.saturating_sub(1), '$');
        if !named_from && !trait_metavariable {
            continue;
        }
        let mut opening = at.saturating_add(1);
        if punct_at(&trees, opening, ':') && punct_at(&trees, opening.saturating_add(1), ':') {
            opening = opening.saturating_add(2);
        }
        let argument = if punct_at(&trees, opening, '<') {
            first_macro_type_argument(&trees, opening)
        } else {
            None
        };
        let concrete_unit = argument.clone().is_some_and(|argument| {
            syn::parse2::<syn::Type>(argument).is_ok_and(|held| aliases.unit(&held))
        });
        let direct =
            argument.is_some_and(|argument| token_stream_has_dollar(&argument)) || concrete_unit;
        let supplied_beside = trees.iter().any(|token| token_is_unit(token, aliases));
        if (named_from && (direct || supplied_beside)) || (trait_metavariable && concrete_unit) {
            found.push(ident.span());
        }
    }
    found
}

fn token_is_unit(token: &proc_macro2::TokenTree, aliases: &Aliases) -> bool {
    match token {
        proc_macro2::TokenTree::Group(group) => {
            group.delimiter() == proc_macro2::Delimiter::Parenthesis && group.stream().is_empty()
        }
        proc_macro2::TokenTree::Ident(ident) => {
            aliases.meaning(&ident.to_string()) == Some(TypeMeaning::Unit)
        }
        proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => false,
    }
}

fn unit_aliases_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for tree in &trees {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(unit_aliases_in_tokens(&group.stream(), aliases));
        }
    }
    for (at, tree) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(type_) = tree else {
            continue;
        };
        if type_ != "type" {
            continue;
        }
        let Some((equals, _)) = trees
            .iter()
            .enumerate()
            .skip(at.saturating_add(1))
            .take_while(|(_offset, token)| !token_is_punct(token, ';'))
            .find(|(_offset, token)| token_is_punct(token, '='))
        else {
            continue;
        };
        let held: proc_macro2::TokenStream = trees
            .iter()
            .skip(equals.saturating_add(1))
            .take_while(|token| !token_is_punct(token, ';'))
            .cloned()
            .collect();
        if token_stream_has_dollar(&held)
            || syn::parse2::<syn::Type>(held).is_ok_and(|unit| aliases.unit(&unit))
        {
            let span = trees
                .get(at.saturating_add(1))
                .and_then(|token| match token {
                    proc_macro2::TokenTree::Ident(ident) => Some(ident.span()),
                    _ => None,
                })
                .unwrap_or_else(|| type_.span());
            found.push(span);
        }
    }
    found
}

fn owned_pointer_aliases_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for tree in &trees {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(owned_pointer_aliases_in_tokens(&group.stream(), aliases));
        }
    }
    for (at, tree) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(type_) = tree else {
            continue;
        };
        if type_ != "type" {
            continue;
        }
        let Some(equals) = trees
            .iter()
            .enumerate()
            .skip(at.saturating_add(1))
            .take_while(|(_offset, token)| !token_is_punct(token, ';'))
            .find_map(|(offset, token)| token_is_punct(token, '=').then_some(offset))
        else {
            continue;
        };
        if let Some(span) = trees
            .iter()
            .skip(equals.saturating_add(1))
            .take_while(|token| !token_is_punct(token, ';'))
            .find_map(|token| match token {
                proc_macro2::TokenTree::Ident(ident) if aliases.owned(&ident.to_string()) => {
                    Some(ident.span())
                }
                _ => None,
            })
        {
            found.push(span);
        }
    }
    found
}

fn trait_object_aliases_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    macro_type_aliases(tokens, |held| aliases.trait_object(held))
}

fn string_aliases_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    macro_type_aliases(tokens, |held| aliases.string_error(held))
}

fn macro_type_aliases(
    tokens: &proc_macro2::TokenStream,
    prohibited: impl Copy + Fn(&syn::Type) -> bool,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for tree in &trees {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(macro_type_aliases(&group.stream(), prohibited));
        }
    }
    for (at, tree) in trees.iter().enumerate() {
        let proc_macro2::TokenTree::Ident(type_) = tree else {
            continue;
        };
        if type_ != "type" {
            continue;
        }
        let Some(equals) = trees
            .iter()
            .enumerate()
            .skip(at.saturating_add(1))
            .take_while(|(_offset, token)| !token_is_punct(token, ';'))
            .find_map(|(offset, token)| token_is_punct(token, '=').then_some(offset))
        else {
            continue;
        };
        let held: proc_macro2::TokenStream = trees
            .iter()
            .skip(equals.saturating_add(1))
            .take_while(|token| !token_is_punct(token, ';'))
            .cloned()
            .collect();
        if syn::parse2::<syn::Type>(held).is_ok_and(|held| prohibited(&held)) {
            found.push(type_.span());
        }
    }
    found
}

fn string_errors_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(string_errors_in_tokens(&group.stream(), aliases));
            continue;
        }
        let proc_macro2::TokenTree::Ident(result) = tree else {
            continue;
        };
        if !aliases.result(&result.to_string()) {
            continue;
        }
        let mut opening = at.saturating_add(1);
        if punct_at(&trees, opening, ':') && punct_at(&trees, opening.saturating_add(1), ':') {
            opening = opening.saturating_add(2);
        }
        if !punct_at(&trees, opening, '<') {
            continue;
        }
        let Some(error) = macro_type_argument(&trees, opening, 1) else {
            continue;
        };
        if token_stream_has_dollar(&error)
            || syn::parse2::<syn::Type>(error).is_ok_and(|held| aliases.string_error(&held))
        {
            found.push(result.span());
        }
    }
    found
}

fn unit_errors_in_tokens(
    tokens: &proc_macro2::TokenStream,
    aliases: &Aliases,
) -> Vec<proc_macro2::Span> {
    let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
    let mut found = Vec::new();
    for (at, tree) in trees.iter().enumerate() {
        if let proc_macro2::TokenTree::Group(group) = tree {
            found.extend(unit_errors_in_tokens(&group.stream(), aliases));
            continue;
        }
        let proc_macro2::TokenTree::Ident(result) = tree else {
            continue;
        };
        if !aliases.result(&result.to_string()) {
            continue;
        }
        let mut opening = at.saturating_add(1);
        if punct_at(&trees, opening, ':') && punct_at(&trees, opening.saturating_add(1), ':') {
            opening = opening.saturating_add(2);
        }
        if !punct_at(&trees, opening, '<') {
            continue;
        }
        let Some(error) = macro_type_argument(&trees, opening, 1) else {
            continue;
        };
        if token_stream_has_dollar(&error)
            || syn::parse2::<syn::Type>(error).is_ok_and(|held| aliases.unit(&held))
        {
            found.push(result.span());
        }
    }
    found
}

fn token_is_punct(token: &proc_macro2::TokenTree, wanted: char) -> bool {
    matches!(token, proc_macro2::TokenTree::Punct(punct) if punct.as_char() == wanted)
}

fn first_macro_type_argument(
    trees: &[proc_macro2::TokenTree],
    opening: usize,
) -> Option<proc_macro2::TokenStream> {
    macro_type_argument(trees, opening, 0)
}

fn macro_type_argument(
    trees: &[proc_macro2::TokenTree],
    opening: usize,
    wanted: usize,
) -> Option<proc_macro2::TokenStream> {
    let mut held = Vec::new();
    let mut depth = 0usize;
    let mut argument = 0usize;
    for (offset, tree) in trees.iter().enumerate().skip(opening.saturating_add(1)) {
        match tree {
            proc_macro2::TokenTree::Punct(punct) if punct.as_char() == '<' => {
                depth = depth.saturating_add(1);
                held.push(tree.clone());
            }
            proc_macro2::TokenTree::Punct(punct)
                if punct.as_char() == '>' && !punct_at(trees, offset.saturating_sub(1), '-') =>
            {
                if depth == 0 {
                    return (argument == wanted).then(|| held.into_iter().collect());
                }
                depth = depth.saturating_sub(1);
                held.push(tree.clone());
            }
            proc_macro2::TokenTree::Punct(punct) if punct.as_char() == ',' && depth == 0 => {
                if argument == wanted {
                    return Some(held.into_iter().collect());
                }
                argument = argument.saturating_add(1);
                held.clear();
            }
            _ => held.push(tree.clone()),
        }
    }
    None
}

fn punct_at(trees: &[proc_macro2::TokenTree], at: usize, wanted: char) -> bool {
    matches!(trees.get(at), Some(proc_macro2::TokenTree::Punct(punct)) if punct.as_char() == wanted)
}

fn tokens_end_with_qualified(trees: &[proc_macro2::TokenTree], owner: &str) -> bool {
    matches!(
        trees,
        [
            ..,
            proc_macro2::TokenTree::Ident(found),
            proc_macro2::TokenTree::Punct(first_colon),
            proc_macro2::TokenTree::Punct(second_colon),
        ] if found == owner && first_colon.as_char() == ':' && second_colon.as_char() == ':'
    )
}
