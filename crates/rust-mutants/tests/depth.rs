// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! That a reading refuses text deeper than its thread's stack holds, by name and before anything recurses through it, and reads every text up to that bound.

#![expect(
    clippy::panic,
    clippy::expect_used,
    clippy::arithmetic_side_effects,
    reason = "a test reports a setup failure and a failed law by panicking, and counts the tokens \
              of texts it wrote itself"
)]

use proc_macro2::{Delimiter, Spacing, TokenStream, TokenTree};
use proptest::prelude::*;
use rust_mutants::parsing::{CHAIN, Depth, NESTING, Past, ReadingError};
use rust_mutants::rule::{Registry, Tier};
use rust_mutants::syntax::{Selection, SyntaxError, discover_file};

/// How far `text` runs, as a reading measures it.
fn depth(text: &str) -> Depth {
    let tokens = text.parse::<TokenStream>().expect("the text lexes");
    Depth::of(&tokens)
}

/// What discovery, item numbering and the skeleton answer for `source`: nothing where all of them read it, and otherwise the first refusal's code.
fn refusal(source: &str) -> Option<String> {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    if let Err(error) = discover_file("src/lib.rs", source.as_bytes(), &selection) {
        return Some(format!("discovery: {}: {error}", error.code().code));
    }
    let file = rust_mutants::instrument::ItemSource {
        path: "src/lib.rs",
        package: "demo",
        source: source.as_bytes(),
    };
    let catalog = match rust_mutants::instrument::catalog_items(&[file]) {
        Ok(catalog) => catalog,
        Err(error) => return Some(format!("items: {}: {error}", error.code().code)),
    };
    let refs: Vec<rust_mutants::touch::ItemRef> = catalog
        .items
        .iter()
        .map(|item| catalog.item_ref(item.index).expect("a cataloged item"))
        .collect();
    let pairs: Vec<_> = catalog.items.iter().zip(&refs).collect();
    let unit = rust_mutants::skeleton::UnitSource {
        package: "demo".to_owned(),
        target: "demo".to_owned(),
        kind: "lib".to_owned(),
        test: false,
        files: [("$root/src/lib.rs".to_owned(), source.as_bytes().to_vec())].into(),
        env: std::collections::BTreeMap::new(),
        emitted: std::collections::BTreeMap::new(),
    };
    let skeletons = rust_mutants::skeleton::evidence(&[unit], &pairs);
    (skeletons.items.len() != catalog.items.len())
        .then(|| "skeleton: an item went unjudged".to_owned())
}

/// Which way discovery found `source` too deep, or a panic naming what it answered instead.
fn past(source: &str) -> Past {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    match discover_file("src/lib.rs", source.as_bytes(), &selection) {
        Err(SyntaxError::Unread {
            source: ReadingError::TooDeep { past, .. },
            ..
        }) => past,
        other => panic!("a text past a bound is refused as too deep, and this one was {other:?}"),
    }
}

/// The code discovery refuses `source` with, or a panic naming what it answered instead.
fn refused_with(source: &str) -> String {
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    match discover_file("src/lib.rs", source.as_bytes(), &selection) {
        Ok(found) => panic!(
            "a text past what a reading holds is refused, and this one was read into {} \
             candidates",
            found.candidates.len()
        ),
        Err(error) => error.code().code.to_owned(),
    }
}

#[test]
fn a_file_nested_past_what_a_reading_holds_is_refused_rather_than_overflowing_its_stack() {
    let deep = format!(
        "fn f() -> u8 {{ {}1{} }}\n",
        "(".repeat(100_000),
        ")".repeat(100_000)
    );
    assert_eq!(
        refused_with(&deep),
        "RM0020",
        "every group is a frame of the parser, the walk and the drop, so a nesting past the \
         reading thread's stack is refused by name before anything recurses through it"
    );
}

#[test]
fn a_chain_past_what_a_reading_holds_is_refused_rather_than_overflowing_its_stack() {
    let long = format!("fn f(a: bool) -> bool {{ {}a }}\n", "!".repeat(100_000));
    assert_eq!(
        refused_with(&long),
        "RM0020",
        "every operator of a chain is a frame of the parser, the walk and the drop, so a chain \
         past the reading thread's stack is refused by name before anything recurses through it"
    );
}

