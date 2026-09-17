// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a person is told about a run, as a value every surface is a projection of.

pub mod agent;
pub mod human;
mod telling;
pub mod tint;

use crate::report::Verdict;

/// What a person is told about one run: the answer, and the things they act on.
///
/// Held apart from how it is drawn so that a terminal, a pipe, a document and
/// a review loop are four functions of one value rather than four accounts of
/// a report (ADR 0020).
#[derive(Debug, Clone)]
pub struct Told {
    /// The answer and the numbers that support it.
    pub headline: Headline,
    /// Where the tests are blind, one entry per item of the source.
    pub places: Vec<Place>,
    /// What stopped the run, or what it found that is not about an item.
    pub diagnostics: Vec<Diagnostic>,
    /// What the run could not establish, which is not what it found.
    pub limitations: Vec<Stated>,
}

/// Something a run could not establish, which it says rather than passes over.
///
/// Held apart from the diagnostics because it is a different thing to be told:
/// a gap is the tests', and a limitation is the run's own, and a reader
/// counting the first does not want the second mixed into the number.
#[derive(Debug, Clone)]
pub struct Stated {
    /// The stable name a reader can look up.
    pub name: String,
    /// What it means for what the run claims.
    pub detail: String,
}

/// The answer a run reached, and what it is made of.
#[derive(Debug, Clone)]
pub struct Headline {
    /// What the run concluded.
    pub verdict: Verdict,
    /// The project it is about, as a reader names it.
    pub project: String,
    /// How many mutations the run cataloged.
    pub cataloged: u32,
    /// How many a test noticed.
    pub killed: u32,
    /// How many ran and nothing noticed.
    pub survived: u32,
    /// How many nothing reached.
    pub unreached: u32,
    /// How long the whole run took.
    pub duration_ms: u64,
    /// Where the run's own record was kept, from the project's root.
    pub kept: String,
}

/// How much of a reader's attention one thing deserves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// The tests have a gap: the thing this is all for.
    Gap,
    /// The run could not establish something, and says so rather than passing over it.
    Limitation,
    /// The run could not proceed.
    Refusal,
}

impl Severity {
    /// The word a diagnostic's first line begins with.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Gap => "gap",
            Self::Limitation => "limitation",
            Self::Refusal => "refused",
        }
    }
}

/// One thing a run found, in the shape a compiler has taught every reader to read.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// How much attention it deserves.
    pub severity: Severity,
    /// The stable name of this kind of finding, which `njutest explain` answers about.
    pub code: &'static str,
    /// What happened, in one line.
    pub title: String,
    /// Where in the source it is, when it is anywhere.
    pub at: Option<Site>,
    /// What a reader needs to know that the title does not say.
    pub notes: Vec<String>,
    /// What a reader can do about it, each one a command they can run.
    pub actions: Vec<Action>,
}

/// One item of the source, with every place the tests were blind to marked on it.
///
/// The unit a reader acts on, because it is the unit they think in. Three
/// survivors in one function are not three problems; they are one function
/// whose boundaries nothing checks, and a reader who is handed them as three
/// entries in a list has to put that back together themselves. Drawn once,
/// with every blind spot on it at the same time, the shape is the thing you
/// see first.
#[derive(Debug, Clone)]
pub struct Place {
    /// The item the blind spots are in, as the source names it.
    pub item: String,
    /// The path from the project's root.
    pub path: String,
    /// The lines to draw, in order, each with its own number.
    pub excerpt: Vec<(u32, String)>,
    /// Why the lines are not being drawn, when they are not.
    pub instead: Option<Excerpt>,
    /// Every place in it the tests did not see, in the order the source has them.
    pub spots: Vec<Spot>,
}

/// One place the tests did not see.
#[derive(Debug, Clone)]
pub struct Spot {
    /// The line, counting from one.
    pub line: u32,
    /// The column, counting from one.
    pub column: u32,
    /// What the code says there.
    pub was: String,
    /// What the run made it say instead.
    pub now: String,
    /// What the run established about it, in the fewest words that are true.
    pub said: String,
    /// Which kind of blindness it is, which is what a reader does something different about.
    pub blindness: Blindness,
    /// How a reader names it again after they have edited the file.
    pub locator: String,
}

/// The kinds of not-seeing, which are different things to do something about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blindness {
    /// A test ran the code and did not notice the change: the test is weak.
    Ran,
    /// No test runs the code at all: the test is missing.
    Never,
    /// It ran out of time rather than answering.
    Waited,
}

