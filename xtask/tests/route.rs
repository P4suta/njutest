// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one reader both audits ask about how a run routed, over the two recordings that write it down.

use njutest_devkit::result::{OptionState, ResultState, option_state, result_state};
use xtask::route::{self, Discharge, Routing};
use xtask::schemas::Producer;

/// A recording as the engine writes it: the mutant by its full identity, a dense index, and the targets that ran.
const ENGINE: &str = r#"
{"seq":1,"timestamp":"2026-09-06T00:00:00Z","elapsed_ms":0,"payload":{"type":"run-start","schema":"rust-mutants-trace-v1","engine":"0.1.0","context":{"kind":"standalone","run_id":"route-specimen","build_selection":"c5a587d94348b75388f86ec2495002bcecf82b4abb21333627414c945c0746ed"}}}
{"seq":2,"timestamp":"2026-09-06T00:00:01Z","elapsed_ms":1,"payload":{"type":"route","route":{"mutant":"aaaaaaaaaaaaaaaaaaaa","index":3,"granularity":"block","fallback":null,"reaching":["pkg/lib/pkg"],"discharged":[],"considered":[],"executed":["pkg/lib/pkg"],"reused":null}}}
{"seq":3,"timestamp":"2026-09-06T00:00:02Z","elapsed_ms":2,"payload":{"type":"mutant-exec","mutant":{"id":"aaaaaaaaaaaaaaaaaaaa","index":3,"target":"pkg/lib/pkg","outcome":"killed","step_notice":null,"exit_code":101,"duration_ms":7,"tests_run":2,"signal":null,"failed_tests":[],"timeout_ms":1000,"timeout_source":"configured","alone":false}}}
{"seq":4,"timestamp":"2026-09-06T00:00:03Z","elapsed_ms":3,"payload":{"type":"route","route":{"mutant":"bbbbbbbbbbbbbbbbbbbb","index":4,"granularity":"unreached","fallback":null,"reaching":[],"discharged":[],"considered":[],"executed":[],"reused":null}}}
"#;

/// A recording as the runner writes it: the mutant by its short name, no index, and no list of what ran.
const RUNNER: &str = r#"
{"seq":1,"timestamp":"2026-09-06T00:00:00Z","elapsed_ms":0,"payload":{"type":"route","route":{"mutant":"aaaaaaaaaaaaaaaaaaaa","granularity":"block","fallback":"touch-incomplete","reaching":["pkg/lib/pkg"],"discharged":[{"target":"pkg/test/ui","proof":"branch-never-taken"}],"considered":["pkg/test/wide"],"reused":null,"tests":[],"refused":null}}}
{"seq":2,"timestamp":"2026-09-06T00:00:01Z","elapsed_ms":1,"payload":{"type":"mutant-exec","mutant":{"mutant":"aaaaaaaaaaaaaaaaaaaa","target":"pkg/lib/pkg","args":[],"outcome":"survived","step_boundary":null,"duration_ms":9,"alone":true}}}
"#;

fn routing(recording: &str, producer: Producer) -> Option<Routing> {
    let result = route::read(recording, producer);
    assert_eq!(
        result_state(&result),
        ResultState::Returned,
        "a fixture recording must be valid: {result:?}"
    );
    match result {
        Ok(routing) => Some(routing),
        Err(_) => None,
    }
}