/// A function whose body is `levels` blocks deep, the shape that costs a reading the most stack a group.
fn blocks(levels: usize) -> String {
    format!(
        "fn f() -> u8 {}1{}\n",
        "{".repeat(levels),
        "}".repeat(levels)
    )
}

#[test]
fn a_file_nested_exactly_as_deep_as_a_reading_holds_is_read_and_one_deeper_is_refused() {
    let at = blocks(NESTING);
    assert_eq!(
        depth(&at).nesting,
        NESTING,
        "the law's text nests exactly at the bound"
    );
    assert_eq!(
        refusal(&at),
        None,
        "the deepest nesting a reading holds is read, on the stack a reading thread has"
    );
    let past_it = blocks(NESTING + 1);
    assert_eq!(depth(&past_it).nesting, NESTING + 1);
    assert_eq!(
        past(&past_it),
        Past::Nesting,
        "one group past the bound is refused, and the refusal says the nesting was too deep"
    );
}

/// `shape` with its `{}` taken by as many of `repeated`, one token, as make the text chain exactly `chain` tokens, counted from where the repeats are the longest run.
fn chained(shape: &str, repeated: &str, chain: usize) -> String {
    let probe = 1_000;
    let base = depth(&shape.replace("{}", &repeated.repeat(probe))).chain - probe;
    let text = shape.replace("{}", &repeated.repeat(chain - base));
    assert_eq!(
        depth(&text).chain,
        chain,
        "the law's text chains exactly {chain} tokens: {shape}"
    );
    text
}

#[test]
fn a_chain_exactly_as_long_as_a_reading_holds_is_read_and_one_longer_is_refused() {
    for (shape, repeated) in [
        ("fn f() -> u8 { {}1 }\n", "return "),
        ("fn f(a: bool) -> bool { {}a }\n", "!"),
        ("fn f(a: u8) { let _ = {}a; }\n", "& "),
        ("fn f(a: u8) -> u8 { a{} }\n", "()"),
    ] {
        let at = chained(shape, repeated, CHAIN);
        assert_eq!(
            refusal(&at),
            None,
            "the longest chain a reading holds is read, on the stack a reading thread has: {shape}"
        );
        let past_it = chained(shape, repeated, CHAIN + 1);
        assert_eq!(
            past(&past_it),
            Past::Chain,
            "one token past the bound is refused, and the refusal says the chain was too long: \
             {shape}"
        );
    }
}

#[test]
fn the_costliest_shapes_read_at_both_bounds_at_once() {
    let levels = NESTING;
    let shape = format!(
        "fn f() -> u8 {}{{}}1{}\n",
        "{".repeat(levels),
        "}".repeat(levels)
    );
    let deepest = chained(&shape, "return ", CHAIN);
    assert_eq!(depth(&deepest).nesting, NESTING);
    assert_eq!(
        refusal(&deepest),
        None,
        "a text at both bounds at once reads: the bounds are the stack a reading thread has with \
         room to spare, not each of them alone"
    );
    let generic = depth("type T = u8;\nfn f() {}\n").chain;
    let levels = (CHAIN - generic) / 3;
    let nested = format!(
        "type T = {}u8{};\nfn f() {{}}\n",
        "Vec<".repeat(levels),
        ">".repeat(levels)
    );
    assert!(depth(&nested).chain <= CHAIN);
    assert_eq!(
        refusal(&nested),
        None,
        "generic arguments are the costliest chain a reading reads, and at the bound they read"
    );
}

#[test]
fn tokens_are_handed_on_however_long_a_chain_they_hold() {
    let long = format!("a{}", " + a".repeat(CHAIN));
    let flat = rust_mutants::flatten::flatten(&long);
    assert!(
        flat.is_ok(),
        "a reading that only lexes builds no tree, so no chain is too long for it: {flat:?}"
    );
    let deep = format!("{}a{}", "(".repeat(NESTING + 1), ")".repeat(NESTING + 1));
    let Err(refused) = rust_mutants::flatten::flatten(&deep) else {
        panic!("a nesting past the bound is refused even where only tokens are read")
    };
    assert_eq!(
        refused.code().code,
        "RM0020",
        "the refusal keeps its code: {refused}"
    );
}