impl Blindness {
    /// What the mark under the code looks like, which is how a reader tells them apart at a glance.
    #[must_use]
    pub const fn word(self) -> &'static str {
        match self {
            Self::Ran => "ran, not noticed",
            Self::Never => "never run",
            Self::Waited => "timed out",
        }
    }

    /// The same, in the words a count stands in front of.
    ///
    /// No comma in any of them: a heading that reads `1 ran, not noticed, 1
    /// never run` parses on sight as four things rather than two, and a
    /// heading is read at a glance or not at all.
    #[must_use]
    pub const fn counted(self) -> &'static str {
        match self {
            Self::Ran => "not noticed",
            Self::Never => "never run",
            Self::Waited => "timed out",
        }
    }

    /// What a reader is being asked to do about it, which is not the same for all three.
    ///
    /// A test that notices a change, a test that reaches the line at all, and
    /// an investigation into why nothing finished are three different pieces
    /// of work. Drawing them under one heading, or handing all three the same
    /// instruction, teaches somebody something false about two of them.
    #[must_use]
    pub const fn asks(self) -> &'static str {
        match self {
            Self::Ran => "write a test that notices it",
            Self::Never => "write a test that reaches it",
            Self::Waited => "find out why nothing finished",
        }
    }
}

/// Where a diagnostic is, and the line it is on.
#[derive(Debug, Clone)]
pub struct Site {
    /// The path from the project's root.
    pub path: String,
    /// The line, counting from one.
    pub line: u32,
    /// The column, counting from one.
    pub column: u32,
    /// The line itself, when it can still be shown.
    pub excerpt: Excerpt,
    /// What to say under the caret.
    pub label: String,
    /// How many characters the caret covers.
    pub width: usize,
}

/// The line a diagnostic is about, or why it is not being shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Excerpt {
    /// The line as it was when the run measured it.
    Read(String),
    /// The file has changed since the run, so the line there now is not the line that was.
    Moved,
    /// The file could not be read at all.
    Unreadable,
}

/// Something a reader can do about a diagnostic.
#[derive(Debug, Clone)]
pub struct Action {
    /// What doing it accomplishes, in a word.
    pub said: String,
    /// The command that does it, which has to work when it is typed.
    pub command: String,
}

/// The characters a drawing is made of, which depend on what the font has.
///
/// Two sets rather than one: the box-drawing set reads as a drawn thing and
/// the ASCII set reads as `rustc`, and a terminal that renders `\u{2502}` as a
/// replacement glyph would make the second set the only legible one.
#[derive(Debug, Clone, Copy)]
pub struct Strokes {
    /// The vertical rule down the left of an excerpt.
    pub rule: &'static str,
    /// The corner that opens a site.
    pub opening: &'static str,
    /// The corner that opens a note.
    pub branch: &'static str,
    /// The corner that closes the last note.
    pub closing: &'static str,
    /// The rule beside the line the caret is on.
    pub beside: &'static str,
    /// What points at the code a diagnostic is about.
    pub point: &'static str,
    /// What stands between what the code said and what the run made it say.
    pub becomes: &'static str,
    /// What opens the place a diagnostic is at.
    pub before: &'static str,
    /// What closes it.
    pub after: &'static str,
    /// What a verdict that found nothing is marked with.
    pub well: &'static str,
    /// What a verdict that found something is marked with.
    pub unwell: &'static str,
}

/// The box-drawing set, for a terminal whose font has it.
const DRAWN: Strokes = Strokes {
    rule: "\u{2502}",
    opening: "\u{256d}\u{2500}",
    branch: "\u{251c}\u{2500}",
    closing: "\u{2570}\u{2500}",
    beside: "\u{00b7}",
    point: "\u{25b2}",
    becomes: "\u{21e2}",
    before: "[",
    after: "]",
    well: "\u{2713} ",
    unwell: "\u{2717} ",
};

/// The set `rustc` uses, which every terminal has.
const PLAIN: Strokes = Strokes {
    rule: "|",
    opening: "-->",
    branch: "=",
    closing: "=",
    beside: "|",
    point: "^",
    becomes: "=>",
    before: " ",
    after: "",
    well: "",
    unwell: "",
};

