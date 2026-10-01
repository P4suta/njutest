// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::{HOME_DIR, TEMP_DIR, flags, unanswered};

#[test]
fn a_probe_that_holds_neither_function_does_not_answer_and_says_why() {
    let empty_module = b"\0asm\x01\0\0\0";
    let said = unanswered(empty_module, &crate::trace::Recorder::disabled(), None)
        .expect("an empty module answers nothing");
    assert!(
        said.contains("holds no function of the standard library's"),
        "a toolchain whose probe the rewrite finds nothing in is refused with the count it found: \
         {said}"
    );
}

#[test]
fn every_sealed_module_is_built_unoptimised_with_its_names_the_object_and_both_exports() {
    let built = flags("/sealed/platform/platform.o");
    for flag in [
        "-Copt-level=0",
        "-Cstrip=none",
        "-Clink-arg=--undefined=chdir",
        "-Clink-arg=--export=chdir",
        "-Clink-arg=--export=malloc",
        "-Clink-arg=/sealed/platform/platform.o",
    ] {
        assert!(built.iter().any(|one| one == flag), "{flag} in {built:?}");
    }
    for export in [TEMP_DIR, HOME_DIR] {
        let exported = format!("-Clink-arg=--export={export}");
        assert!(built.contains(&exported), "{exported} in {built:?}");
    }
}
