// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Whether an operator swap is the tree it names: the same operands, grouped as they were, joined by the new operator.

use crate::parsing::{Parsing, ReadingError};
use proc_macro2::{Punct, Spacing, TokenStream, TokenTree};
pub(super) use rust_mutants_decision::swap::Side;
use rust_mutants_decision::swap::{Binding, Operator};
use syn::spanned::Spanned as _;
use syn::visit_mut::VisitMut;
use syn::{BinOp, Expr};

/// The operator `op` writes, or nothing for an operator this release does not know.
const fn operator(op: &BinOp) -> Option<Operator> {
    Some(match op {
        BinOp::Mul(_) => Operator::Mul,
        BinOp::Div(_) => Operator::Div,
        BinOp::Rem(_) => Operator::Rem,
        BinOp::Add(_) => Operator::Add,
        BinOp::Sub(_) => Operator::Sub,
        BinOp::Shl(_) => Operator::Shl,
        BinOp::Shr(_) => Operator::Shr,
        BinOp::BitAnd(_) => Operator::BitAnd,
        BinOp::BitXor(_) => Operator::BitXor,
        BinOp::BitOr(_) => Operator::BitOr,
        BinOp::Eq(_) => Operator::Eq,
        BinOp::Ne(_) => Operator::Ne,
        BinOp::Lt(_) => Operator::Lt,
        BinOp::Le(_) => Operator::Le,
        BinOp::Gt(_) => Operator::Gt,
        BinOp::Ge(_) => Operator::Ge,
        BinOp::And(_) => Operator::And,
        BinOp::Or(_) => Operator::Or,
        BinOp::AddAssign(_) => Operator::AddAssign,
        BinOp::SubAssign(_) => Operator::SubAssign,
        BinOp::MulAssign(_) => Operator::MulAssign,
        BinOp::DivAssign(_) => Operator::DivAssign,
        BinOp::RemAssign(_) => Operator::RemAssign,
        BinOp::BitXorAssign(_) => Operator::BitXorAssign,
        BinOp::BitAndAssign(_) => Operator::BitAndAssign,
        BinOp::BitOrAssign(_) => Operator::BitOrAssign,
        BinOp::ShlAssign(_) => Operator::ShlAssign,
        BinOp::ShrAssign(_) => Operator::ShrAssign,
        _ => return None,
    })
}

/// How tightly `op` binds, or nothing for an operator this release does not know.
pub(super) const fn binding(op: &BinOp) -> Option<Binding> {
    match operator(op) {
        Some(operator) => Some(Binding::of(operator)),
        None => None,
    }
}

/// Whether `operand`, written as it is on `side` of an operator binding as `new` does, would be read as a different operand.
/// Only an operator between operands can be regrouped: every other kind of expression binds tighter than any binary operator or had to be parenthesized to stand there at all.
pub(super) fn regroups(operand: &Expr, side: Side, new: Binding) -> bool {
    let Expr::Binary(inner) = operand else {
        return false;
    };
    binding(&inner.op).is_none_or(|inner| rust_mutants_decision::swap::regroups(inner, side, new))
}

/// Takes every parenthesis and invisible group out of what it visits.
struct Ungroup;

impl VisitMut for Ungroup {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        loop {
            let inner = match expr {
                Expr::Paren(paren) => (*paren.expr).clone(),
                Expr::Group(group) => (*group.expr).clone(),
                _ => break,
            };
            *expr = inner;
        }
        syn::visit_mut::visit_expr_mut(self, expr);
    }
}

/// Replaces the operator of the one binary expression whose operator starts at `at`, and says whether it found one.
struct Swap<'a> {
    at: usize,
    new: &'a BinOp,
    found: bool,
}

impl VisitMut for Swap<'_> {
    fn visit_expr_binary_mut(&mut self, binary: &mut syn::ExprBinary) {
        if binary.op.span().byte_range().start == self.at {
            binary.op = *self.new;
            self.found = true;
        }
        syn::visit_mut::visit_expr_binary_mut(self, binary);
    }
}

