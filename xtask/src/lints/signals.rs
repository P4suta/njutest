// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one rule for signalling a process by its id, judged by what the code does with a name rather than by how the name is spelled.

use std::collections::{BTreeMap, BTreeSet};

use syn::visit::Visit;

use super::{Finding, Kind, item_attributes};

/// The shared owned native boundary for each platform is the only production place that may signal a process by id.
const SIGNALLERS: [&str; 2] = [
    "crates/njutest-process/src/unix.rs",
    "crates/njutest-process/src/windows.rs",
];

/// The file that states this rule, which spells every way to signal in order to refuse it.
const RULE: &str = "xtask/src/lints/signals.rs";

/// Every name a crate offers for sending a signal to a process or a group by its id: a function, a method, a system call's number.
const NAMES: [&str; 21] = [
    "kill",
    "killpg",
    "kill_process",
    "kill_process_group",
    "kill_current_process_group",
    "kill_with",
    "send_signal",
    "tgkill",
    "tkill",
    "sigqueue",
    "pidfd_send_signal",
    "proc_signal_with_audittoken",
    "SYS_kill",
    "SYS_tgkill",
    "SYS_tkill",
    "SYS_pidfd_send_signal",
    "SYS_rt_sigqueueinfo",
    "SYS_rt_tgsigqueueinfo",
    "TerminateProcess",
    "TerminateJobObject",
    "GenerateConsoleCtrlEvent",
];

/// The one signalling name that is also an ordinary word, which a binding, a field, or a method of a type this file writes may carry.
const WORD: &str = "kill";

/// The programs that signal the processes they are given, by the name any path to one ends in.
const PROGRAMS: [&str; 8] = [
    "kill",
    "pkill",
    "killall",
    "skill",
    "taskkill",
    "tskill",
    "stop-process",
    "spps",
];

/// The programs that start another program named among their own arguments.
const WRAPPERS: [&str; 16] = [
    "sudo", "doas", "env", "nohup", "nice", "ionice", "setsid", "stdbuf", "exec", "command",
    "builtin", "time", "xargs", "timeout", "busybox", "eval",
];

/// The programs besides the POSIX shell that run a script they are handed as text.
const SHELLS: [&str; 9] = [
    "bash",
    "zsh",
    "dash",
    "ksh",
    "mksh",
    "fish",
    "cmd",
    "powershell",
    "pwsh",
];

/// Whether `name` is a program that runs a script it is handed as text.
fn shell(name: &str) -> bool {
    name == "sh" || SHELLS.contains(&name)
}

/// The words that open or join a compound command rather than name a program.
const KEYWORDS: [&str; 8] = ["if", "then", "else", "elif", "do", "while", "until", "!"];

/// The macros that make whatever system call their text spells.
const ASSEMBLY: [&str; 3] = ["asm", "global_asm", "naked_asm"];

/// The functions that make the system call their first argument numbers.
const SYSCALLS: [&str; 2] = ["syscall", "__syscall"];

/// The methods of an `Option` or a `Result` that hand what it holds to the closure they are given.
const HANDING: [&str; 5] = ["map", "and_then", "is_some_and", "is_ok_and", "inspect"];

/// Whether `file` is held to the rule: everything the gate reads but the signallers, the rule itself, and a crate's suites.
pub(super) fn held(file: &str) -> bool {
    !SIGNALLERS.contains(&file) && file != RULE && !suite(file)
}

/// Whether `file` sits under a crate's `tests` directory, beside its `src` rather than inside it, where a test interrupts what it started the way a person would.
fn suite(file: &str) -> bool {
    file.split('/')
        .take_while(|segment| *segment != "src")
        .any(|segment| segment == "tests")
}

/// Every place `parsed` signals a process by its id, outside what it compiles only for tests.
pub(super) fn found(parsed: &syn::File, file: &str) -> Vec<Finding> {
    if compiled_only_for_tests(&parsed.attrs) {
        return Vec::new();
    }
    let declared = Declared::of(parsed);
    let mut signals = Signals {
        file,
        declared: &declared,
        tests: 0,
        scopes: Vec::new(),
        selves: Vec::new(),
        commands: BTreeMap::new(),
        chained: BTreeSet::new(),
        found: Vec::new(),
    };
    signals.visit_file(parsed);
    signals.found
}

/// Whether `attributes` compile what they sit on only for tests.
fn compiled_only_for_tests(attributes: &[syn::Attribute]) -> bool {
    attributes.iter().any(|attribute| match &attribute.meta {
        syn::Meta::List(list) if list.path.is_ident("cfg") => {
            list.tokens.to_string() == "test"
                || super::all_of(&list.tokens)
                    .is_some_and(|parts| parts.iter().any(|part| part.to_string() == "test"))
        }
        syn::Meta::List(_) | syn::Meta::Path(_) | syn::Meta::NameValue(_) => false,
    })
}

/// Whether `name` is one of the ways to signal by id.
fn signalling(name: &proc_macro2::Ident) -> bool {
    NAMES.iter().any(|one| name == one)
}

/// Whether `text` names a function that signals by id the way a symbol lookup spells it, rather than a word a sentence uses.
fn symbol(text: &str) -> bool {
    text != WORD && NAMES.contains(&text)
}

/// Whether `bytes`, less the NUL that ends a C string, name a function that signals by id.
fn symbol_bytes(bytes: &[u8]) -> bool {
    let name = match bytes.strip_suffix(&[0]) {
        Some(name) => name,
        None => bytes,
    };
    NAMES.iter().any(|one| one.as_bytes() == name)
}