/// What the composition root learned about where the output is going.
///
/// The width, the colour and the glyphs are the environment's to know and
/// `main.rs`'s to ask (ADR 0001); a renderer that asked would be a seam, and
/// one that guessed would be wrong on the machine that mattered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Terminal {
    /// How many columns there are to write across.
    pub width: usize,
    /// Whether colour is wanted.
    pub colour: bool,
    /// Whether the font is expected to have more than ASCII.
    pub unicode: bool,
    /// Whether a person is reading this, rather than a program.
    ///
    /// The one thing that is not a capability: it decides which projection is
    /// written at all, and a pipe gets the record stream because that is a
    /// contract with whatever is on the other end of it.
    pub drawing: bool,
}

impl Default for Terminal {
    /// What a stream nobody has said anything about gets, which is what a pipe gets.
    fn default() -> Self {
        Self {
            drawing: false,
            ..Self::plain(ROOM)
        }
    }
}

impl Terminal {
    /// A terminal `width` columns across that takes neither colour nor anything but ASCII, which is what a test asserts against and what a pipe gets.
    #[must_use]
    pub const fn plain(width: usize) -> Self {
        Self {
            width,
            colour: false,
            unicode: false,
            drawing: true,
        }
    }
}

/// How one thing is set apart from the things around it.
///
/// Named by what it is for rather than by the colour it happens to be, so a
/// palette is one table to read rather than a number at every call site.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// A gap in the tests: the thing a run is for.
    Gap,
    /// Something the run could not establish.
    Limitation,
    /// Something that stopped the run.
    Refusal,
    /// A run that found nothing.
    Well,
    /// A part of a catalog, which assures nothing on its own.
    Partial,
    /// The rules and gutters a drawing is made of, which the eye should pass over.
    Frame,
    /// A command a reader is meant to type.
    Command,
    /// A name a reader is meant to notice.
    Subject,
    /// The bytes a run replaced, lit where they are in the code.
    Changed,
    /// A word the language reserves.
    Keyword,
    /// A type, which in Rust begins with a capital.
    Type,
    /// A string or a character.
    Text,
    /// A number.
    Number,
    /// A comment.
    Aside,
    /// A name being called.
    Call,
    /// A lifetime or an attribute.
    Marker,
    /// Code that is none of the above.
    Code,
}

impl Style {
    /// What to write before the text, as the parameters of one escape.
    ///
    /// Amber, teal and rose rather than the sixteen a theme redefines: a
    /// palette chosen once and read the same on every terminal that has 256
    /// colours, which is every terminal anybody has used this decade.
    #[must_use]
    pub const fn code(self) -> &'static str {
        match self {
            Self::Gap => "1;38;5;214",
            Self::Limitation => "38;5;73",
            Self::Refusal => "1;38;5;203",
            Self::Well => "1;38;5;114",
            Self::Partial => "38;5;110",
            Self::Frame => "38;5;244",
            Self::Command => "38;5;252",
            Self::Subject => "1;38;5;253",
            Self::Changed => "1;4;38;5;204",
            Self::Keyword => "38;5;176",
            Self::Type => "38;5;79",
            Self::Text => "38;5;150",
            Self::Number => "38;5;179",
            Self::Aside => "3;38;5;243",
            Self::Call => "38;5;111",
            Self::Marker => "38;5;139",
            Self::Code => "38;5;250",
        }
    }
}

/// How a terminal's capabilities turn into the characters written to it.
///
/// Every decision about colour and glyphs is made here, so a renderer reads as
/// what it is saying rather than as escape codes, and a test asserts the
/// decision once rather than at every line that made it.
#[derive(Debug, Clone, Copy)]
pub struct Telling {
    terminal: Terminal,
}

impl Telling {
    /// How to write for `terminal`.
    #[must_use]
    pub const fn of(terminal: Terminal) -> Self {
        Self { terminal }
    }

    /// The characters this terminal's drawings are made of.
    #[must_use]
    pub const fn strokes(self) -> Strokes {
        if self.terminal.unicode { DRAWN } else { PLAIN }
    }

    /// `text` under `style`, or plainly where colour is not wanted.
    ///
    /// The palette is the 256-colour one rather than the sixteen a theme
    /// redefines, so amber is amber on a terminal somebody has made their own.
    #[must_use]
    pub fn painted(self, style: Style, text: &str) -> String {
        if !self.terminal.colour {
            return text.to_owned();
        }
        format!("\u{1b}[{}m{text}\u{1b}[0m", style.code())
    }