/// One item as the file reads it where nothing but items encloses it, each kind parsed as its container parses it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Unit {
    /// An item of the file or of an inline module.
    Free(syn::Item),
    /// A member of an `impl` block.
    OfImpl(syn::ImplItem),
    /// A member of a trait.
    OfTrait(syn::TraitItem),
    /// A member of an `extern` block.
    Foreign(syn::ForeignItem),
}

impl Unit {
    /// `tokens`, lexed by `parsing`, read as an item of this unit's kind, or nothing where they do not read as one whole.
    ///
    /// # Errors
    /// The tokens could not be read at all, which is not an answer about them.
    fn read_tokens_as(
        &self,
        parsing: &Parsing,
        tokens: TokenStream,
    ) -> Result<Option<Self>, ReadingError> {
        Self::whole(match self {
            Self::Free(_) => parsing.read_tokens(tokens).map(Self::Free),
            Self::OfImpl(_) => parsing.read_tokens(tokens).map(Self::OfImpl),
            Self::OfTrait(_) => parsing.read_tokens(tokens).map(Self::OfTrait),
            Self::Foreign(_) => parsing.read_tokens(tokens).map(Self::Foreign),
        })
    }

    /// A unit read whole, nothing where what was read is not one, and the failure back where it could not be read at all.
    fn whole(read: Result<Self, ReadingError>) -> Result<Option<Self>, ReadingError> {
        match read {
            Ok(unit) => Ok(Some(unit)),
            Err(ReadingError::Syntax { .. }) => Ok(None),
            Err(
                unread @ (ReadingError::Exhausted { .. }
                | ReadingError::ThreadUnavailable { .. }
                | ReadingError::TooDeep { .. }),
            ) => Err(unread),
        }
    }

    /// This unit with every parenthesis and invisible group taken out, so two units compare equal exactly when they hold the same tree.
    fn ungrouped(mut self) -> Self {
        match &mut self {
            Self::Free(item) => Ungroup.visit_item_mut(item),
            Self::OfImpl(item) => Ungroup.visit_impl_item_mut(item),
            Self::OfTrait(item) => Ungroup.visit_trait_item_mut(item),
            Self::Foreign(item) => Ungroup.visit_foreign_item_mut(item),
        }
        self
    }

    /// This unit with the operator starting at byte `at` replaced by `new`, or nothing where no operator starts there.
    fn swapped(&self, at: usize, new: &BinOp) -> Option<Self> {
        let mut swapped = self.clone();
        let mut swap = Swap {
            at,
            new,
            found: false,
        };
        match &mut swapped {
            Self::Free(item) => swap.visit_item_mut(item),
            Self::OfImpl(item) => swap.visit_impl_item_mut(item),
            Self::OfTrait(item) => swap.visit_trait_item_mut(item),
            Self::Foreign(item) => swap.visit_foreign_item_mut(item),
        }
        swap.found.then_some(swapped)
    }

    /// The statement that holds `edit` in the innermost block of this unit that does, with the bytes of that block's braces, where the statement read alone reads as its block reads it: it ends with a `;`, or it ends the block.
    fn statement_holding(
        &self,
        edit: &std::ops::Range<usize>,
    ) -> Option<(std::ops::Range<usize>, &syn::Stmt)> {
        let mut innermost = Innermost {
            edit: edit.clone(),
            found: None,
        };
        match self {
            Self::Free(item) => syn::visit::Visit::visit_item(&mut innermost, item),
            Self::OfImpl(item) => syn::visit::Visit::visit_impl_item(&mut innermost, item),
            Self::OfTrait(item) => syn::visit::Visit::visit_trait_item(&mut innermost, item),
            Self::Foreign(item) => syn::visit::Visit::visit_foreign_item(&mut innermost, item),
        }
        let (block, at) = innermost.found?;
        let statement = block.stmts.get(at)?;
        if statement.span().byte_range().start > edit.start {
            return None;
        }
        let settled = match statement {
            syn::Stmt::Local(_) | syn::Stmt::Expr(_, Some(_)) => true,
            syn::Stmt::Expr(_, None) | syn::Stmt::Item(_) | syn::Stmt::Macro(_) => {
                at.checked_add(1) == Some(block.stmts.len())
            }
        };
        settled.then(|| (block.brace_token.span.join().byte_range(), statement))
    }

