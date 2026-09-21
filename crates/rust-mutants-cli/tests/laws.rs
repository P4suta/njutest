// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What holds for every run of the engine's command line rather than for the ones somebody thought to write down.

#![expect(
    clippy::panic,
    reason = "a law that cannot reach its subject has nothing to state, and saying so by \
              panicking is how a property test reports it"
)]

use proptest::prelude::*;
use rust_mutants_cli::app::doctor::rendered_bytes;
use rust_mutants_cli::app::estimate::Estimated;
use rust_mutants_cli::app::stored;
use rust_mutants_cli::cli;

/// What `--run-id` was given, as the command line hands it over.
fn run_named(id: &str) -> cli::Command {
    let mut parsed = cli::parse(
        ["rust-mutants", "run"]
            .into_iter()
            .map(std::ffi::OsString::from),
    )
    .unwrap_or_else(|usage| panic!("`run` with nothing else on it parses: {usage:?}"));
    match &mut parsed.command {
        cli::Command::Run { run_id, .. } => *run_id = Some(id.to_owned()),
        _ => panic!("the parser returned a non-run command for the literal `run`"),
    }
    parsed.command
}

/// The names a person may pass to `--run-id`: anything, and the few a filesystem answers to already.
fn named() -> impl Strategy<Value = String> {
    prop_oneof![
        4 => "\\PC{0,80}",
        1 => prop::sample::select(vec![
            ".".to_owned(),
            "..".to_owned(),
            "...".to_owned(),
            "a/b".to_owned(),
            "/absolute".to_owned(),
            "..\\\\b".to_owned(),
            String::new(),
        ]),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    /// A name a run may go by is a name the report directory holds, and nothing above it.
    #[test]
    fn a_name_a_run_goes_by_stores_its_report_under_the_report_directory(
        name in named()
    ) {
        let Ok(accepted) = stored::named(&run_named(&name), jiff::Timestamp::UNIX_EPOCH) else {
            return Ok(());
        };
        let parts: Vec<std::path::Component<'_>> =
            std::path::Path::new(accepted.as_str()).components().collect();
        prop_assert_eq!(
            parts.len(),
            1,
            "a run's name is one name: {:?} is {} of them, and the report goes somewhere \
             the report directory does not hold",
            accepted,
            parts.len()
        );
        prop_assert!(
            parts
                .iter()
                .all(|part| matches!(*part, std::path::Component::Normal(_))),
            "and a name rather than a way of getting somewhere: a run named {:?} writes \
             its report over the directory that holds every other run, and the next \
             person asking about a run reads it back as theirs",
            accepted
        );
    }

    /// Two runs a moment apart read back in the order they ran.
    #[test]
    fn the_name_a_run_goes_by_orders_the_runs_the_way_they_ran(
        first in 0i64..4_000_000_000,
        gap in 1i64..1_000_000_000,
    ) {
        let earlier = jiff::Timestamp::from_second(first)
            .unwrap_or_else(|error| panic!("the generated earlier timestamp is valid: {error}"));
        let later_seconds = first
            .checked_add(gap)
            .unwrap_or_else(|| panic!("the bounded generated seconds fit i64"));
        let later = jiff::Timestamp::from_second(later_seconds)
            .unwrap_or_else(|error| panic!("the generated later timestamp is valid: {error}"));
        prop_assume!(earlier < later);
        let earlier_id = stored::run_id(earlier).expect("canonical timestamp");
        let later_id = stored::run_id(later).expect("canonical timestamp");
        prop_assert!(
            earlier_id < later_id,
            "a directory listing is how the runs are read back, so the name has to sort \
             the way they ran: {} then {}",
            earlier_id,
            later_id
        );
    }

    /// The room a sweep says it gave back is never more than it gave back.
    #[test]
    fn bytes_as_a_person_reads_them_never_say_there_is_more_than_there_is(
        bytes in 0u64..u64::MAX,
    ) {
        let said = rendered_bytes(bytes)
            .unwrap_or_else(|error| panic!("every u64 byte count is renderable: {error}"));
        let (value, unit) = said
            .split_once(' ')
            .unwrap_or_else(|| panic!("a rendering is a number and a unit: {said}"));
        let power = ["B", "KiB", "MiB", "GiB", "TiB"]
            .iter()
            .position(|named| *named == unit)
            .unwrap_or_else(|| panic!("a unit a reader knows: {said}"));
        let exponent = u32::try_from(power)
            .unwrap_or_else(|error| panic!("the five-unit table fits u32: {error}"));
        let scale = 1024u128.pow(exponent);
        let (whole, tenths) = value.split_once('.').unwrap_or((value, "0"));
        let whole: u128 = whole
            .parse::<u128>()
            .unwrap_or_else(|error| panic!("the rendered whole number is numeric: {error}"));
        let tenths: u128 = tenths
            .parse::<u128>()
            .unwrap_or_else(|error| panic!("the rendered tenths are numeric: {error}"));

        let floor = whole
            .checked_mul(10)
            .and_then(|scaled| scaled.checked_add(tenths))
            .and_then(|scaled| scaled.checked_mul(scale))
            .unwrap_or_else(|| panic!("the rendered bounded byte count fits u128"))
            / 10;
        prop_assert!(
            floor <= u128::from(bytes),
            "{bytes} bytes read as {said}, which is more room than there is: a person \
             reading it goes away satisfied and the disk is still full"
        );
        prop_assert!(
            whole < 1024 || power == 4,
            "and a rendering carries its value in the largest unit that fits, or nobody \
             reads it as a size at all: {said}"
        );
    }

    /// Every share a dry run prints is a share.
    #[test]
    fn every_share_a_dry_run_prints_is_between_none_of_it_and_all_of_it(
        cataloged in 0u64..10_000,
        selected_seed in 0u64..10_000,
        pair_seed in 0u64..100_000,
        test_seed in 0u64..100_000,
        tests_whole in 0u64..100_000,
        targets in 0u64..64,
    ) {
        let selected_bound = cataloged.checked_add(1).expect("the bounded catalog count");
        let selected = selected_seed
            .checked_rem(selected_bound)
            .expect("the catalog bound is nonzero");
        let whole_pairs = cataloged
            .checked_mul(targets)
            .expect("the bounded pair count");
        let pair_bound = whole_pairs.checked_add(1).expect("the bounded pair count");
        let pairs = pair_seed
            .checked_rem(pair_bound)
            .expect("the pair bound is nonzero");
        let test_bound = tests_whole.checked_add(1).expect("the bounded test count");
        let tests = test_seed
            .checked_rem(test_bound)
            .expect("the test bound is nonzero");
        let counted = Estimated {
            cataloged: cataloged.into(),
            selected: selected.into(),
            pairs: pairs.into(),
            tests: tests.into(),
            tests_whole: tests_whole.into(),
            ..Estimated::default()
        };
        let said = counted
            .said(targets.into())
            .unwrap_or_else(|error| panic!("the bounded generated estimate is exact: {error}"));
        for share in said
            .split_whitespace()
            .filter_map(|word| word.strip_suffix("%"))
            .map(|number| number.parse::<f64>().expect("a rendered percentage"))
        {
            prop_assert!(
                (0.0..=100.0).contains(&share),
                "a share outside the whole is a number a person cannot act on, and it \
                 reads as the proof layers working: {said}"
            );
        }
    }

    /// A line a run is narrowed to is a line, and a range is one way round.
    #[test]
    fn a_file_a_run_is_narrowed_to_addresses_a_line_that_exists(
        path in "[a-z/]{1,20}\\.rs",
        from in 0u32..5_000,
        to in 0u32..5_000,
    ) {
        let Ok((named, lines)) = rust_mutants_cli::app::addressed(&format!("{path}:{from}-{to}"))
        else {
            return Ok(());
        };
        prop_assert_eq!(&named, &path, "the path it addresses is the path it was given");
        let (first, last) = lines.unwrap_or_else(|| panic!("a range was given: {path}"));
        prop_assert!(
            first >= 1 && first <= last,
            "a file has no line zero and no range that ends before it starts, and taking \
             one would measure a selection nobody asked for and report it as the whole: \
             {first}-{last}"
        );
    }

    /// A path with no lines after it is the whole file, and the path is unchanged.
    #[test]
    fn a_path_with_no_line_after_it_addresses_the_file_it_names(
        path in "[a-z/]{1,20}\\.rs",
    ) {
        let (named, lines) = rust_mutants_cli::app::addressed(&path)
            .unwrap_or_else(|_error| panic!("a path is a path: {path}"));
        prop_assert_eq!(named, path);
        prop_assert_eq!(lines, None, "and it narrows to no line of it");
    }
}