    /// `text` as a link to `target`, where the terminal follows one.
    ///
    /// A path a reader can click is a file they do not have to find, and a
    /// terminal that does not know the sequence shows the text and drops the
    /// rest, so this costs nothing where it does nothing.
    #[must_use]
    pub fn linked(self, target: &str, text: &str) -> String {
        if !self.terminal.colour {
            return text.to_owned();
        }
        format!("\u{1b}]8;;{target}\u{7}{text}\u{1b}]8;;\u{7}")
    }

    /// What a diagnostic's first line begins with.
    #[must_use]
    pub fn severity(self, severity: Severity, code: &str) -> String {
        let style = match severity {
            Severity::Gap => Style::Gap,
            Severity::Limitation => Style::Limitation,
            Severity::Refusal => Style::Refusal,
        };
        format!(
            "{}{}{}:",
            self.painted(style, severity.word()),
            self.painted(Style::Frame, "["),
            self.painted(style, &format!("{code}{}", self.painted(Style::Frame, "]")))
        )
    }

    /// The word a run's answer goes by, with the mark that says which kind of answer it is.
    #[must_use]
    pub fn verdict(self, verdict: Verdict) -> String {
        let strokes = self.strokes();
        let (style, mark) = match verdict {
            Verdict::Assured | Verdict::ChangeAssured | Verdict::ScopeAssured => {
                (Style::Well, strokes.well)
            }
            Verdict::Defect | Verdict::Error => (Style::Refusal, strokes.unwell),
            Verdict::Insufficient => (Style::Gap, strokes.unwell),
            Verdict::Partial => (Style::Partial, strokes.beside),
        };
        self.painted(style, &format!("{mark}{}", verdict.name()))
    }

    /// A command a reader is meant to type, which is worth marking as one.
    #[must_use]
    pub fn command(self, command: &str) -> String {
        self.painted(Style::Command, command)
    }

    /// Where a diagnostic is, as a reader's terminal can open it.
    #[must_use]
    pub fn where_at(self, site: &Site) -> String {
        let said = format!("{}:{}:{}", site.path, site.line, site.column);
        let linked = self.linked(&format!("file://{}", site.path), &said);
        let strokes = self.strokes();
        format!(
            "{}{}{}",
            self.frame(strokes.before),
            self.painted(Style::Subject, &linked),
            self.frame(strokes.after)
        )
    }

    /// A rule, a corner or a gutter, which the eye should pass over.
    #[must_use]
    pub fn frame(self, stroke: &str) -> String {
        self.painted(Style::Frame, stroke)
    }

    /// The caret under the part of `line` a diagnostic is about, and the label beside it.
    ///
    /// The indent is how wide the line is up to the column *on a terminal*,
    /// which is neither how many bytes it is nor how many `char`s: `見出し` is
    /// three characters, nine bytes, and six columns, and a caret that counted
    /// either of the first two would land three columns short of the code it
    /// is about.
    #[must_use]
    pub fn caret(self, site: &Site, gutter: usize, line: &str) -> Vec<String> {
        let before: String = line
            .chars()
            .take(usize::try_from(site.column.saturating_sub(1)).unwrap_or(usize::MAX))
            .collect();
        let indent = wide(&before);
        let width = site.width.max(1);
        let painted = self.painted(Style::Gap, &self.strokes().point.repeat(width));
        self.marked(
            &painted,
            Marked {
                indent,
                width,
                gutter,
            },
            &site.label,
        )
    }

    /// A mark at the column it is about, and what to say about it, beside it or under it.
    ///
    /// Beside is where a reader looks first, and it is where the label goes
    /// while there is room for it. A caret far enough to the right leaves no
    /// room, and a label hung under it there is folded to nothing and drawn
    /// past the edge anyway; below the mark it is narrow but whole.
    fn marked(self, mark: &str, at: Marked, label: &str) -> Vec<String> {
        let Marked {
            indent,
            width,
            gutter,
        } = at;
        let hanging = indent.saturating_add(width).saturating_add(1);
        let room = self.room(gutter.saturating_add(3).saturating_add(hanging));
        if room < BESIDE {
            let under = gutter.saturating_add(5);
            let mut lines = vec![format!("{:indent$}{mark}", "")];
            lines.extend(
                folded(label, self.room(under))
                    .into_iter()
                    .map(|one| format!("  {one}")),
            );
            return lines;
        }
        let mut folded = folded(label, room).into_iter();
        let first = folded.next().unwrap_or_default();
        let mut lines = vec![format!("{:indent$}{mark} {first}", "")];
        lines.extend(folded.map(|rest| format!("{:hanging$}{rest}", "")));
        lines
    }

