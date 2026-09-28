// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The laws that make a transcript a function of its inputs, over inputs a generator picks.

use proptest::prelude::{ProptestConfig, any, prop, prop_assert_eq, prop_assert_ne, proptest};
use rust_mutants_sealed::{Arguments, SealedStop};

use crate::common::{command, invocation, runner};

/// A command that reads everything an invocation varies and writes a file it insists is new.
fn reader() -> Vec<u8> {
    command(
        &[
            "args_sizes_get",
            "args_get",
            "random_get",
            "clock_time_get",
            "path_open",
        ],
        "(data (i32.const 500) \"made.txt\")
         (data (i32.const 520) \"abc\")",
        "(call $expect (call $args_sizes_get (i32.const 64) (i32.const 68)) (i32.const 0))
         (call $emit (i32.const 64) (i32.const 8))
         (call $expect (call $args_get (i32.const 1000) (i32.const 2000)) (i32.const 0))
         (call $emit (i32.const 2000) (i32.load (i32.const 68)))
         (call $expect (call $random_get (i32.const 300) (i32.const 16)) (i32.const 0))
         (call $emit (i32.const 300) (i32.const 16))
         (call $expect (call $clock_time_get (i32.const 1) (i64.const 1) (i32.const 400)) (i32.const 0))
         (call $emit (i32.const 400) (i32.const 8))
         (call $expect (call $path_open (i32.const 3) (i32.const 0) (i32.const 500) (i32.const 8) (i32.const 5) (i64.const -1) (i64.const -1) (i32.const 0) (i32.const 72)) (i32.const 0))
         (i32.store (i32.const 80) (i32.const 520))
         (i32.store (i32.const 84) (i32.const 3))
         (call $expect (call $fd_write (i32.load (i32.const 72)) (i32.const 80) (i32.const 1) (i32.const 88)) (i32.const 0))",
    )
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_same_invocation_twice_is_the_same_transcript_byte_for_byte(
        seed in any::<u64>(),
        fuel in 100_u64..20_000,
        words in prop::collection::vec(any::<u16>(), 0..4),
    ) {
        let runner = runner();
        let module = runner.prepare(&reader()).expect("the command is valid");
        let mut asked = invocation();
        asked.seed = seed;
        asked.fuel = fuel;
        let mut arguments = vec!["command".to_owned()];
        arguments.extend(words.iter().map(|word| format!("w{word}")));
        asked.arguments = Arguments::new(arguments).expect("valid arguments");
        let first = module.invoke(&asked).expect("an answer about the guest");
        let second = module.invoke(&asked).expect("an answer about the guest");
        prop_assert_eq!(first.digest(), second.digest());
        prop_assert_eq!(&first, &second);
    }

    #[test]
    fn a_file_made_exclusively_is_new_to_every_invocation(seed in any::<u64>()) {
        let runner = runner();
        let module = runner.prepare(&reader()).expect("the command is valid");
        let mut asked = invocation();
        asked.seed = seed;
        for _ in 0..3 {
            let transcript = module.invoke(&asked).expect("an answer about the guest");
            prop_assert_eq!(transcript.stop(), SealedStop::Returned);
            prop_assert_eq!(transcript.overlay().len(), 1);
        }
    }

    #[test]
    fn another_seed_is_another_stream(seed in any::<u64>()) {
        let runner = runner();
        let module = runner.prepare(&reader()).expect("the command is valid");
        let mut one = invocation();
        one.seed = seed;
        let mut other = invocation();
        other.seed = seed ^ 1;
        let first = module.invoke(&one).expect("an answer about the guest");
        let second = module.invoke(&other).expect("an answer about the guest");
        prop_assert_ne!(first.stdout(), second.stdout());
        prop_assert_ne!(first.invocation(), second.invocation());
    }
}
