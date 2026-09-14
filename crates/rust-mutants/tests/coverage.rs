// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Coverage: which regions of which files one test reached, in the units the tools actually use.

#![expect(
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::path::{Path, PathBuf};

use rust_mutants::coverage::{
    Block, CoverageErrorKind, INSTRUMENT_FLAG, PROFILE_ENV, Point, REGION_KIND_CODE, Region,
    parse_export,
};

const EXPORT: &str = r#"{
  "type": "llvm.coverage.json.export",
  "version": "3.1.0",
  "data": [{
    "files": [{"filename": "/w/src/lib.rs"}],
    "functions": [
      {
        "name": "_RNvCs_1w3max",
        "count": 1,
        "filenames": ["/w/src/lib.rs"],
        "regions": [
          [2, 1, 2, 34, 1, 0, 0, 0],
          [3, 8, 3, 13, 1, 0, 0, 0],
          [3, 16, 3, 17, 0, 0, 0, 0],
          [9, 1, 9, 2, 4, 0, 0, 2]
        ]
      },
      {
        "name": "_RNvCs_1w7unused",
        "count": 0,
        "filenames": ["/w/src/other.rs"],
        "regions": [[7, 5, 7, 10, 0, 0, 0, 0]]
      }
    ],
    "totals": {}
  }]
}"#;

#[test]
fn an_export_is_read_into_regions_per_file() {
    let files = parse_export(EXPORT.as_bytes()).expect("an export");
    let paths: Vec<&Path> = files.iter().map(|file| file.path.as_path()).collect();
    assert_eq!(
        paths,
        [Path::new("/w/src/lib.rs"), Path::new("/w/src/other.rs")],
        "one entry per file that has regions, in path order"
    );
}

#[test]
fn every_region_keeps_its_place_its_count_and_its_kind() {
    let files = parse_export(EXPORT.as_bytes()).expect("an export");
    let lib = files
        .iter()
        .find(|file| file.path == Path::new("/w/src/lib.rs"))
        .expect("the library");
    assert_eq!(
        lib.regions,
        [
            Region {
                start: Point { line: 2, column: 1 },
                end: Point {
                    line: 2,
                    column: 34
                },
                count: 1,
                kind: REGION_KIND_CODE,
            },
            Region {
                start: Point { line: 3, column: 8 },
                end: Point {
                    line: 3,
                    column: 13
                },
                count: 1,
                kind: REGION_KIND_CODE,
            },
            Region {
                start: Point {
                    line: 3,
                    column: 16
                },
                end: Point {
                    line: 3,
                    column: 17
                },
                count: 0,
                kind: REGION_KIND_CODE,
            },
            Region {
                start: Point { line: 9, column: 1 },
                end: Point { line: 9, column: 2 },
                count: 4,
                kind: 2,
            },
        ]
    );
    let other = files
        .iter()
        .find(|file| file.path == Path::new("/w/src/other.rs"))
        .expect("the other file");
    assert_eq!(other.regions.len(), 1);
    assert_eq!(other.regions[0].count, 0);
}

#[test]
fn only_a_code_region_that_ran_is_a_covered_block() {
    let files = parse_export(EXPORT.as_bytes()).expect("an export");
    let blocks = rust_mutants::coverage::covered(&files);
    assert_eq!(
        blocks,
        [
            Block {
                file: PathBuf::from("/w/src/lib.rs"),
                start: Point { line: 2, column: 1 },
                end: Point {
                    line: 2,
                    column: 34
                },
            },
            Block {
                file: PathBuf::from("/w/src/lib.rs"),
                start: Point { line: 3, column: 8 },
                end: Point {
                    line: 3,
                    column: 13
                },
            },
        ]
        .into_iter()
        .collect(),
        "the uncovered region, the region of another kind, and the other file's zero are all out"
    );
}

#[test]
fn a_block_holds_a_position_by_its_line_and_byte_column() {
    let block = Block {
        file: PathBuf::from("/w/src/lib.rs"),
        start: Point { line: 3, column: 8 },
        end: Point { line: 4, column: 5 },
    };
    let inside = [(3, 8), (3, 99), (4, 1), (4, 4)];
    for (line, column) in inside {
        assert!(
            block.contains(Path::new("/w/src/lib.rs"), Point { line, column }),
            "{line}:{column} is inside"
        );
    }
    for (line, column) in [(3, 7), (4, 5), (5, 1), (2, 100)] {
        assert!(
            !block.contains(Path::new("/w/src/lib.rs"), Point { line, column }),
            "{line}:{column} is outside; the end is exclusive"
        );
    }
    assert!(
        !block.contains(Path::new("/w/src/other.rs"), Point { line: 3, column: 9 }),
        "another file is another place"
    );
}

#[test]
fn an_export_that_is_not_one_is_refused_rather_than_read_in_part() {
    for bad in [
        "",
        "not json",
        r#"{"data": []}"#,
        r#"{"type":"llvm.coverage.json.export","version":"3.1.0","data":[{"functions":[{"name":"f","count":0,"filenames":[],"regions":[[1,1,1,2,0,0,0,0]]}]}]}"#,
        r#"{"type":"llvm.coverage.json.export","version":"3.1.0","data":[{"functions":[{"name":"f","count":0,"filenames":["/w/a.rs"],"regions":[[1,1,1]]}]}]}"#,
    ] {
        let error = parse_export(bad.as_bytes()).expect_err("refused");
        assert_eq!(error.kind(), CoverageErrorKind::Unreadable, "{bad}");
        assert!(error.to_string().contains("RM6001"), "{error}");
    }
}

#[test]
fn the_flags_the_tools_are_driven_with_are_frozen() {
    assert_eq!(INSTRUMENT_FLAG, "-C instrument-coverage");
    assert_eq!(PROFILE_ENV, "LLVM_PROFILE_FILE");
}

#[test]
fn a_region_that_ends_before_it_starts_is_refused() {
    let export = |start: (u32, u32), end: (u32, u32)| {
        format!(
            r#"{{"type":"llvm.coverage.json.export","version":"2.0.1","data":[{{"functions":[
                {{"filenames":["src/lib.rs"],"regions":[[{},{},{},{},1,0,0,0]]}}]}}]}}"#,
            start.0, start.1, end.0, end.1
        )
    };
    let forward = parse_export(export((3, 1), (5, 2)).as_bytes());
    assert!(forward.is_ok(), "{forward:?}");

    for (start, end, why) in [
        (
            (5, 1),
            (3, 2),
            "a region that ends on an earlier line than it starts on",
        ),
        (
            (5, 9),
            (5, 2),
            "a region that ends before it starts on the same line",
        ),
    ] {
        let error = parse_export(export(start, end).as_bytes()).expect_err(why);
        assert_eq!(
            error.kind(),
            CoverageErrorKind::Unreadable,
            "{why}: a measurement nobody can read is one a run must not route by: {error}"
        );
        assert!(
            error.to_string().contains("ends"),
            "{why}: the refusal says what is wrong with it: {error}"
        );
    }
}

#[test]
fn a_region_that_starts_and_ends_at_the_same_place_is_read() {
    let export = br#"{"type":"llvm.coverage.json.export","version":"2.0.1","data":[{"functions":[
        {"filenames":["src/lib.rs"],"regions":[[4,7,4,7,1,0,0,0]]}]}]}"#;
    let read = parse_export(export).expect("an empty region is a region");
    assert_eq!(read.len(), 1, "{read:?}");
}