#[test]
fn the_route_reader_accepts_both_producer_vocabularies() {
    let Some(engine) = routing(ENGINE, Producer::Engine) else {
        return;
    };
    let Some(runner) = routing(RUNNER, Producer::Runner) else {
        return;
    };
    for (name, routing) in [("engine", &engine), ("runner", &runner)] {
        let found = routing.route("aaaaaaaaaaaaaaaaaaaa");
        assert_eq!(
            option_state(found),
            OptionState::Present,
            "{name} names the mutant it routed"
        );
        let Some(found) = found else { continue };
        assert_eq!(found.reaching, vec!["pkg/lib/pkg".to_owned()], "{name}");
        let ran: Vec<&str> = routing
            .execs_of("aaaaaaaaaaaaaaaaaaaa")
            .map(|exec| exec.target.as_str())
            .collect();
        assert_eq!(ran, vec!["pkg/lib/pkg"], "{name} names what ran");
    }
    let engine_route = engine.routes.first();
    assert_eq!(
        option_state(engine_route),
        OptionState::Present,
        "the engine recorded no route: {:?}",
        engine.routes
    );
    if let Some(engine_route) = engine_route {
        assert!(
            route::GRANULARITIES.contains(&engine_route.granularity.as_str()),
            "{:?}",
            engine.routes
        );
    }
    let runner_route = runner.routes.first();
    assert_eq!(
        option_state(runner_route),
        OptionState::Present,
        "the runner recorded no route: {:?}",
        runner.routes
    );
    if let Some(runner_route) = runner_route {
        assert!(
            route::GRANULARITIES.contains(&runner_route.granularity.as_str()),
            "{:?}",
            runner.routes
        );
    }
    assert_eq!(
        route::ENGINE_GRANULARITIES,
        route::RUNNER_GRANULARITIES,
        "one rule decides a route now, so both recordings say it in one vocabulary"
    );
}

#[test]
fn each_producer_keeps_the_fields_only_it_records() {
    let Some(engine) = routing(ENGINE, Producer::Engine) else {
        return;
    };
    let Some(runner) = routing(RUNNER, Producer::Runner) else {
        return;
    };
    let engine_route = engine.route("aaaaaaaaaaaaaaaaaaaa");
    assert_eq!(option_state(engine_route), OptionState::Present);
    let Some(engine_route) = engine_route else {
        return;
    };
    assert_eq!(engine_route.index, Some(3));
    assert_eq!(engine_route.executed, vec!["pkg/lib/pkg".to_owned()]);
    assert!(engine_route.considered.is_empty());
    let engine_exec = engine.execs.first();
    assert_eq!(option_state(engine_exec), OptionState::Present);
    if let Some(engine_exec) = engine_exec {
        assert_eq!(engine_exec.tests_run, Some(2));
        assert_eq!(engine_exec.alone, route::Isolation::Shared);
    }

    let runner_route = runner.route("aaaaaaaaaaaaaaaaaaaa");
    assert_eq!(option_state(runner_route), OptionState::Present);
    let Some(runner_route) = runner_route else {
        return;
    };
    assert_eq!(runner_route.index, None);
    assert_eq!(runner_route.considered, vec!["pkg/test/wide".to_owned()]);
    assert!(runner_route.executed.is_empty());
    assert_eq!(
        runner_route.discharged,
        vec![Discharge {
            target: "pkg/test/ui".to_owned(),
            proof: "branch-never-taken".to_owned(),
        }]
    );
    assert!(runner_route.discharges("pkg/test/ui"));
    let runner_exec = runner.execs.first();
    assert_eq!(option_state(runner_exec), OptionState::Present);
    if let Some(runner_exec) = runner_exec {
        assert_eq!(runner_exec.alone, route::Isolation::Alone);
        assert_eq!(runner_exec.tests_run, None);
    }
}

#[test]
fn a_line_that_is_not_json_rejects_the_entire_recording() {
    let result = route::read("not json\n\n{\"type\":\"route\"}\n", Producer::Runner);
    assert_eq!(
        result_state(&result),
        ResultState::Refused,
        "corrupt evidence must not become an empty recording"
    );
    if let Err(error) = result {
        assert_eq!(error.line, 1, "{error}");
    }
}

#[test]
fn a_mutant_nothing_routed_has_no_route_and_no_executions() {
    let Some(routing) = routing(ENGINE, Producer::Engine) else {
        return;
    };
    assert_eq!(
        option_state(routing.route("cccccccccccccccccccc")),
        OptionState::Absent
    );
    assert_eq!(routing.execs_of("bbbbbbbbbbbbbbbbbbbb").count(), 0);
}