    /// The bytes of the file this unit spans, attributes included.
    fn bytes(&self) -> std::ops::Range<usize> {
        match self {
            Self::Free(item) => item.span().byte_range(),
            Self::OfImpl(item) => item.span().byte_range(),
            Self::OfTrait(item) => item.span().byte_range(),
            Self::Foreign(item) => item.span().byte_range(),
        }
    }
}

/// One unit of the file, where it stands and the tree it holds, parentheses and all, so every node of it spans the bytes it was read from.
#[derive(Debug)]
struct Leaf {
    bytes: std::ops::Range<usize>,
    unit: Unit,
    /// Its tokens, lexed from its own text the first time a swap in it is held to it.
    tokens: std::cell::OnceCell<TokenStream>,
    /// The tokens inside each block of it a swap was held to a statement of, by the bytes of the block's braces in its text.
    blocks: std::cell::RefCell<std::collections::BTreeMap<(usize, usize), Vec<TokenTree>>>,
}

impl Leaf {
    /// `unit`, where it stands.
    fn of(unit: Unit) -> Self {
        Self {
            bytes: unit.bytes(),
            unit,
            tokens: std::cell::OnceCell::new(),
            blocks: std::cell::RefCell::new(std::collections::BTreeMap::new()),
        }
    }
}

/// How the lexer joins the tokens either side of an edit to what follows them: the punctuation mark just before it to the edit's first character, and the one it ends with, where it ends with one, to the text after it.
#[derive(Debug, Clone, Copy)]
struct Seams {
    before: Spacing,
    after: Option<Spacing>,
}

impl Seams {
    /// The seams of `written` between `head` and `tail`, or nothing where a character either side could run into it as one token or open a comment, which lexing the edit alone would not see.
    fn between(head: &str, written: &str, tail: &str) -> Option<Self> {
        let pairs = [
            (head.chars().next_back(), written.chars().next()),
            (written.chars().next_back(), tail.chars().next()),
        ];
        if pairs.iter().any(|pair| match pair {
            (Some(left), Some(right)) => runs_into(*left, *right),
            (None, _) | (_, None) => false,
        }) {
            return None;
        }
        Some(Self {
            before: joined(written),
            after: written
                .chars()
                .next_back()
                .is_some_and(punctuates)
                .then(|| joined(tail)),
        })
    }
}

/// Whether `right` written right after `left` lexes as part of the same token as it, or opens a comment with it.
fn runs_into(left: char, right: char) -> bool {
    let names = |character: char| character.is_alphanumeric() || character == '_';
    ((names(left) || matches!(left, '"' | '\'')) && names(right))
        || (names(left) && matches!(right, '"' | '\'' | '#'))
        || (left == '/' && matches!(right, '/' | '*'))
}

/// Whether the lexer reads `character` as punctuation.
fn punctuates(character: char) -> bool {
    "~!@#$%^&*-=+|;:,<.>/?'".contains(character)
}

/// How the lexer joins a punctuation mark to `text` right after it: to punctuation that opens no comment.
fn joined(text: &str) -> Spacing {
    if text.chars().next().is_some_and(punctuates)
        && !text.starts_with("//")
        && !text.starts_with("/*")
    {
        Spacing::Joint
    } else {
        Spacing::Alone
    }
}

/// `tree` joined to what follows it as `spacing` says, where it is a punctuation mark.
fn rejoined(tree: TokenTree, spacing: Spacing) -> TokenTree {
    match tree {
        TokenTree::Punct(punct) => {
            let mut joined = Punct::new(punct.as_char(), spacing);
            joined.set_span(punct.span());
            TokenTree::Punct(joined)
        }
        other @ (TokenTree::Group(_) | TokenTree::Ident(_) | TokenTree::Literal(_)) => other,
    }
}