/// The code `answered` refused with, or what it answered instead.
fn code_of<T, E: std::fmt::Display>(
    answered: Result<T, E>,
    code: impl FnOnce(&E) -> &'static str,
) -> String {
    match answered {
        Ok(_) => "read".to_owned(),
        Err(error) => format!("{}: {error}", code(&error)),
    }
}

#[test]
fn every_way_into_the_engine_that_reads_rust_refuses_a_text_too_deep_by_its_own_code() {
    let past = NESTING + 1;
    let deep = format!(
        "fn f() -> bool {{ {}true{} }}\n",
        "(".repeat(past),
        ")".repeat(past)
    );
    let registry = Registry::canonical();
    let selection = Selection::tier(&registry, Tier::All);
    let named = |answered: Result<String, rust_mutants::instrument::ModuleNameError>| match answered
    {
        Ok(_) => "read".to_owned(),
        Err(rust_mutants::instrument::ModuleNameError::Tokens { source }) => {
            format!("{}: {source}", source.code().code)
        }
        Err(rust_mutants::instrument::ModuleNameError::SuffixesExhausted) => {
            "suffixes exhausted".to_owned()
        }
    };
    let reading = |error: &ReadingError| error.code().code;
    let answers = [
        (
            "discover_file",
            code_of(
                discover_file("src/lib.rs", deep.as_bytes(), &selection),
                |error| error.code().code,
            ),
        ),
        (
            "module_name",
            named(rust_mutants::instrument::module_name("src/lib.rs", &deep)),
        ),
        (
            "module_named_for",
            named(rust_mutants::instrument::module_named_for(
                &deep,
                "__rm_witness",
            )),
        ),
        (
            "flatten",
            code_of(rust_mutants::flatten::flatten(&deep), |error| {
                error.code().code
            }),
        ),
        (
            "forbids_guard_noise",
            code_of(
                rust_mutants::discover::forbids_guard_noise(&deep, &[]),
                reading,
            ),
        ),
        (
            "freestanding",
            code_of(rust_mutants::discover::freestanding(&deep, "2024"), reading),
        ),
        (
            "read_through",
            code_of(
                rust_mutants::testkit::source::read_through(&deep, "__rm"),
                reading,
            ),
        ),
    ];
    let lost: Vec<String> = answers
        .iter()
        .filter(|(_, answered)| !answered.starts_with("RM0020: "))
        .map(|(entry, answered)| format!("{entry}: {answered}"))
        .collect();
    assert!(
        lost.is_empty(),
        "every way into the engine that reads Rust refuses a text too deep to read by the \
         reading's own code, whatever error it wraps the reading in: {lost:#?}"
    );
}

#[test]
fn instrumenting_a_file_too_deep_to_read_says_so_by_the_readings_own_code() {
    let past = NESTING + 1;
    let deep = format!(
        "fn f() -> bool {{ {}true{} }}\n",
        "(".repeat(past),
        ")".repeat(past)
    );
    let source = deep.as_bytes();
    let file = rust_mutants::instrument::ItemSource {
        path: "src/lib.rs",
        package: "demo",
        source,
    };
    let probing = [rust_mutants::instrument::witness::Probing {
        index: 0,
        value: rust_mutants::span::Span::new(15, 19).expect("a span"),
        question: rust_mutants::probe::Question::True,
        super_depth: 0,
    }];
    let comparable = std::collections::BTreeSet::new();
    let probed = std::collections::BTreeMap::new();
    let codes = [
        (
            "items",
            rust_mutants::instrument::items("src/lib.rs", source, 0).map(|_| ()),
        ),
        (
            "catalog_items",
            rust_mutants::instrument::catalog_items(&[file]).map(|_| ()),
        ),
        (
            "instrument_file",
            rust_mutants::instrument::instrument_file(&rust_mutants::instrument::Instrumenting {
                path: "src/lib.rs",
                source,
                placements: &[],
                markers: &[],
                comparable: &comparable,
                probed: &probed,
                catalog_digest: "0",
                first_item: 0,
                watched: "/watched",
            })
            .map(|_| ()),
        ),
        (
            "witness_file",
            rust_mutants::instrument::witness::witness_file(
                "src/lib.rs",
                source,
                &rust_mutants::instrument::witness::Asking {
                    conditions: &[],
                    probes: &probing,
                },
            )
            .map(|_| ()),
        ),
    ];
    let lost: Vec<String> = codes
        .iter()
        .filter_map(|(entry, answered)| match answered {
            Err(error) if error.code().code == "RM0020" => None,
            Err(error) => Some(format!("{entry}: {}: {error}", error.code().code)),
            Ok(()) => Some(format!("{entry}: read")),
        })
        .collect();
    assert!(
        lost.is_empty(),
        "a reading that refused the text is not a source that changed under the run (RM3002) \
         nor a defect of the engine (RM3004): its own code says what to do: {lost:#?}"
    );
}