    /// `line` with the bytes each spot replaced painted where they are.
    ///
    /// A mark under the code says where to look; the code itself saying it is
    /// what a reader sees without looking. Painting adds no columns, so
    /// everything under the line still lands where it was placed.
    #[must_use]
    pub fn lit(self, line: &str, spots: &[&Spot]) -> String {
        if !self.terminal.colour {
            return line.to_owned();
        }
        if spots.is_empty() {
            return self.tinted(line);
        }
        let characters: Vec<char> = line.chars().collect();
        let mut painted = String::new();
        let mut at = 0usize;
        let mut spots: Vec<&&Spot> = spots.iter().collect();
        spots.sort_by_key(|spot| spot.column);
        for spot in spots {
            let from = usize::try_from(spot.column.saturating_sub(1)).unwrap_or(usize::MAX);
            let width = spot.was.chars().count().max(1);
            let to = from.saturating_add(width).min(characters.len());
            if from < at || from >= characters.len() {
                continue;
            }
            let before: String = characters
                .get(at..from)
                .unwrap_or_default()
                .iter()
                .collect();
            painted.push_str(&self.tinted(&before));
            let span: String = characters
                .get(from..to)
                .unwrap_or_default()
                .iter()
                .collect();
            painted.push_str(&self.painted(Style::Changed, &span));
            at = to;
        }
        let after: String = characters.get(at..).unwrap_or_default().iter().collect();
        painted.push_str(&self.tinted(&after));
        painted
    }

    /// `line` with each part of the language in a colour of its own.
    #[must_use]
    pub fn tinted(self, line: &str) -> String {
        if !self.terminal.colour {
            return line.to_owned();
        }
        tint::parts(line)
            .into_iter()
            .map(|(part, text)| {
                let style = match part {
                    tint::Part::Keyword => Style::Keyword,
                    tint::Part::Type => Style::Type,
                    tint::Part::Text => Style::Text,
                    tint::Part::Number => Style::Number,
                    tint::Part::Aside => Style::Aside,
                    tint::Part::Call => Style::Call,
                    tint::Part::Marker => Style::Marker,
                    tint::Part::Code => Style::Code,
                };
                self.painted(style, &text)
            })
            .collect()
    }

    /// The mark under one blind spot, and what it says, folded to the terminal.
    ///
    /// What the code says and what the run made it say, side by side: a reader
    /// who is told only that a rule fired has to know the rule, and a reader
    /// shown `> \u{21e2} >=` knows what was asked of their tests without knowing
    /// anything.
    #[must_use]
    pub fn spot(self, spot: &Spot, gutter: usize, line: &str) -> Vec<String> {
        let before: String = line
            .chars()
            .take(usize::try_from(spot.column.saturating_sub(1)).unwrap_or(usize::MAX))
            .collect();
        let indent = wide(&before);
        let width = spot.was.chars().count().max(1);
        let strokes = self.strokes();
        let mark = self.painted(Style::Gap, &strokes.point.repeat(width));
        let change = changed(spot);
        let change = match change {
            Changed::Gone => self.painted(Style::Refusal, "deleted"),
            Changed::Into(now) => format!(
                "{} {}",
                self.painted(Style::Gap, strokes.becomes),
                self.tinted(now)
            ),
            Changed::From(was, now) => format!(
                "{} {} {}",
                self.tinted(was),
                self.painted(Style::Gap, strokes.becomes),
                self.tinted(now)
            ),
        };
        let style = match spot.blindness {
            Blindness::Ran => Style::Gap,
            Blindness::Never => Style::Refusal,
            Blindness::Waited => Style::Limitation,
        };
        let said = format!("{change}   {}", self.painted(style, &spot.said));
        self.marked(
            &mark,
            Marked {
                indent,
                width,
                gutter,
            },
            &said,
        )
    }

    /// How many columns there are for prose after `indent` has been spent.
    #[must_use]
    pub const fn room(self, indent: usize) -> usize {
        self.terminal.width.saturating_sub(indent)
    }
}