/// `tokens` with the whole tokens of one group that `edit` covers replaced by `written`, each seam joined as the lexer joins it in the text the edit is made in, or nothing where `edit` covers no such run of tokens.
fn spliced(
    tokens: &TokenStream,
    edit: &std::ops::Range<usize>,
    written: &TokenStream,
    seams: Seams,
) -> Option<TokenStream> {
    let trees: Vec<TokenTree> = tokens.clone().into_iter().collect();
    let holds = |tree: &TokenTree| match tree {
        TokenTree::Group(group) => {
            group.span_open().byte_range().end <= edit.start
                && edit.end <= group.span_close().byte_range().start
        }
        TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => false,
    };
    if let Some(inside) = trees.iter().position(holds) {
        return trees
            .into_iter()
            .enumerate()
            .map(|(at, tree)| match tree {
                TokenTree::Group(group) if at == inside => {
                    let mut rebuilt = proc_macro2::Group::new(
                        group.delimiter(),
                        spliced(&group.stream(), edit, written, seams)?,
                    );
                    rebuilt.set_span(group.span());
                    Some(TokenTree::Group(rebuilt))
                }
                other @ (TokenTree::Group(_)
                | TokenTree::Ident(_)
                | TokenTree::Punct(_)
                | TokenTree::Literal(_)) => Some(other),
            })
            .collect();
    }
    let first = trees
        .iter()
        .position(|tree| tree.span().byte_range().start == edit.start)?;
    let last = trees
        .iter()
        .position(|tree| tree.span().byte_range().end == edit.end)?;
    if last < first {
        return None;
    }
    let mut written: Vec<TokenTree> = written.clone().into_iter().collect();
    if let Some(after) = seams.after
        && let Some(end) = written.pop()
    {
        written.push(rejoined(end, after));
    }
    let mut out = Vec::with_capacity(trees.len().saturating_add(written.len()));
    for (at, tree) in trees.into_iter().enumerate() {
        if at == first {
            out.append(&mut written);
        }
        if (first..=last).contains(&at) {
            continue;
        }
        let touching =
            at.checked_add(1) == Some(first) && tree.span().byte_range().end == edit.start;
        out.push(if touching {
            rejoined(tree, seams.before)
        } else {
            tree
        });
    }
    Some(out.into_iter().collect())
}

/// Every unit of `items` in file order, descending into what only holds items: inline modules, `impl` blocks, traits and `extern` blocks.
fn leaves(items: &[syn::Item], found: &mut Vec<Leaf>) {
    for item in items {
        match item {
            syn::Item::Mod(module) => match &module.content {
                Some((_, inner)) => leaves(inner, found),
                None => found.push(Leaf::of(Unit::Free(item.clone()))),
            },
            syn::Item::Impl(block) => found.extend(
                block
                    .items
                    .iter()
                    .map(|member| Leaf::of(Unit::OfImpl(member.clone()))),
            ),
            syn::Item::Trait(block) => found.extend(
                block
                    .items
                    .iter()
                    .map(|member| Leaf::of(Unit::OfTrait(member.clone()))),
            ),
            syn::Item::ForeignMod(block) => found.extend(
                block
                    .items
                    .iter()
                    .map(|member| Leaf::of(Unit::Foreign(member.clone()))),
            ),
            other => found.push(Leaf::of(Unit::Free(other.clone()))),
        }
    }
}

/// The units of one file, which every operator swap in it is held to.
///
/// A file is its items read one after another, and each is read by its own tokens up to its own closing brace or semicolon, so an edit inside one unit changes that unit's tree and no other: holding a swap to its unit holds it to the file, at the cost of the unit rather than of the file.
/// A block's statements are read the same way where one ends with a `;` or ends its block, so a swap inside such a statement is held to it at the cost of the statement, and only its edit is lexed again.
#[derive(Debug)]
pub(super) struct Grouping<'p> {
    leaves: Vec<Leaf>,
    read: std::cell::Cell<Option<usize>>,
    parsing: &'p Parsing,
    #[cfg(any(test, feature = "testkit"))]
    planted_accept_wrong: bool,
}

