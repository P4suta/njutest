// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Redirects: a module whose named functions answer through one it exports, and the names of the standard library's that are to.

use rust_mutants_sealed::{Redirect, SealedError, SealedStop, names_std_env, redirected};

use crate::common::{command, invocation, run};

#[test]
fn the_symbol_of_std_and_every_copy_of_it_are_named_and_nothing_else_is() {
    for name in [
        "_RNvNtCsg9AFU4YgWuS_3std3env8temp_dir",
        "_RNvNtCsg9AFU4YgWuS_3std3env8temp_dirCs9Rb0RK0dXmT_9homeprobe",
        "_ZN3std3env8temp_dir17h0123456789abcdefE",
    ] {
        assert!(names_std_env(name, "temp_dir"), "{name}");
    }
    for name in [
        "_RNvNtCsg9AFU4YgWuS_3std3env8home_dir",
        "_RNvNtCsg9AFU4YgWuS_3std3env9temp_dirs",
        "_RNCNvNtCsg9AFU4YgWuS_3std3env8temp_dir0B5_",
        "_RNvNtCsg9AFU4YgWuS_4core3env8temp_dir",
        "_ZN3std3env8temp_dir17h0123E",
        "_ZN3std3env8temp_dir17hxyz3456789abcdefE",
        "temp_dir",
        "",
    ] {
        assert!(!names_std_env(name, "temp_dir"), "{name}");
    }
}

/// A command whose `$_RNvNtCsX_3std3env8temp_dir` writes 1 where it is told and whose export `answer` writes 2, and whose `_start` calls the first and prints what it wrote.
fn answering(answer_type: &str) -> Vec<u8> {
    command(
        &[],
        &format!(
            "(func $_RNvNtCsX_3std3env8temp_dir (param $at i32)
               (i32.store8 (local.get $at) (i32.const 49)))
             (func (export \"answer\") {answer_type}
               (i32.store8 (local.get 0) (i32.const 50)))"
        ),
        "(call $_RNvNtCsX_3std3env8temp_dir (i32.const 500))
         (call $emit (i32.const 500) (i32.const 1))",
    )
}

/// The redirect of the standard library's `temp_dir` to the export `answer`.
const TEMP_DIR: Redirect = Redirect {
    names: |name| names_std_env(name, "temp_dir"),
    export: "answer",
};

#[test]
fn a_named_function_answers_through_the_export_it_is_redirected_to() {
    let bytes = answering("(param i32)");
    assert_eq!(run(&bytes, &invocation()).stdout().bytes(), b"1");
    let rewritten = redirected(&bytes, &[TEMP_DIR]).expect("the module is redirected");
    assert_eq!(rewritten.counts, [1]);
    let transcript = run(&rewritten.bytes, &invocation());
    assert_eq!(transcript.stop(), SealedStop::Returned);
    assert_eq!(
        transcript.stdout().bytes(),
        b"2",
        "the function the name section names is answered by the export it is redirected to"
    );
}

#[test]
fn a_module_that_names_nothing_to_redirect_is_left_as_it_is() {
    let bytes = command(&[], "", "");
    let rewritten = redirected(&bytes, &[TEMP_DIR]).expect("nothing to redirect");
    assert_eq!(rewritten.counts, [0]);
    assert_eq!(rewritten.bytes, bytes);
}

#[test]
fn a_redirect_the_module_cannot_answer_is_refused() {
    let unexported = command(
        &[],
        "(func $_RNvNtCsX_3std3env8temp_dir (param i32))",
        "(call $_RNvNtCsX_3std3env8temp_dir (i32.const 0))",
    );
    match redirected(&unexported, &[TEMP_DIR]) {
        Err(error @ SealedError::RedirectUnexported { export: "answer" }) => {
            assert_eq!(error.code().code(), "RS1007");
        }
        other => panic!("a module without the export cannot be redirected: {other:?}"),
    }
    match redirected(&answering("(param i32 i32)"), &[TEMP_DIR]) {
        Err(SealedError::RedirectMismatched { function, .. }) => {
            assert_eq!(function, "_RNvNtCsX_3std3env8temp_dir");
        }
        other => panic!("an export of another type cannot answer: {other:?}"),
    }
}