/// What to say beside the mark, which is not always both halves of the change.
///
/// The mark already covers what the code said, and the line is above it, so
/// echoing a whole deleted statement beside its own caret is saying it three
/// times. Short enough to take in at a glance — an operator, a method name —
/// and both halves side by side are the clearest thing there is.
enum Changed<'a> {
    /// The run took it out.
    Gone,
    /// It became this, and what it was is long enough that the mark says it better.
    Into(&'a str),
    /// It was this and became that, which fits.
    From(&'a str, &'a str),
}

/// How wide something can be and still be worth saying twice.
const AT_A_GLANCE: usize = 16;

/// What one spot's label has to say.
fn changed(spot: &Spot) -> Changed<'_> {
    if spot.now.trim().is_empty() {
        return Changed::Gone;
    }
    if wide(&spot.was) > AT_A_GLANCE {
        return Changed::Into(&spot.now);
    }
    Changed::From(&spot.was, &spot.now)
}

/// How many columns `text` takes on a terminal, counting nothing for what it is painted with.
///
/// An escape sequence moves the cursor nowhere, so a folder that counted its
/// bytes would break a line that fits and a caret placed after it would land
/// short. Every measurement here goes through this.
#[must_use]
pub fn wide(text: &str) -> usize {
    let mut width: usize = 0;
    let mut characters = text.chars();
    while let Some(one) = characters.next() {
        if one != '\u{1b}' {
            width = width.saturating_add(unicode_width::UnicodeWidthChar::width(one).unwrap_or(0));
            continue;
        }
        for inside in characters.by_ref() {
            if inside.is_ascii_alphabetic() || inside == '\u{7}' {
                break;
            }
        }
    }
    width
}

/// `text` broken so that no line is wider than `room`, breaking only between words.
///
/// A note that ran past the edge was the same note wrapped by the terminal at
/// whatever column it happened to reach, which put the continuation under the
/// first character of the line and lost the shape the rest of the drawing
/// depends on.
///
/// The spaces inside a line are kept as they were, because a run of them is
/// how one thing is set apart from the next: collapsing them is how `a  b`
/// becomes `a b` and a column of aligned labels stops being one.
#[must_use]
pub fn folded(text: &str, room: usize) -> Vec<String> {
    if room == 0 || wide(text) <= room {
        return vec![text.to_owned()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let would = if line.is_empty() {
            wide(word)
        } else {
            wide(&line).saturating_add(1).saturating_add(wide(word))
        };
        if !line.is_empty() && would > room {
            lines.push(std::mem::take(&mut line.trim_end().to_owned()));
            line.clear();
        }
        if !line.is_empty() || word.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    let line = line.trim_end().to_owned();
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// The lines a drawing needs, read once before anything is drawn.
///
/// Read up front and passed in rather than opened where a line is wanted: a
/// renderer that touched the filesystem could not be asserted without one, and
/// the same value serves a test that supplies its own files.
#[derive(Debug, Clone, Default)]
pub struct Sources(std::collections::BTreeMap<String, Vec<String>>);

impl Sources {
    /// Every line of the files `report` names, so a place can be drawn whole.
    #[must_use]
    pub fn span(&self, path: &str, from: u32, to: u32) -> Vec<(u32, String)> {
        let Some(lines) = self.0.get(path) else {
            return Vec::new();
        };
        let first = usize::try_from(from)
            .unwrap_or(usize::MAX)
            .saturating_sub(1);
        let last = usize::try_from(to).unwrap_or(usize::MAX).min(lines.len());
        (first..last)
            .filter_map(|at| {
                lines.get(at).map(|line| {
                    (
                        u32::try_from(at.saturating_add(1)).unwrap_or(u32::MAX),
                        line.clone(),
                    )
                })
            })
            .collect()
    }

    /// The files `report` names, read from `root`. A file that cannot be read is one whose lines are not shown.
    #[must_use]
    pub fn read(root: &std::path::Path, report: &crate::report::Report) -> Self {
        let wanted: std::collections::BTreeSet<&str> = report
            .findings
            .iter()
            .filter_map(|finding| finding.path.as_deref())
            .chain(report.mutants.iter().map(|mutant| mutant.path.as_str()))
            .filter(|path| !path.is_empty())
            .collect();
        Self(
            wanted
                .into_iter()
                .filter_map(|path| {
                    std::fs::read_to_string(root.join(path))
                        .ok()
                        .map(|text| (path.to_owned(), lines_of(&text)))
                })
                .collect(),
        )
    }

    /// The files a test supplies, as path and contents.
    #[must_use]
    pub fn of<I, P, T>(files: I) -> Self
    where
        I: IntoIterator<Item = (P, T)>,
        P: Into<String>,
        T: AsRef<str>,
    {
        Self(
            files
                .into_iter()
                .map(|(path, text)| (path.into(), lines_of(text.as_ref())))
                .collect(),
        )
    }

    /// The line a diagnostic is about, or why it is not being shown.
    ///
    /// A line that no longer holds the bytes the run replaced is a line the
    /// file has moved out from under, and drawing it would show a reader code
    /// the run never measured.
    #[must_use]
    pub fn at(&self, path: &str, line: u32, original: &str) -> Excerpt {
        let Some(lines) = self.0.get(path) else {
            return Excerpt::Unreadable;
        };
        let at = usize::try_from(line)
            .unwrap_or(usize::MAX)
            .saturating_sub(1);
        let Some(text) = lines.get(at) else {
            return Excerpt::Moved;
        };
        if !original.is_empty() && !text.contains(original) {
            return Excerpt::Moved;
        }
        Excerpt::Read(text.clone())
    }
}

/// `text` as lines, without their terminators and without a carriage return a Windows checkout left.
fn lines_of(text: &str) -> Vec<String> {
    text.lines()
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect()
}

/// Who is on the other end of the output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Reader {
    /// A person at a terminal.
    Person,
    /// Something that will parse it.
    #[default]
    Program,
}

/// What was said about colour, which is three answers rather than two flags.
///
/// `NO_COLOR` and `CLICOLOR_FORCE` are not independent — both set means forced
/// — so they are one question with three answers rather than two booleans a
/// caller can set to a combination nobody meant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Wanted {
    /// `CLICOLOR_FORCE`: colour even where nothing else asks for it.
    Forced,
    /// `NO_COLOR`: none, whatever else is true.
    Refused,
    /// Nothing said, so whether the reader is a person decides.
    #[default]
    Unsaid,
}

/// What the font is expected to have.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Glyphs {
    /// More than ASCII.
    Drawn,
    /// ASCII, which every terminal has.
    #[default]
    Plain,
}