impl<'p> Grouping<'p> {
    /// The units `file` holds, read back with `parsing`.
    pub(super) fn of(file: &syn::File, parsing: &'p Parsing) -> Self {
        let mut found = Vec::new();
        leaves(&file.items, &mut found);
        Self {
            leaves: found,
            read: std::cell::Cell::new(Some(0)),
            parsing,
            #[cfg(any(test, feature = "testkit"))]
            planted_accept_wrong: false,
        }
    }

    #[cfg(any(test, feature = "testkit"))]
    pub(super) const fn planted(mut self) -> Self {
        self.planted_accept_wrong = true;
        self
    }

    /// How many bytes of source holding every swap has lexed or parsed again, or nothing once that stopped fitting.
    pub(super) const fn read(&self) -> Option<usize> {
        self.read.get()
    }

    /// How many units read alone as the file `text` reads them, and the bytes of every one that does not.
    ///
    /// # Errors
    /// A unit could not be read at all, which says nothing about how it reads.
    #[cfg(any(test, feature = "testkit"))]
    pub(super) fn read_alone(
        &self,
        text: &str,
    ) -> Result<(usize, Vec<std::ops::Range<usize>>), ReadingError> {
        let mut alike = 0_usize;
        let mut differing = Vec::new();
        for leaf in &self.leaves {
            let read = match text.get(leaf.bytes.clone()) {
                Some(unit) => match self.parsing.tokens(unit) {
                    Ok(tokens) => leaf.unit.read_tokens_as(self.parsing, tokens)?,
                    Err(ReadingError::Syntax { .. }) => None,
                    Err(
                        unread @ (ReadingError::Exhausted { .. }
                        | ReadingError::ThreadUnavailable { .. }
                        | ReadingError::TooDeep { .. }),
                    ) => return Err(unread),
                },
                None => None,
            };
            if read.is_some_and(|read| read.ungrouped() == leaf.unit.clone().ungrouped()) {
                alike = alike.saturating_add(1);
            } else {
                differing.push(leaf.bytes.clone());
            }
        }
        Ok((alike, differing))
    }

    /// Counts `bytes` more of source read back.
    fn reread(&self, bytes: usize) {
        self.read
            .set(self.read.get().and_then(|read| read.checked_add(bytes)));
    }

    /// The tokens of `leaf`, whose text `unit` is, lexed the first time they are asked for; nothing where the text alone is not tokens.
    ///
    /// # Errors
    /// The unit could not be read at all.
    fn tokens_of(&self, leaf: &Leaf, unit: &str) -> Result<Option<TokenStream>, ReadingError> {
        if let Some(tokens) = leaf.tokens.get() {
            return Ok(Some(tokens.clone()));
        }
        self.reread(unit.len());
        match self.parsing.tokens(unit) {
            Ok(tokens) => Ok(Some(leaf.tokens.get_or_init(|| tokens).clone())),
            Err(ReadingError::Syntax { .. }) => Ok(None),
            Err(
                unread @ (ReadingError::Exhausted { .. }
                | ReadingError::ThreadUnavailable { .. }
                | ReadingError::TooDeep { .. }),
            ) => Err(unread),
        }
    }