/// Whether `attribute` links the declaration it sits on to a function that signals by id.
fn links_a_signal(attribute: &syn::Attribute) -> bool {
    match &attribute.meta {
        syn::Meta::NameValue(named) if named.path.is_ident("link_name") => matches!(
            &named.value,
            syn::Expr::Lit(syn::ExprLit { lit: syn::Lit::Str(name), .. })
                if NAMES.contains(&name.value().as_str())
        ),
        syn::Meta::NameValue(_) | syn::Meta::List(_) | syn::Meta::Path(_) => false,
    }
}

/// The name of the program `word` starts, whatever path reaches it, as a platform that ignores case spells it.
fn program_name(word: &str) -> String {
    let base = match word.rsplit(['/', '\\']).next() {
        Some(base) => base,
        None => word,
    };
    let lower = base.to_ascii_lowercase();
    match lower.strip_suffix(".exe") {
        Some(stem) => stem.to_owned(),
        None => lower,
    }
}

/// Whether `word` asks `shell` to run the argument after it as a script.
fn script_flag(shell: &str, word: &str) -> bool {
    let lower = word.to_ascii_lowercase();
    match shell {
        "cmd" => lower == "/c" || lower == "/k",
        "powershell" | "pwsh" => lower == "-command" || lower == "-c",
        _ => lower.starts_with('-') && !lower.starts_with("--") && lower.contains('c'),
    }
}

/// Whether `word` sets a variable for the command after it rather than naming a program.
fn assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(name, _value)| {
        !name.is_empty()
            && name
                .chars()
                .all(|letter| letter.is_ascii_alphanumeric() || letter == '_')
    })
}

/// The simple commands of `script`, each as its words with the quotes taken off, split wherever a shell starts another command.
fn simple_commands(script: &str) -> Vec<Vec<String>> {
    let mut split = Split {
        commands: vec![Vec::new()],
        word: String::new(),
    };
    let mut quote: Option<char> = None;
    let mut letters = script.chars().peekable();
    while let Some(letter) = letters.next() {
        match (quote, letter) {
            (Some(open), _) if letter == open => quote = None,
            (Some('"') | None, '\\') => {
                if let Some(escaped) = letters.next() {
                    split.word.push(escaped);
                }
            }
            (None, '\'' | '"') => quote = Some(letter),
            (None, ';' | '&' | '|' | '(' | ')' | '`' | '{' | '}' | '\n') => split.command(),
            (None, '$') if letters.peek() == Some(&'(') => split.command(),
            (None, _) if letter.is_whitespace() => split.end_word(),
            (Some(_) | None, _) => split.word.push(letter),
        }
    }
    split.end_word();
    split.commands
}

/// A script being split into its simple commands.
struct Split {
    /// Every command found so far, the last one still being read.
    commands: Vec<Vec<String>>,
    /// The word being read.
    word: String,
}

impl Split {
    /// Ends the word being read.
    fn end_word(&mut self) {
        if !self.word.is_empty()
            && let Some(command) = self.commands.last_mut()
        {
            command.push(std::mem::take(&mut self.word));
        }
    }

    /// Ends the command being read.
    fn command(&mut self) {
        self.end_word();
        self.commands.push(Vec::new());
    }
}

/// The name of the type `ty` is, by the last segment of its path, through references.
fn type_name(ty: &syn::Type) -> Option<String> {
    match ty {
        syn::Type::Path(path) => path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        syn::Type::Reference(reference) => type_name(&reference.elem),
        syn::Type::Paren(paren) => type_name(&paren.elem),
        syn::Type::Group(group) => type_name(&group.elem),
        _ => None,
    }
}

/// The first type argument `arguments` holds.
fn held_type(arguments: &syn::PathArguments) -> Option<&syn::Type> {
    let syn::PathArguments::AngleBracketed(bracketed) = arguments else {
        return None;
    };
    bracketed.args.iter().find_map(|argument| match argument {
        syn::GenericArgument::Type(held) => Some(held),
        _ => None,
    })
}

/// What a file says a value is.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Typed {
    /// A type the file names, by the last segment of its path.
    Named(String),
    /// An `Option` or a `Result` holding what it holds.
    Wrapped(Box<Self>),
    /// Nothing the file says.
    Unknown,
}

impl Typed {
    /// What `ty` says a value is, through references and boxes.
    fn of(ty: &syn::Type) -> Self {
        match ty {
            syn::Type::Reference(reference) => Self::of(&reference.elem),
            syn::Type::Paren(paren) => Self::of(&paren.elem),
            syn::Type::Group(group) => Self::of(&group.elem),
            syn::Type::Path(path) if path.qself.is_none() => {
                let Some(last) = path.path.segments.last() else {
                    return Self::Unknown;
                };
                let held = held_type(&last.arguments);
                if last.ident == "Option" || last.ident == "Result" {
                    held.map_or(Self::Unknown, |inner| {
                        Self::Wrapped(Box::new(Self::of(inner)))
                    })
                } else if last.ident == "Box" {
                    held.map_or(Self::Unknown, Self::of)
                } else {
                    Self::Named(last.ident.to_string())
                }
            }
            _ => Self::Unknown,
        }
    }

    /// What the `Option` or `Result` this is holds.
    fn unwrapped(self) -> Self {
        match self {
            Self::Wrapped(held) => *held,
            Self::Named(_) | Self::Unknown => Self::Unknown,
        }
    }

    /// Whether this is a type named `name`.
    fn is(&self, name: &str) -> bool {
        match self {
            Self::Named(named) => named == name,
            Self::Wrapped(_) | Self::Unknown => false,
        }
    }
}

/// One word of a command line as the file writes it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Word {
    /// Text the file spells, every value a name it binds might hold, and the line it is on.
    Text {
        /// The values.
        values: Vec<String>,
        /// The line.
        line: usize,
    },
    /// A word the file computes.
    Opaque,
}

