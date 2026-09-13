// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A library that reads a file its own build script wrote, and a value that build script put in the environment.

include!(concat!(env!("OUT_DIR"), "/table.rs"));

/// The generated total, and one more.
#[must_use]
pub fn one_more_than_total() -> i32 {
    GENERATED_TOTAL + 1
}

/// What the build script said about itself.
#[must_use]
pub const fn tag() -> &'static str {
    env!("FIXTURE_BUILD_TAG")
}

#[cfg(test)]
mod tests {
    use super::{one_more_than_total, tag};

    #[test]
    fn the_generated_total_is_six_and_one_more_is_seven() {
        assert_eq!(one_more_than_total(), 7);
    }

    #[test]
    fn the_build_script_put_its_tag_in_the_environment() {
        assert_eq!(tag(), "written-by-the-build-script");
    }

    #[test]
    fn a_test_process_is_told_where_the_build_directory_is() {
        let out = std::env::var("OUT_DIR").expect("a test process learns its OUT_DIR");
        assert!(
            std::path::Path::new(&out).join("table.rs").is_file(),
            "and the directory holds what the build script wrote: {out}"
        );
    }
}