    /// Whether `text` with `edit` rewritten as `written` reads as this file's tree with exactly the operator starting at byte `at` replaced by `new`: the operands it had, grouped as they were.
    /// An edit no single unit holds is one this cannot vouch for, and it says so; the unit is lexed once and each edit alone again, spliced into its tokens where the lexer would join them.
    ///
    /// # Errors
    /// The unit could not be read back at all, which is not an answer about the swap.
    pub(super) fn keeps(
        &self,
        text: &str,
        (edit, written): (std::ops::Range<usize>, &str),
        (at, new): (usize, &BinOp),
    ) -> Result<bool, ReadingError> {
        let after = self
            .leaves
            .partition_point(|leaf| leaf.bytes.end < edit.end);
        let Some(leaf) = self
            .leaves
            .get(after)
            .filter(|leaf| leaf.bytes.start <= edit.start && edit.end <= leaf.bytes.end)
        else {
            return Ok(false);
        };
        let (Some(head), Some(tail), Some(unit), Some(start), Some(end)) = (
            text.get(leaf.bytes.start..edit.start),
            text.get(edit.end..leaf.bytes.end),
            text.get(leaf.bytes.clone()),
            edit.start.checked_sub(leaf.bytes.start),
            edit.end.checked_sub(leaf.bytes.start),
        ) else {
            return Ok(false);
        };
        let Some(seams) = Seams::between(head, written, tail) else {
            return Ok(false);
        };
        let Some(tokens) = self.tokens_of(leaf, unit)? else {
            return Ok(false);
        };
        self.reread(written.len());
        let window = match self.parsing.tokens(written) {
            Ok(window) => window,
            Err(ReadingError::Syntax { .. }) => return Ok(false),
            Err(
                unread @ (ReadingError::Exhausted { .. }
                | ReadingError::ThreadUnavailable { .. }
                | ReadingError::TooDeep { .. }),
            ) => return Err(unread),
        };
        let splice = Splice {
            bytes: start..end,
            written: &window,
            seams,
            at,
            new,
        };
        let alone = match leaf.unit.statement_holding(&edit) {
            Some(statement) => self.statement_keeps((leaf, &tokens), statement, &splice)?,
            None => Alone::Unit,
        };
        let kept = match alone {
            Alone::Kept => true,
            Alone::Changed => false,
            Alone::Unit => self.unit_keeps((leaf, &tokens), &splice)?,
        };
        #[cfg(any(test, feature = "testkit"))]
        if self.planted_accept_wrong {
            return Ok(true);
        }
        Ok(kept)
    }

    /// Whether the statement `statement` holds, of the block whose braces `statement` names, reads alone as its block reads it with `splice` made in the tokens of `leaf`; nothing where it cannot be read alone.
    ///
    /// # Errors
    /// The statement could not be read back at all.
    fn statement_keeps(
        &self,
        (leaf, tokens): (&Leaf, &TokenStream),
        (braces, statement): (std::ops::Range<usize>, &syn::Stmt),
        splice: &Splice<'_>,
    ) -> Result<Alone, ReadingError> {
        let base = leaf.bytes.start;
        let relative = |range: std::ops::Range<usize>| {
            Some(range.start.checked_sub(base)?..range.end.checked_sub(base)?)
        };
        let (Some(braces), Some(span)) =
            (relative(braces), relative(statement.span().byte_range()))
        else {
            return Ok(Alone::Unit);
        };
        let own: TokenStream = {
            let mut blocks = leaf.blocks.borrow_mut();
            let trees = match blocks.entry((braces.start, braces.end)) {
                std::collections::btree_map::Entry::Occupied(read) => read.into_mut(),
                std::collections::btree_map::Entry::Vacant(unread) => {
                    let Some(block) = group_at(tokens, &braces) else {
                        return Ok(Alone::Unit);
                    };
                    unread.insert(block.stream().into_iter().collect())
                }
            };
            let from = trees.partition_point(|tree| tree.span().byte_range().start < span.start);
            let to = trees.partition_point(|tree| tree.span().byte_range().end <= span.end);
            let Some(own) = trees.get(from..to) else {
                return Ok(Alone::Unit);
            };
            own.iter().cloned().collect()
        };
        let Some(swapped) = spliced(&own, &splice.bytes, splice.written, splice.seams) else {
            return Ok(Alone::Unit);
        };
        let mut expected = statement.clone();
        let mut swap = Swap {
            at: splice.at,
            new: splice.new,
            found: false,
        };
        swap.visit_stmt_mut(&mut expected);
        if !swap.found {
            return Ok(Alone::Unit);
        }
        Ungroup.visit_stmt_mut(&mut expected);
        self.reread(span.len());
        let mut read = match self
            .parsing
            .read_tokens_with(syn::Block::parse_within, swapped)
        {
            Ok(read) => read,
            Err(ReadingError::Syntax { .. }) => return Ok(Alone::Changed),
            Err(
                unread @ (ReadingError::Exhausted { .. }
                | ReadingError::ThreadUnavailable { .. }
                | ReadingError::TooDeep { .. }),
            ) => return Err(unread),
        };
        let [read] = read.as_mut_slice() else {
            return Ok(Alone::Changed);
        };
        Ungroup.visit_stmt_mut(read);
        Ok(if *read == expected {
            Alone::Kept
        } else {
            Alone::Changed
        })
    }