impl Word {
    /// One spelled value on `line`.
    fn spelled(value: String, line: usize) -> Self {
        Self::Text {
            values: vec![value],
            line,
        }
    }

    /// Whether any value this word might hold passes `test`, and the line it is on.
    fn any(&self, test: impl Fn(&str) -> bool) -> Option<usize> {
        match self {
            Self::Text { values, line } => values.iter().any(|value| test(value)).then_some(*line),
            Self::Opaque => None,
        }
    }
}

/// What a file declares that judging it needs.
#[derive(Debug)]
struct Declared {
    /// Every method the file implements, by the type it implements it for.
    methods: BTreeMap<String, BTreeSet<String>>,
    /// Every named field of every struct the file declares, by struct.
    fields: BTreeMap<String, BTreeMap<String, syn::Type>>,
    /// Every string a constant, a static, or a binding of the file is given, by its name.
    texts: BTreeMap<String, Vec<String>>,
    /// Every function of the file that starts the program one of its parameters names, with that parameter's place.
    starters: BTreeMap<String, usize>,
    /// The names `Command` is known by here.
    commands: BTreeSet<String>,
    /// Whether the file imports some module whole, so a bare name may be any item of it.
    globbed: bool,
}

impl Declared {
    /// What `parsed` declares.
    fn of(parsed: &syn::File) -> Self {
        let mut declared = Self {
            methods: BTreeMap::new(),
            fields: BTreeMap::new(),
            texts: BTreeMap::new(),
            starters: BTreeMap::new(),
            commands: BTreeSet::from(["Command".to_owned()]),
            globbed: false,
        };
        declared.visit_file(parsed);
        declared
    }

    /// Records the string `name` is given, where it is given one.
    fn bind_text(&mut self, name: &proc_macro2::Ident, given: &syn::Expr) {
        if let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(text),
            ..
        }) = given
        {
            self.texts
                .entry(name.to_string())
                .or_default()
                .push(text.value());
        }
    }

    /// Records `signature` as a starter where `body` starts the program one of its parameters names.
    fn starter(&mut self, signature: &syn::Signature, body: &syn::Block) {
        let parameters: Vec<String> = signature
            .inputs
            .iter()
            .filter_map(|input| match input {
                syn::FnArg::Typed(typed) => bound_name(&typed.pat),
                syn::FnArg::Receiver(_) => None,
            })
            .collect();
        let mut started = Started {
            commands: &self.commands,
            names: Vec::new(),
        };
        started.visit_block(body);
        if let Some(place) = parameters
            .iter()
            .position(|parameter| started.names.contains(parameter))
        {
            self.starters.insert(signature.ident.to_string(), place);
        }
    }
}

impl Visit<'_> for Declared {
    fn visit_item_impl(&mut self, item: &syn::ItemImpl) {
        if let Some(owner) = type_name(&item.self_ty) {
            let methods = self.methods.entry(owner).or_default();
            for inner in &item.items {
                if let syn::ImplItem::Fn(method) = inner {
                    methods.insert(method.sig.ident.to_string());
                }
            }
        }
        syn::visit::visit_item_impl(self, item);
    }

    fn visit_item_struct(&mut self, item: &syn::ItemStruct) {
        if let syn::Fields::Named(named) = &item.fields {
            let fields = self.fields.entry(item.ident.to_string()).or_default();
            for field in &named.named {
                if let Some(name) = &field.ident {
                    fields.insert(name.to_string(), field.ty.clone());
                }
            }
        }
        syn::visit::visit_item_struct(self, item);
    }

    fn visit_item_const(&mut self, item: &syn::ItemConst) {
        self.bind_text(&item.ident, &item.expr);
        syn::visit::visit_item_const(self, item);
    }

    fn visit_item_static(&mut self, item: &syn::ItemStatic) {
        self.bind_text(&item.ident, &item.expr);
        syn::visit::visit_item_static(self, item);
    }

    fn visit_local(&mut self, local: &syn::Local) {
        if let (Some(name), Some(init)) = (bound_ident(&local.pat), &local.init) {
            self.bind_text(name, &init.expr);
        }
        syn::visit::visit_local(self, local);
    }

    fn visit_item_fn(&mut self, item: &syn::ItemFn) {
        self.starter(&item.sig, &item.block);
        syn::visit::visit_item_fn(self, item);
    }

    fn visit_impl_item_fn(&mut self, item: &syn::ImplItemFn) {
        self.starter(&item.sig, &item.block);
        syn::visit::visit_impl_item_fn(self, item);
    }

    fn visit_use_rename(&mut self, rename: &syn::UseRename) {
        if rename.ident == "Command" {
            self.commands.insert(rename.rename.to_string());
        }
    }

    fn visit_use_glob(&mut self, _glob: &syn::UseGlob) {
        self.globbed = true;
    }
}

/// Every name a body hands to `Command::new` whole.
struct Started<'a> {
    /// The names `Command` is known by.
    commands: &'a BTreeSet<String>,
    /// The names handed to it.
    names: Vec<String>,
}

impl Visit<'_> for Started<'_> {
    fn visit_expr_call(&mut self, call: &syn::ExprCall) {
        if let syn::Expr::Path(function) = call.func.as_ref()
            && starts_a_command(self.commands, &function.path)
            && let Some(syn::Expr::Path(program)) = call.args.first()
            && let Some(name) = program.path.get_ident()
        {
            self.names.push(name.to_string());
        }
        syn::visit::visit_expr_call(self, call);
    }
}

/// Whether `path` is `Command::new`, by any name `commands` says `Command` has here.
fn starts_a_command(commands: &BTreeSet<String>, path: &syn::Path) -> bool {
    let mut segments = path.segments.iter().rev();
    let (Some(new), Some(owner)) = (segments.next(), segments.next()) else {
        return false;
    };
    new.ident == "new" && commands.contains(&owner.ident.to_string())
}