#[test]
fn how_far_a_text_runs_is_counted_as_the_rules_say() {
    for (text, nesting, chain) in [
        ("a + b", 0, 3),
        ("f(a, b)", 1, 3),
        ("((a))", 2, 3),
        ("fn f() {} fn g() {}", 1, 4),
        ("if a {} else {}", 1, 5),
        ("match x { 0 => {} 1 => {} }", 2, 4),
        ("#[doc = \"x\"] fn f() {}", 1, 7),
        ("/// x\nfn f() {}", 1, 7),
        ("#[a] #![b] c", 1, 2),
        ("x as u8 as u16", 0, 5),
        ("{a} + b", 1, 4),
        ("{a} b", 1, 2),
        ("a => b", 0, 1),
        ("a = > b", 0, 4),
        ("a; b c d", 0, 3),
        ("#!x", 0, 1),
    ] {
        assert_eq!(
            depth(text),
            Depth { nesting, chain },
            "a run ends at `;`, `,`, `=>` and a closing brace nothing goes on from, and an \
             attribute counts apart from its run: {text}"
        );
    }
}

#[test]
fn every_link_of_a_chain_the_parser_nests_counts_even_through_braces() {
    for (head, link) in [
        ("let _ = x", " + if {c} {1} else {2}"),
        ("let _ = x", " + {0}"),
        ("let _ = x", " + match {y} { _ => 0 }"),
        ("let _ = x", " + for _ in {0} {}"),
        ("let _ = x", " + loop {}"),
        ("let _ = x", " + unsafe {0}"),
        ("let _ = x", " + S { a: 0 }"),
        ("let _ = {x}", "(1)"),
        ("let _ = {x}", "[1]"),
        ("let _ = {x}", "?"),
        ("let _ = {x}", ".m()"),
        ("let _ = {x}", " as u8"),
        ("if c {}", " else if c {}"),
        ("let _ = ", "|| "),
        ("let _: ", "fn() -> "),
    ] {
        let counted =
            |links: usize| depth(&format!("fn f() {{ {head}{}0; }}\n", link.repeat(links))).chain;
        assert!(
            counted(51) > counted(50),
            "each link the parser nests one tree deeper is a token the measure counts, so no \
             chain passes the bound unmeasured: {head}{link}"
        );
    }
}

/// What came last in a group, as the reference reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Prior {
    Nothing,
    Hash,
    HashBang,
    Brace,
    Other,
}

