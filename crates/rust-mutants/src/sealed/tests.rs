// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

use super::bench::Tree;
use super::{TARGET, installed};

#[test]
fn a_module_cache_is_retained_outside_its_disposable_snapshot() {
    let retained = njutest_devkit::temporary::CacheDirectory::make("sealed-product-root-")
        .expect("the parent-owned user-cache control");
    let vars = crate::vars::Variables::of([(
        std::ffi::OsString::from("XDG_CACHE_HOME"),
        retained.path().as_os_str().to_owned(),
    )]);
    let snapshot = std::path::Path::new("disposable-snapshot/target");
    let cache = super::module_cache(snapshot, Some(&vars)).expect("an actual retained cache");
    assert_eq!(
        cache.directory(),
        retained
            .path()
            .join("wasmtime-modules-v1")
            .canonicalize()
            .expect("the actual retained cache identity")
    );
    assert!(!cache.directory().starts_with(snapshot));
}

#[test]
fn an_absent_or_relative_retained_root_refuses_without_creating_a_snapshot_cache() {
    let snapshot = std::path::Path::new("disposable-snapshot/target");
    let relative = crate::vars::Variables::of([(
        std::ffi::OsString::from("NJUTEST_FIXTURE_BUILD_CACHE"),
        std::ffi::OsString::from("relative"),
    )]);
    for vars in [
        None,
        Some(&crate::vars::Variables::empty()),
        Some(&relative),
    ] {
        assert!(matches!(
            super::module_cache(snapshot, vars),
            Err(rust_mutants_sealed::SealedError::Preparation { .. })
        ));
    }
}

#[test]
fn a_tree_is_spelled_as_its_build_baked_it_in_and_places_a_directory_by_its_names() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().join("tree");
    let tree = Tree::read(&root, std::iter::empty()).expect("an empty tree");
    assert_eq!(
        Some(tree.root.as_str()),
        root.to_str(),
        "the tree is preopened at the root cargo bakes into `CARGO_MANIFEST_DIR`"
    );
    assert_eq!(tree.within(&root), Some(String::new()));
    assert_eq!(
        tree.within(&root.join("member").join("inner")),
        Some("member/inner".to_owned())
    );
    assert_eq!(tree.within(dir.path()), None);
    assert_eq!(tree.within(&dir.path().join("elsewhere")), None);
}

#[test]
fn a_sysroot_without_the_targets_library_directory_does_not_hold_it() {
    let sysroot = tempfile::tempdir().expect("tempdir");
    assert!(!installed(sysroot.path()).expect("an absent directory is an answer"));
}

#[test]
fn a_library_directory_without_a_standard_library_does_not_hold_it() {
    let sysroot = tempfile::tempdir().expect("tempdir");
    let libdir = sysroot.path().join("lib/rustlib").join(TARGET).join("lib");
    std::fs::create_dir_all(&libdir).expect("mkdir");
    std::fs::write(libdir.join("libcore-0.rlib"), b"").expect("a library");
    assert!(!installed(sysroot.path()).expect("listed"));
    std::fs::write(libdir.join("libstd-0.rlib"), b"").expect("the standard library");
    assert!(installed(sysroot.path()).expect("listed"));
}

/// What `rustc --print cfg --target wasm32-wasip1` says of the names a target alone decides.
fn wasip1() -> crate::facts::Facts {
    crate::facts::Facts::printed(
        "panic=\"abort\"\ntarget_arch=\"wasm32\"\ntarget_endian=\"little\"\ntarget_env=\"p1\"\n\
         target_family=\"wasm\"\ntarget_os=\"wasi\"\ntarget_pointer_width=\"32\"\n\
         target_vendor=\"unknown\"\n",
    )
}

/// One configuration file, the environment the build inherits, and the flags it is compiled and documented with before the sealed build's own.
type Chosen<'a> = (
    &'a str,
    &'a [(&'a str, &'a str)],
    &'a [&'a str],
    &'a [&'a str],
);

#[test]
fn the_sealed_build_keeps_the_flags_cargo_would_choose_and_links_its_own_after_them() {
    let linked = super::start_linked();
    let cases: [Chosen<'_>; 6] = [
        ("", &[], &[], &[]),
        (
            "[build]\nrustflags = [\"--cfg\", \"built\"]\nrustdocflags = \"--cfg documented\"\n",
            &[],
            &["--cfg", "built"],
            &["--cfg", "documented"],
        ),
        (
            "[build]\nrustflags = [\"--cfg\", \"built\"]\n\
             [target.wasm32-wasip1]\nrustflags = [\"--cfg\", \"for-wasi\"]\n\
             [target.aarch64-apple-darwin]\nrustflags = [\"--cfg\", \"for-a-mac\"]\n",
            &[],
            &["--cfg", "for-wasi"],
            &[],
        ),
        (
            "[target.'cfg(target_family = \"wasm\")']\nrustflags = [\"--cfg\", \"any-wasm\"]\n\
             [target.'cfg(unix)']\nrustflags = [\"--cfg\", \"unix\"]\n",
            &[],
            &["--cfg", "any-wasm"],
            &[],
        ),
        (
            "[build]\nrustflags = [\"--cfg\", \"built\"]\n",
            &[("RUSTFLAGS", "--cfg  plain"), ("RUSTDOCFLAGS", "--cfg doc")],
            &["--cfg", "plain"],
            &["--cfg", "doc"],
        ),
        (
            "",
            &[
                ("CARGO_ENCODED_RUSTFLAGS", "--cfg\u{1f}encoded"),
                ("RUSTFLAGS", "--cfg ignored"),
            ],
            &["--cfg", "encoded"],
            &[],
        ),
    ];
    for (text, env, compiled, documented) in cases {
        let mut given = crate::vars::Variables::empty();
        for (name, value) in env {
            given.set(*name, *value);
        }
        let flags = match super::Flags::of(
            &given,
            &crate::cargo::config::layer(text),
            &wasip1(),
            &linked,
        ) {
            Ok(flags) => flags,
            Err(why) => panic!("{text:?} {env:?}: {}", why.name()),
        };
        let with_linked = |before: &[&str]| -> Vec<String> {
            before
                .iter()
                .map(|flag| (*flag).to_owned())
                .chain(linked.iter().cloned())
                .collect()
        };
        assert_eq!(flags.compile, with_linked(compiled), "{text:?} {env:?}");
        assert_eq!(flags.document, with_linked(documented), "{text:?} {env:?}");
    }
}

#[test]
fn flags_whose_choice_is_not_known_seal_nothing_and_say_why() {
    let linked = super::start_linked();
    for text in [
        "[target.'cfg(debug_assertions)']\nrustflags = [\"--cfg\", \"undecided\"]\n",
        "[target.'cfg(not a predicate']\nrustflags = [\"-O\"]\n",
        "[build]\nrustflags = 7\n",
        "not toml at all [",
    ] {
        assert_eq!(
            super::Flags::of(
                &crate::vars::Variables::empty(),
                &crate::cargo::config::layer(text),
                &wasip1(),
                &linked
            ),
            Err(super::Unsealed::FlagsUnmerged),
            "{text:?}"
        );
    }
}
