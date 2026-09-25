// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The commands a documented workflow runs, split the way a shell splits them.

use njutest_devkit::workflow_commands::commands;

fn argv(page: &str) -> Vec<Vec<String>> {
    commands(page, "tool")
        .into_iter()
        .map(|command| command.argv)
        .collect()
}

#[test]
fn a_command_is_found_on_a_step_line_in_a_block_and_across_continued_lines() {
    let page = "```yaml\n- run: tool one --a\n- name: two\n  run: |\n    other thing\n    tool two \\\n      --b \"${{ x }}-y\" > out.txt || true\n```\n";
    assert_eq!(
        argv(page),
        vec![
            vec!["tool".to_owned(), "one".to_owned(), "--a".to_owned()],
            vec![
                "tool".to_owned(),
                "two".to_owned(),
                "--b".to_owned(),
                "X-y".to_owned()
            ],
        ],
        "a step line, a block, a continued line, an expression, a quote, a redirection and a list are each read the way the shell reads them"
    );
}

#[test]
fn a_command_outside_a_yaml_fence_is_prose_and_not_a_workflow() {
    assert!(argv("run: tool one\n```sh\n- run: tool two\n```\n").is_empty());
}