/// How far `tokens` run, the rules written as recursion: the reference the measure's own walk is held to.
fn by_recursion(tokens: &TokenStream) -> Depth {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let (mut nesting, mut longest, mut run, mut held) = (0, 0, 0, 0);
    let mut prior = Prior::Nothing;
    let mut at = 0;
    while let Some(tree) = trees.get(at) {
        let goes_on = match tree {
            TokenTree::Group(_) => true,
            TokenTree::Punct(punct) => punct.as_char() != '#',
            TokenTree::Ident(ident) => ident == "as" || ident == "else",
            TokenTree::Literal(_) => false,
        };
        if prior == Prior::Brace && !goes_on {
            longest = longest.max(run + held);
            (run, held, prior) = (0, 0, Prior::Nothing);
        }
        match tree {
            TokenTree::Group(group) => {
                let inner = by_recursion(&group.stream());
                nesting = nesting.max(inner.nesting + 1);
                held = held.max(inner.chain);
                if group.delimiter() == Delimiter::Bracket
                    && matches!(prior, Prior::Hash | Prior::HashBang)
                {
                    prior = Prior::Other;
                } else {
                    run += 1;
                    prior = if group.delimiter() == Delimiter::Brace {
                        Prior::Brace
                    } else {
                        Prior::Other
                    };
                }
            }
            TokenTree::Punct(punct) => {
                let arrow = punct.as_char() == '='
                    && punct.spacing() == Spacing::Joint
                    && matches!(trees.get(at + 1), Some(TokenTree::Punct(next)) if next.as_char() == '>');
                if matches!(punct.as_char(), ';' | ',') || arrow {
                    if arrow {
                        at += 1;
                    }
                    longest = longest.max(run + held);
                    (run, held, prior) = (0, 0, Prior::Nothing);
                } else if punct.as_char() == '#' {
                    prior = Prior::Hash;
                } else if punct.as_char() == '!' && prior == Prior::Hash {
                    prior = Prior::HashBang;
                } else {
                    run += 1;
                    prior = Prior::Other;
                }
            }
            TokenTree::Ident(_) | TokenTree::Literal(_) => {
                run += 1;
                prior = Prior::Other;
            }
        }
        at += 1;
    }
    Depth {
        nesting,
        chain: longest.max(run + held),
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(4096))]

    #[test]
    fn the_measure_walks_the_tokens_as_the_rules_read_them(
        fragments in proptest::collection::vec(
            proptest::sample::select(vec![
                "(", ")", "[", "]", "{", "}", "\"(\"", "r#\"(\"#", "'('", "'a", "// ( \n",
                "/* ( */", "#", "!", "#[a]", "#![a]", "/// (\n", "//! (\n", ";", ",", "=>", "= >",
                "=", ">", "+", "-", "&&", "as", "else", "if", "fn", "a", "1", " ", "\n", "é",
            ]),
            0..48,
        )
    ) {
        let text: String = fragments.concat();
        match text.parse::<TokenStream>() {
            Ok(tokens) => prop_assert_eq!(Depth::of(&tokens), by_recursion(&tokens), "{:?}", text),
            Err(_not_tokens) => {}
        }
    }
}

#[test]
fn a_file_is_read_as_syn_reads_one_whatever_starts_it() {
    for prefix in [
        "",
        "\u{feff}",
        "#!/usr/bin/env run-cargo-script\n",
        "\u{feff}#!/usr/bin/env x\n",
        "#![allow(dead_code)]\n",
        "#! [allow(dead_code)]\n",
        "#!/* c */[allow(dead_code)]\n",
        "#!// c\n[allow(dead_code)]\n",
        "#!/// c\n[allow(dead_code)]\n",
        "#!\u{200e}[allow(dead_code)]\n",
        "#!/*/ c\n",
        "#!",
        "#!x",
        "#!/**/[a]\n",
    ] {
        let text = format!("{prefix}fn f() {{}}\n");
        let agreed = rust_mutants::parsing::apart(|parsing| {
            match (parsing.file(&text), syn::parse_file(&text)) {
                (Ok(ours), Ok(theirs)) => ours == theirs,
                (Err(_), Err(_)) => true,
                (Ok(_) | Err(_), Ok(_) | Err(_)) => false,
            }
        });
        assert!(
            matches!(agreed, Ok(true)),
            "a file is read as syn reads one, a byte order mark and a shebang taken off as it \
             takes them: {text:?}"
        );
    }
}

#[test]
fn the_remedy_names_the_bounds_a_reading_holds() {
    let code = rust_mutants::error::error_codes()
        .iter()
        .find(|code| code.code == "RM0020")
        .expect("RM0020 is a code");
    let remedy = code.remedy.expect("RM0020 says what to do");
    assert!(
        remedy.contains(&NESTING.to_string()) && remedy.contains(&CHAIN.to_string()),
        "a reader is told the bounds the reading holds, not ones it held once: {remedy}"
    );
}