    /// Whether the unit of `leaf` reads as the file reads it with `splice` made in its `tokens`.
    ///
    /// # Errors
    /// The unit could not be read back at all.
    fn unit_keeps(
        &self,
        (leaf, tokens): (&Leaf, &TokenStream),
        splice: &Splice<'_>,
    ) -> Result<bool, ReadingError> {
        let Some(swapped) = spliced(tokens, &splice.bytes, splice.written, splice.seams) else {
            return Ok(false);
        };
        self.reread(leaf.bytes.len());
        let (Some(read), Some(expected)) = (
            leaf.unit.read_tokens_as(self.parsing, swapped)?,
            leaf.unit.swapped(splice.at, splice.new),
        ) else {
            return Ok(false);
        };
        Ok(expected.ungrouped() == read.ungrouped())
    }
}

/// What reading a swap's statement alone answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Alone {
    /// The statement reads alone as the swap names it.
    Kept,
    /// It reads, or fails to read, as anything else.
    Changed,
    /// It cannot be read alone as its block reads it, so its unit is read instead.
    Unit,
}

/// One swap's edit to splice into the tokens it is made in: the bytes of the unit's text it covers, what it writes there, lexed alone, how the lexer joins its seams, and the operator it says it swaps, the one starting at byte `at` of the file, for `new`.
struct Splice<'a> {
    bytes: std::ops::Range<usize>,
    written: &'a TokenStream,
    seams: Seams,
    at: usize,
    new: &'a BinOp,
}

/// The group of `tokens` whose bytes are exactly `span`, however deep it stands.
fn group_at(tokens: &TokenStream, span: &std::ops::Range<usize>) -> Option<proc_macro2::Group> {
    tokens.clone().into_iter().find_map(|tree| match tree {
        TokenTree::Group(group) => {
            let bytes = group.span().byte_range();
            if bytes == *span {
                Some(group)
            } else if bytes.start <= span.start && span.end <= bytes.end {
                group_at(&group.stream(), span)
            } else {
                None
            }
        }
        TokenTree::Ident(_) | TokenTree::Punct(_) | TokenTree::Literal(_) => None,
    })
}

/// The walk that finds the innermost block whose braces hold an edit.
struct Innermost<'u> {
    edit: std::ops::Range<usize>,
    /// The innermost block so far, and where among its statements the one that could hold the edit stands.
    found: Option<(&'u syn::Block, usize)>,
}

impl<'u> syn::visit::Visit<'u> for Innermost<'u> {
    fn visit_block(&mut self, block: &'u syn::Block) {
        let braces = block.brace_token.span;
        if braces.open().byte_range().end > self.edit.start
            || self.edit.end > braces.close().byte_range().start
        {
            return;
        }
        let at = block
            .stmts
            .partition_point(|statement| statement.span().byte_range().end < self.edit.end);
        self.found = Some((block, at));
        if let Some(statement) = block.stmts.get(at) {
            syn::visit::visit_stmt(self, statement);
        }
    }
}