/// The one name `pattern` binds, where it binds exactly one by itself.
fn bound_ident(pattern: &syn::Pat) -> Option<&proc_macro2::Ident> {
    match pattern {
        syn::Pat::Ident(ident) if ident.subpat.is_none() => Some(&ident.ident),
        syn::Pat::Type(typed) => bound_ident(&typed.pat),
        _ => None,
    }
}

/// The one name `pattern` binds, as text.
fn bound_name(pattern: &syn::Pat) -> Option<String> {
    bound_ident(pattern).map(ToString::to_string)
}

/// Whether `path` names what a `Some` or an `Ok` holds.
fn unwraps(path: &syn::Path) -> bool {
    path.segments
        .last()
        .is_some_and(|last| last.ident == "Some" || last.ident == "Ok")
}

/// Whether `first` is a system call named by its constant rather than by a number.
fn named_number(first: &syn::Expr) -> bool {
    matches!(first, syn::Expr::Path(path) if path.path.segments.last().is_some_and(|last| last.ident.to_string().starts_with("SYS_")))
}

/// Where `span` begins, as a key.
fn position(span: proc_macro2::Span) -> (usize, usize) {
    let start = span.start();
    (start.line, start.column)
}

/// Every name a pattern binds.
struct Names(Vec<String>);

impl Visit<'_> for Names {
    fn visit_pat_ident(&mut self, ident: &syn::PatIdent) {
        self.0.push(ident.ident.to_string());
        syn::visit::visit_pat_ident(self, ident);
    }
}

/// One walk of a file, knowing what each binding in scope is, which command line each names, and how deep in code compiled only for tests it is.
struct Signals<'a> {
    /// The file, for the findings.
    file: &'a str,
    /// What the file declares.
    declared: &'a Declared,
    /// How many enclosing items are compiled only for tests.
    tests: usize,
    /// What each binding in scope is, innermost last.
    scopes: Vec<BTreeMap<String, Typed>>,
    /// The type each enclosing `impl` is for, innermost last.
    selves: Vec<Option<String>>,
    /// The command line each binding of the function being read builds.
    commands: BTreeMap<String, Vec<Word>>,
    /// Where each method call already read as a link of a longer command line is.
    chained: BTreeSet<(usize, usize)>,
    /// Every finding.
    found: Vec<Finding>,
}