/// What the composition root found out about where the output is going.
///
/// The pieces rather than the conclusion, so the rules that turn them into a
/// [`Terminal`] — which of `NO_COLOR` and `CLICOLOR_FORCE` wins, what a `dumb`
/// terminal means, what to do when nothing says how wide it is — are asserted
/// in a test rather than believed.
#[derive(Debug, Clone, Default)]
pub struct Asked {
    /// Who is on the other end.
    pub reader: Reader,
    /// How wide it said it is.
    pub columns: Option<usize>,
    /// What was said about colour.
    pub colour: Wanted,
    /// What `TERM` says.
    pub term: Option<String>,
    /// What the locale says the font has.
    pub glyphs: Glyphs,
}

/// The least room a label needs beside a mark before it reads better under one.
const BESIDE: usize = 24;

/// Where a mark goes on a line, and what is to the left of it.
#[derive(Debug, Clone, Copy)]
struct Marked {
    /// How many columns of the line come before the mark.
    indent: usize,
    /// How many columns the mark covers, which is how wide the code under it is.
    width: usize,
    /// How wide the line numbers to its left are.
    gutter: usize,
}

/// How wide to draw when nothing said.
///
/// Wider than eighty, because eighty is the width of a punched card and a
/// diagnostic that fits one has been trimmed to fit a machine nobody has.
pub const ROOM: usize = 100;

impl Terminal {
    /// What to draw for, given what was found out.
    #[must_use]
    pub fn of(asked: &Asked) -> Self {
        let dumb = asked.term.as_deref() == Some("dumb");
        let a_person = asked.reader == Reader::Person;
        Self {
            width: asked.columns.filter(|it| *it >= 20).unwrap_or(ROOM),
            colour: match asked.colour {
                Wanted::Forced => true,
                Wanted::Refused => false,
                Wanted::Unsaid => a_person && !dumb,
            },
            unicode: asked.glyphs == Glyphs::Drawn && !dumb,
            drawing: a_person || asked.colour == Wanted::Forced,
        }
    }
}
