// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Nothing that reads evidence completes it: filling a field a recording left out is the one shape the schema check forbids, so only the sentinels, which lay specimens, may call the filler.

#[test]
fn only_a_sentinel_completes_a_specimen() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut callers = Vec::new();
    for entry in walkdir::WalkDir::new(&root) {
        let entry = entry.expect("the source tree is readable");
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "rs") {
            continue;
        }
        let text = std::fs::read_to_string(path).expect("a source file");
        if text.contains("specimen::completed(") {
            callers.push(
                path.strip_prefix(&root)
                    .expect("under src")
                    .display()
                    .to_string(),
            );
        }
    }
    callers.sort();
    assert_eq!(
        callers,
        ["engineaudit/sentinel.rs", "proofaudit/sentinel.rs"],
        "a reader that completed its input before checking it would pass the schema law on anything"
    );
}