impl Signals<'_> {
    /// Records a signal on `line`, unless it is compiled only for tests.
    fn note(&mut self, line: usize) {
        if self.tests == 0 {
            self.found.push(Finding {
                kind: Kind::RawGroupSignal,
                file: self.file.to_owned(),
                line,
            });
        }
    }

    /// Records a signal where `span` begins.
    fn note_at(&mut self, span: proc_macro2::Span) {
        self.note(span.start().line);
    }

    /// Walks what `attributes` sit on, counting it when it is compiled only for tests.
    fn within(&mut self, attributes: &[syn::Attribute], walk: impl FnOnce(&mut Self)) {
        let tests = compiled_only_for_tests(attributes);
        if tests {
            self.tests = self.tests.saturating_add(1);
        }
        walk(self);
        if tests {
            self.tests = self.tests.saturating_sub(1);
        }
    }

    /// Walks a scope of its own.
    fn scoped(&mut self, walk: impl FnOnce(&mut Self)) {
        self.scopes.push(BTreeMap::new());
        walk(self);
        self.scopes.pop();
    }

    /// Walks a function, whose command lines are its own.
    fn function(&mut self, signature: &syn::Signature, walk: impl FnOnce(&mut Self)) {
        let outer = std::mem::take(&mut self.commands);
        self.scoped(|inner| {
            for input in &signature.inputs {
                if let syn::FnArg::Typed(typed) = input {
                    inner.bind(&typed.pat, Typed::of(&typed.ty));
                }
            }
            walk(inner);
        });
        self.commands = outer;
    }

    /// Says `name` is `typed` in the innermost scope.
    fn declare(&mut self, name: String, typed: Typed) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, typed);
        }
    }

    /// What the binding `name` is, as far as the file says.
    fn bound(&self, name: &str) -> Typed {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .cloned()
            .unwrap_or(Typed::Unknown)
    }

    /// The type of the `impl` being read.
    fn self_type(&self) -> Typed {
        self.selves
            .last()
            .and_then(Clone::clone)
            .map_or(Typed::Unknown, Typed::Named)
    }

    /// Binds every name `pattern` binds, each as what `typed` says of it.
    fn bind(&mut self, pattern: &syn::Pat, typed: Typed) {
        match pattern {
            syn::Pat::Ident(ident) => {
                self.declare(ident.ident.to_string(), typed);
                if let Some((_at, inner)) = &ident.subpat {
                    self.bind(inner, Typed::Unknown);
                }
            }
            syn::Pat::Type(declared) => self.bind(&declared.pat, Typed::of(&declared.ty)),
            syn::Pat::Reference(reference) => self.bind(&reference.pat, typed),
            syn::Pat::Paren(paren) => self.bind(&paren.pat, typed),
            syn::Pat::Guard(guarded) => self.bind(&guarded.pat, typed),
            syn::Pat::TupleStruct(tuple) if unwraps(&tuple.path) && tuple.elems.len() == 1 => {
                let held = typed.unwrapped();
                for element in &tuple.elems {
                    self.bind(element, held.clone());
                }
            }
            other => {
                let mut names = Names(Vec::new());
                names.visit_pat(other);
                for name in names.0 {
                    self.declare(name, Typed::Unknown);
                }
            }
        }
    }

    /// What `expression` is, as far as the file says.
    fn type_of(&self, expression: &syn::Expr) -> Typed {
        match expression {
            syn::Expr::Path(path) if path.qself.is_none() => match path.path.get_ident() {
                Some(name) if name == "self" => self.self_type(),
                Some(name) => self.bound(&name.to_string()),
                None => Typed::Unknown,
            },
            syn::Expr::Field(field) => self.field_type(&field.base, &field.member),
            syn::Expr::MethodCall(call) => self.returned(call),
            syn::Expr::Try(tried) => self.type_of(&tried.expr).unwrapped(),
            syn::Expr::Reference(reference) => self.type_of(&reference.expr),
            syn::Expr::Paren(paren) => self.type_of(&paren.expr),
            syn::Expr::Group(group) => self.type_of(&group.expr),
            syn::Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.type_of(&unary.expr)
            }
            _ => Typed::Unknown,
        }
    }

    /// What the field `member` of `base` is, where the file declares the struct.
    fn field_type(&self, base: &syn::Expr, member: &syn::Member) -> Typed {
        let (Typed::Named(owner), syn::Member::Named(name)) = (self.type_of(base), member) else {
            return Typed::Unknown;
        };
        self.declared
            .fields
            .get(&owner)
            .and_then(|fields| fields.get(&name.to_string()))
            .map_or(Typed::Unknown, Typed::of)
    }

    /// What `call` returns, where its method only passes on, opens, or starts what it is called on.
    fn returned(&self, call: &syn::ExprMethodCall) -> Typed {
        let receiver = self.type_of(&call.receiver);
        let method = call.method.to_string();
        match method.as_str() {
            "as_mut" | "as_ref" | "as_deref" | "as_deref_mut" | "take" | "by_ref" => receiver,
            "unwrap" | "expect" => receiver.unwrapped(),
            "spawn" if receiver.is("Command") => {
                Typed::Wrapped(Box::new(Typed::Named("Child".to_owned())))
            }
            _ => Typed::Unknown,
        }
    }

    /// Whether `method` of a value `typed` is one whose body this file shows, or the child's own `kill`, which knows whether it has reaped.
    fn answers(&self, typed: &Typed, method: &str) -> bool {
        match typed {
            Typed::Named(name) => {
                (name == "Child" && method == WORD)
                    || self
                        .declared
                        .methods
                        .get(name)
                        .is_some_and(|methods| methods.contains(method))
            }
            Typed::Wrapped(_) | Typed::Unknown => false,
        }
    }

    /// Records the signalling `method` of a value `typed`, unless it answers.
    fn judge_receiver(&mut self, typed: &Typed, method: &str, span: proc_macro2::Span) {
        if !self.answers(typed, method) {
            self.note_at(span);
        }
    }

    /// Judges a path in an expression: a module's signalling function, a type's signalling method, or a bare signalling name.
    fn judge_path(&mut self, qualified: Option<&syn::QSelf>, path: &syn::Path) {
        let Some(last) = path.segments.last() else {
            return;
        };
        if !signalling(&last.ident) {
            return;
        }
        let method = last.ident.to_string();
        let span = last.ident.span();
        if let Some(qualified) = qualified {
            self.judge_receiver(&Typed::of(&qualified.ty), &method, span);
            return;
        }
        match path.segments.iter().rev().nth(1) {
            None if method == WORD => {
                if self.declared.globbed {
                    self.note_at(span);
                }
            }
            None => self.note_at(span),
            Some(owner) if owner.ident == "Self" => {
                let typed = self.self_type();
                self.judge_receiver(&typed, &method, span);
            }
            Some(owner)
                if owner
                    .ident
                    .to_string()
                    .starts_with(|letter: char| letter.is_ascii_uppercase()) =>
            {
                self.judge_receiver(&Typed::Named(owner.ident.to_string()), &method, span);
            }
            Some(_module) => self.note_at(span),
        }
    }

    /// Judges a function call: a system call by number, a program started, a program handed to a function that starts it.
    fn judge_call(&mut self, call: &syn::ExprCall) {
        let syn::Expr::Path(function) = call.func.as_ref() else {
            return;
        };
        let Some(last) = function.path.segments.last() else {
            return;
        };
        if SYSCALLS.iter().any(|name| last.ident == name)
            && !call.args.first().is_some_and(named_number)
        {
            self.note_at(last.ident.span());
        }
        if starts_a_command(&self.declared.commands, &function.path)
            && let Some(program) = call.args.first()
        {
            let word = self.word(program);
            self.judge_argv(std::slice::from_ref(&word));
        }
        self.judge_started(&last.ident, &call.args);
    }

    /// Judges the argument a function of this file that starts a program is handed as that program.
    fn judge_started(
        &mut self,
        name: &proc_macro2::Ident,
        arguments: &syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>,
    ) {
        let Some(place) = self.declared.starters.get(&name.to_string()).copied() else {
            return;
        };
        if let Some(program) = arguments.iter().nth(place) {
            let word = self.word(program);
            self.judge_argv(std::slice::from_ref(&word));
        }
    }

    /// Judges the command line a chain of method calls builds, once, from its outermost link.
    fn judge_chain(&mut self, call: &syn::ExprMethodCall) {
        if !self.chained.insert(position(call.method.span())) {
            return;
        }
        let mut origin = call.receiver.as_ref();
        while let syn::Expr::MethodCall(inner) = origin {
            self.chained.insert(position(inner.method.span()));
            origin = inner.receiver.as_ref();
        }
        let Some(mut line) = self.command_line(&call.receiver) else {
            return;
        };
        line.extend(self.appended(call));
        if let syn::Expr::Path(root) = origin
            && let Some(name) = root.path.get_ident()
            && self.commands.contains_key(&name.to_string())
        {
            self.commands.insert(name.to_string(), line.clone());
        }
        self.judge_argv(&line);
    }

    /// The command line `expression` builds, where it starts at `Command::new` or at a binding that did.
    fn command_line(&self, expression: &syn::Expr) -> Option<Vec<Word>> {
        match expression {
            syn::Expr::Call(call) => {
                let syn::Expr::Path(function) = call.func.as_ref() else {
                    return None;
                };
                starts_a_command(&self.declared.commands, &function.path).then(|| {
                    call.args
                        .first()
                        .map(|program| vec![self.word(program)])
                        .unwrap_or_default()
                })
            }
            syn::Expr::MethodCall(call) => {
                let mut line = self.command_line(&call.receiver)?;
                line.extend(self.appended(call));
                Some(line)
            }
            syn::Expr::Path(path) => path
                .path
                .get_ident()
                .and_then(|name| self.commands.get(&name.to_string()))
                .cloned(),
            syn::Expr::Reference(reference) => self.command_line(&reference.expr),
            syn::Expr::Paren(paren) => self.command_line(&paren.expr),
            _ => None,
        }
    }

    /// The words `call` adds to a command line, where it is `arg` or `args`.
    fn appended(&self, call: &syn::ExprMethodCall) -> Vec<Word> {
        match (call.method.to_string().as_str(), call.args.first()) {
            ("arg", Some(argument)) => vec![self.word(argument)],
            ("args", Some(arguments)) => self.words(arguments),
            _ => Vec::new(),
        }
    }

    /// The words a sequence the file writes out holds, or one it computes.
    fn words(&self, sequence: &syn::Expr) -> Vec<Word> {
        match sequence {
            syn::Expr::Array(array) => array
                .elems
                .iter()
                .map(|element| self.word(element))
                .collect(),
            syn::Expr::Reference(reference) => self.words(&reference.expr),
            syn::Expr::Paren(paren) => self.words(&paren.expr),
            syn::Expr::Group(group) => self.words(&group.expr),
            syn::Expr::Macro(invocation) if invocation.mac.path.is_ident("vec") => {
                match arguments_of(&invocation.mac) {
                    Ok(elements) => elements.iter().map(|element| self.word(element)).collect(),
                    Err(_not_a_list) => vec![Word::Opaque],
                }
            }
            _ => vec![Word::Opaque],
        }
    }

    /// The word `expression` is: a string the file spells, the strings a name of it binds, or what a conversion of one of those is.
    fn word(&self, expression: &syn::Expr) -> Word {
        match expression {
            syn::Expr::Lit(literal) => match &literal.lit {
                syn::Lit::Str(text) => Word::spelled(text.value(), text.span().start().line),
                _ => Word::Opaque,
            },
            syn::Expr::Path(path) => match path.path.get_ident() {
                Some(name) => match self.declared.texts.get(&name.to_string()) {
                    Some(values) => Word::Text {
                        values: values.clone(),
                        line: name.span().start().line,
                    },
                    None => Word::Opaque,
                },
                None => Word::Opaque,
            },
            syn::Expr::Macro(invocation) => formatted(&invocation.mac),
            syn::Expr::Reference(reference) => self.word(&reference.expr),
            syn::Expr::Paren(paren) => self.word(&paren.expr),
            syn::Expr::Group(group) => self.word(&group.expr),
            syn::Expr::Call(call) if call.args.len() == 1 => match call.args.first() {
                Some(argument) => self.word(argument),
                None => Word::Opaque,
            },
            syn::Expr::MethodCall(call) if call.args.is_empty() => self.word(&call.receiver),
            _ => Word::Opaque,
        }
    }

    /// Judges a command line: its program, the program a wrapper starts, and the script a shell is handed.
    fn judge_argv(&mut self, words: &[Word]) {
        let Some((program, rest)) = words.split_first() else {
            return;
        };
        let Word::Text { values, line } = program else {
            return;
        };
        for value in values {
            let name = program_name(value);
            if PROGRAMS.contains(&name.as_str()) {
                self.note(*line);
            } else if WRAPPERS.contains(&name.as_str()) {
                self.judge_wrapped(rest);
            } else if shell(&name) {
                self.judge_shell(&name, rest);
            }
        }
    }

    /// Judges the arguments of a wrapper, any of which may be the program it starts.
    fn judge_wrapped(&mut self, rest: &[Word]) {
        for (at, word) in rest.iter().enumerate() {
            if let Some(line) = word.any(|value| PROGRAMS.contains(&program_name(value).as_str())) {
                self.note(line);
            }
            let starts = word
                .any(|value| {
                    let name = program_name(value);
                    shell(&name) || WRAPPERS.contains(&name.as_str())
                })
                .is_some();
            if starts {
                if let Some(started) = rest.get(at..) {
                    self.judge_argv(started);
                }
                return;
            }
        }
    }

    /// Judges the arguments of `shell`, the script among which is run as text.
    fn judge_shell(&mut self, shell: &str, rest: &[Word]) {
        let positional = shell == "powershell" || shell == "pwsh";
        let mut script_next = false;
        for word in rest {
            match word {
                Word::Text { values, line } => {
                    for value in values {
                        if script_next || (positional && !value.starts_with('-')) {
                            self.judge_script(value, *line);
                        }
                    }
                    script_next = values.iter().any(|value| script_flag(shell, value));
                }
                Word::Opaque => script_next = false,
            }
        }
    }

    /// Judges every simple command of `script`, as a shell would start it.
    fn judge_script(&mut self, script: &str, line: usize) {
        for command in simple_commands(script) {
            let words: Vec<Word> = command
                .into_iter()
                .skip_while(|word| KEYWORDS.contains(&word.as_str()) || assignment(word))
                .map(|word| Word::spelled(word, line))
                .collect();
            self.judge_argv(&words);
        }
    }

    /// Judges a macro: an assembly block, the elements of a `vec!`, and whatever else its arguments do, or its tokens where they are no arguments.
    fn judge_macro(&mut self, invocation: &syn::Macro) {
        let Some(last) = invocation.path.segments.last() else {
            return;
        };
        if ASSEMBLY.iter().any(|name| last.ident == name) {
            self.note_at(last.ident.span());
            return;
        }
        match arguments_of(invocation) {
            Ok(arguments) => {
                if last.ident == "vec" {
                    let words: Vec<Word> = arguments
                        .iter()
                        .map(|argument| self.word(argument))
                        .collect();
                    self.judge_argv(&words);
                }
                for argument in &arguments {
                    self.visit_expr(argument);
                }
            }
            Err(_not_arguments) => self.scan_tokens(&invocation.tokens),
        }
    }

    /// Every signalling name among `tokens` a macro that is not a call leaves unparsed, and every program it hands `Command::new`.
    fn scan_tokens(&mut self, tokens: &proc_macro2::TokenStream) {
        let trees: Vec<proc_macro2::TokenTree> = tokens.clone().into_iter().collect();
        for (at, tree) in trees.iter().enumerate() {
            match tree {
                proc_macro2::TokenTree::Ident(name) if signalling(name) => {
                    let pathed = at.checked_sub(1).and_then(|before| trees.get(before)).is_some_and(
                        |before| matches!(before, proc_macro2::TokenTree::Punct(colon) if colon.as_char() == ':'),
                    );
                    if *name != WORD || pathed {
                        self.note_at(name.span());
                    }
                }
                proc_macro2::TokenTree::Ident(name) if *name == "new" => {
                    self.scan_started(&trees, at);
                }
                proc_macro2::TokenTree::Group(group) => self.scan_tokens(&group.stream()),
                proc_macro2::TokenTree::Ident(_)
                | proc_macro2::TokenTree::Punct(_)
                | proc_macro2::TokenTree::Literal(_) => {}
            }
        }
    }

    /// Judges the program a `Command::new` at `at` among `trees` is handed, where the tokens spell it.
    fn scan_started(&mut self, trees: &[proc_macro2::TokenTree], at: usize) {
        let owner = at.checked_sub(3).and_then(|place| trees.get(place));
        let named = matches!(owner, Some(proc_macro2::TokenTree::Ident(owner)) if self.declared.commands.contains(&owner.to_string()));
        let Some(proc_macro2::TokenTree::Group(arguments)) =
            at.checked_add(1).and_then(|place| trees.get(place))
        else {
            return;
        };
        if !named {
            return;
        }
        let Some(proc_macro2::TokenTree::Literal(literal)) = arguments.stream().into_iter().next()
        else {
            return;
        };
        let spelled = proc_macro2::TokenStream::from(proc_macro2::TokenTree::Literal(literal));
        match syn::parse2::<syn::LitStr>(spelled) {
            Ok(text) => {
                let word = Word::spelled(text.value(), text.span().start().line);
                self.judge_argv(std::slice::from_ref(&word));
            }
            Err(_not_a_string) => {}
        }
    }

    /// Walks the closure a method of an `Option` or a `Result` hands what it holds to, knowing what that is.
    fn judge_handed(&mut self, call: &syn::ExprMethodCall) -> bool {
        if !HANDING.iter().any(|name| call.method == name) {
            return false;
        }
        let Some(syn::Expr::Closure(closure)) = call.args.first() else {
            return false;
        };
        let held = self.type_of(&call.receiver).unwrapped();
        self.visit_expr(&call.receiver);
        self.scoped(|inner| {
            for (place, input) in closure.inputs.iter().enumerate() {
                let typed = if place == 0 {
                    held.clone()
                } else {
                    Typed::Unknown
                };
                inner.bind(input, typed);
            }
            inner.visit_expr(&closure.body);
        });
        true
    }
}

