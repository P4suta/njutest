// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands a page's workflow examples run, split the way a shell splits them, so a test can hand each to the parser it names.

/// One command a workflow example runs: where it is, and its arguments with the program first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The line of the page the command starts on, counted from one.
    pub line: usize,
    /// The program and its arguments, with every `${{ … }}` expression replaced by `X`.
    pub argv: Vec<String>,
}

/// Every command of `program` that a `run:` step inside a `yaml` fence of `page` runs.
#[must_use]
pub fn commands(page: &str, program: &str) -> Vec<Command> {
    let lines: Vec<(usize, &str)> = (1_usize..).zip(page.lines()).collect();
    let mut found = Vec::new();
    let mut fenced = false;
    let mut resume = 0;
    for (index, &(_, line)) in lines.iter().enumerate() {
        if index < resume {
            continue;
        }
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            fenced = !fenced && trimmed.trim_end() == "```yaml";
            continue;
        }
        let Some(said) = run_value(trimmed).filter(|_| fenced) else {
            continue;
        };
        let script = if said.trim() == "|" {
            let body = block(&lines, index);
            resume = lines
                .iter()
                .position(|&(number, _)| body.last().is_some_and(|&(last, _)| number == last))
                .map_or(resume, |at| at.max(index));
            body
        } else {
            lines
                .get(index)
                .map(|&(number, _)| vec![(number, said.to_owned())])
                .unwrap_or_default()
        };
        found.extend(script_commands(&script, program));
    }
    found
}

/// What follows `run:` on a step line, whether the line opens the step or continues it.
fn run_value(trimmed: &str) -> Option<&str> {
    trimmed
        .strip_prefix("- run:")
        .or_else(|| trimmed.strip_prefix("run:"))
}

/// The numbered lines of the `run: |` block opened at `index`.
fn block(lines: &[(usize, &str)], index: usize) -> Vec<(usize, String)> {
    let opener = lines.get(index).map_or(0, |&(_, line)| indent(line));
    lines
        .iter()
        .skip(index)
        .skip(1)
        .take_while(|&&(_, line)| line.trim().is_empty() || indent(line) > opener)
        .map(|&(number, line)| (number, line.trim().to_owned()))
        .collect()
}

fn indent(line: &str) -> usize {
    line.chars()
        .take_while(|character| character.is_whitespace())
        .count()
}

/// The commands of `program` in a script, with continued lines joined and each command of a pipeline or list taken on its own.
fn script_commands(script: &[(usize, String)], program: &str) -> Vec<Command> {
    let mut found = Vec::new();
    let mut joined = String::new();
    let mut started = None;
    for (number, text) in script {
        let text = text.split(" #").next().unwrap_or_default();
        let line = *started.get_or_insert(*number);
        if let Some(continued) = text.strip_suffix('\\') {
            joined.push_str(continued);
            joined.push(' ');
            continue;
        }
        joined.push_str(text);
        started = None;
        for segment in segments(&joined) {
            let argv = words(&segment);
            if argv.first().is_some_and(|first| first == program) {
                found.push(Command { line, argv });
            }
        }
        joined.clear();
    }
    found
}

/// The commands of one shell line, split at `&&`, `||`, `;` and `|`.
fn segments(line: &str) -> Vec<String> {
    let mut parts = vec![line.to_owned()];
    for separator in ["&&", "||", ";", "|"] {
        parts = parts
            .iter()
            .flat_map(|part| part.split(separator).map(str::to_owned).collect::<Vec<_>>())
            .collect();
    }
    parts
}

/// `command` with every `${{ … }}` expression replaced by `X`.
fn expressions_replaced(command: &str) -> String {
    let mut plain = String::new();
    let mut rest = command;
    while let Some((before, after)) = rest.split_once("${{") {
        plain.push_str(before);
        plain.push('X');
        rest = after.split_once("}}").map_or("", |(_, tail)| tail);
    }
    plain.push_str(rest);
    plain
}

/// A command split into words the way a shell would, stopping at the first redirection.
fn words(command: &str) -> Vec<String> {
    let mut said = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for character in expressions_replaced(command).chars() {
        match (quote, character) {
            (Some(open), close) if open == close => quote = None,
            (Some(_), inside) => word.push(inside),
            (None, '"' | '\'') => quote = Some(character),
            (None, space) if space.is_whitespace() => {
                if !word.is_empty() {
                    said.push(std::mem::take(&mut word));
                }
            }
            (None, other) => word.push(other),
        }
    }
    if !word.is_empty() {
        said.push(word);
    }
    said.into_iter()
        .take_while(|word| {
            !word.starts_with('>') && !word.starts_with("2>") && !word.starts_with('<')
        })
        .collect()
}