/// The arguments of `invocation`, where it is a list of expressions.
fn arguments_of(
    invocation: &syn::Macro,
) -> syn::Result<syn::punctuated::Punctuated<syn::Expr, syn::Token![,]>> {
    invocation.parse_body_with(syn::punctuated::Punctuated::parse_terminated)
}

/// The text a formatting macro starts from, as the word it makes.
fn formatted(invocation: &syn::Macro) -> Word {
    let formats = invocation.path.segments.last().is_some_and(|last| {
        ["format", "concat", "format_args"]
            .iter()
            .any(|name| last.ident == name)
    });
    if !formats {
        return Word::Opaque;
    }
    match arguments_of(invocation) {
        Ok(arguments) => match arguments.first() {
            Some(syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Str(text),
                ..
            })) => Word::spelled(text.value(), text.span().start().line),
            _ => Word::Opaque,
        },
        Err(_not_arguments) => Word::Opaque,
    }
}

impl<'ast> Visit<'ast> for Signals<'_> {
    fn visit_item(&mut self, item: &'ast syn::Item) {
        self.within(item_attributes(item), |walk| {
            syn::visit::visit_item(walk, item);
        });
    }

    fn visit_impl_item(&mut self, item: &'ast syn::ImplItem) {
        let attributes: &[syn::Attribute] = match item {
            syn::ImplItem::Const(one) => &one.attrs,
            syn::ImplItem::Fn(one) => &one.attrs,
            syn::ImplItem::Type(one) => &one.attrs,
            syn::ImplItem::Macro(one) => &one.attrs,
            _ => &[],
        };
        self.within(attributes, |walk| syn::visit::visit_impl_item(walk, item));
    }

    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        self.selves.push(type_name(&item.self_ty));
        syn::visit::visit_item_impl(self, item);
        self.selves.pop();
    }

    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        self.function(&item.sig, |walk| syn::visit::visit_item_fn(walk, item));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        self.function(&item.sig, |walk| syn::visit::visit_impl_item_fn(walk, item));
    }

    fn visit_expr_closure(&mut self, closure: &'ast syn::ExprClosure) {
        self.scoped(|walk| {
            for input in &closure.inputs {
                walk.bind(input, Typed::Unknown);
            }
            walk.visit_expr(&closure.body);
        });
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.scoped(|walk| syn::visit::visit_block(walk, block));
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        let Some(init) = &local.init else {
            self.bind(&local.pat, Typed::Unknown);
            return;
        };
        self.visit_expr(&init.expr);
        if let Some((_else, diverge)) = &init.diverge {
            self.visit_expr(diverge);
        }
        let typed = self.type_of(&init.expr);
        if let (Some(name), Some(line)) = (bound_name(&local.pat), self.command_line(&init.expr)) {
            self.commands.insert(name, line);
        }
        self.bind(&local.pat, typed);
    }

    fn visit_expr_if(&mut self, expression: &'ast syn::ExprIf) {
        self.scoped(|walk| {
            walk.visit_expr(&expression.cond);
            walk.visit_block(&expression.then_branch);
        });
        if let Some((_else, otherwise)) = &expression.else_branch {
            self.visit_expr(otherwise);
        }
    }

    fn visit_expr_while(&mut self, expression: &'ast syn::ExprWhile) {
        self.scoped(|walk| {
            walk.visit_expr(&expression.cond);
            walk.visit_block(&expression.body);
        });
    }

    fn visit_expr_let(&mut self, expression: &'ast syn::ExprLet) {
        self.visit_expr(&expression.expr);
        let typed = self.type_of(&expression.expr);
        self.bind(&expression.pat, typed);
    }

    fn visit_expr_match(&mut self, expression: &'ast syn::ExprMatch) {
        self.visit_expr(&expression.expr);
        let typed = self.type_of(&expression.expr);
        for arm in &expression.arms {
            self.scoped(|walk| {
                walk.bind(&arm.pat, typed.clone());
                walk.visit_pat(&arm.pat);
                walk.visit_expr(&arm.body);
            });
        }
    }

    fn visit_expr_for_loop(&mut self, expression: &'ast syn::ExprForLoop) {
        self.visit_expr(&expression.expr);
        self.scoped(|walk| {
            walk.bind(&expression.pat, Typed::Unknown);
            walk.visit_block(&expression.body);
        });
    }

    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        self.judge_path(expression.qself.as_ref(), &expression.path);
        syn::visit::visit_expr_path(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        if signalling(&call.method) {
            let typed = self.type_of(&call.receiver);
            self.judge_receiver(&typed, &call.method.to_string(), call.method.span());
        }
        self.judge_chain(call);
        self.judge_started(&call.method, &call.args);
        if !self.judge_handed(call) {
            syn::visit::visit_expr_method_call(self, call);
        }
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        self.judge_call(call);
        syn::visit::visit_expr_call(self, call);
    }

    fn visit_expr_array(&mut self, array: &'ast syn::ExprArray) {
        let words: Vec<Word> = array
            .elems
            .iter()
            .map(|element| self.word(element))
            .collect();
        self.judge_argv(&words);
        syn::visit::visit_expr_array(self, array);
    }

    fn visit_macro(&mut self, invocation: &'ast syn::Macro) {
        self.judge_macro(invocation);
    }

    fn visit_use_name(&mut self, name: &'ast syn::UseName) {
        if signalling(&name.ident) {
            self.note_at(name.ident.span());
        }
    }

    fn visit_use_rename(&mut self, rename: &'ast syn::UseRename) {
        if signalling(&rename.ident) {
            self.note_at(rename.ident.span());
        }
    }

    fn visit_foreign_item_fn(&mut self, item: &'ast syn::ForeignItemFn) {
        if signalling(&item.sig.ident) || item.attrs.iter().any(links_a_signal) {
            self.note_at(item.sig.ident.span());
        }
        syn::visit::visit_foreign_item_fn(self, item);
    }

    fn visit_lit_str(&mut self, literal: &'ast syn::LitStr) {
        if symbol(&literal.value()) {
            self.note_at(literal.span());
        }
    }

    fn visit_lit_byte_str(&mut self, literal: &'ast syn::LitByteStr) {
        if symbol_bytes(&literal.value()) {
            self.note_at(literal.span());
        }
    }

    fn visit_lit_cstr(&mut self, literal: &'ast syn::LitCStr) {
        if symbol_bytes(literal.value().as_bytes()) {
            self.note_at(literal.span());
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::lints::{Kind, scan_source};

    #[test]
    fn process_generation_signals_are_confined_to_the_owned_native_boundary() {
        let source =
            "fn stop(token: AuditToken) { unsafe { proc_signal_with_audittoken(&token, 9); } }";
        let outside =
            scan_source("crates/app/src/lib.rs", source).expect("the signal specimen parses");
        assert!(
            outside
                .iter()
                .any(|finding| finding.kind == Kind::RawGroupSignal),
            "a generation-bound signal outside the native owner must be refused"
        );
        for path in [
            "crates/njutest-process/src/unix.rs",
            "crates/njutest-process/src/windows.rs",
        ] {
            assert!(
                !scan_source(path, source)
                    .expect("the boundary specimen parses")
                    .iter()
                    .any(|finding| finding.kind == Kind::RawGroupSignal),
                "the shared native owner must retain its actual signal boundary: {path}"
            );
        }
    }
}
