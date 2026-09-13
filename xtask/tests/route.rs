// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The one reader both audits ask about how a run routed, over the two recordings that write it down.

#![expect(
    clippy::indexing_slicing,
    reason = "a test asserts with panics and reads as a table"
)]

use xtask::route::{self, Discharge};

/// A recording as the engine writes it: the mutant by its full identity, a dense index, and the targets that ran.
const ENGINE: &str = r#"
{"seq":1,"timestamp":"2026-09-06T00:00:00Z","elapsed_ms":0,"type":"run-start","schema":"rust-mutants-trace-v1","engine":"0.1.0"}
{"seq":2,"timestamp":"2026-09-06T00:00:01Z","elapsed_ms":1,"type":"route","route":{"mutant":"aaaaaaaaaaaaaaaaaaaa","index":3,"granularity":"block","reaching":["pkg/lib/pkg"],"executed":["pkg/lib/pkg"]}}
{"seq":3,"timestamp":"2026-09-06T00:00:02Z","elapsed_ms":2,"type":"mutant-exec","mutant":{"id":"aaaaaaaaaaaaaaaaaaaa","index":3,"target":"pkg/lib/pkg","outcome":"killed","exit_code":101,"duration_ms":7,"tests_run":2}}
{"seq":4,"timestamp":"2026-09-06T00:00:03Z","elapsed_ms":3,"type":"route","route":{"mutant":"bbbbbbbbbbbbbbbbbbbb","index":4,"granularity":"unreached"}}
"#;

/// A recording as the runner writes it: the mutant by its short name, no index, and no list of what ran.
const RUNNER: &str = r#"
{"seq":1,"timestamp":"2026-09-06T00:00:00Z","elapsed_ms":0,"type":"route","route":{"mutant":"aaaaaaaaaaaaaaaaaaaa","granularity":"block","fallback":"touch-incomplete","reaching":["pkg/lib/pkg"],"discharged":[{"target":"pkg/test/ui","proof":"branch-never-taken"}],"considered":["pkg/test/wide"],"reused":null}}
{"seq":2,"timestamp":"2026-09-06T00:00:01Z","elapsed_ms":1,"type":"mutant-exec","mutant":{"mutant":"aaaaaaaaaaaaaaaaaaaa","target":"pkg/lib/pkg","args":[],"outcome":"survived","duration_ms":9,"alone":true}}
"#;

#[test]
fn the_route_reader_accepts_both_producer_vocabularies() {
    let engine = route::read(ENGINE);
    let runner = route::read(RUNNER);
    for (name, routing) in [("engine", &engine), ("runner", &runner)] {
        let found = routing
            .route("aaaaaaaaaaaaaaaaaaaa")
            .unwrap_or_else(|| panic!("{name} names the mutant it routed"));
        assert_eq!(found.reaching, vec!["pkg/lib/pkg".to_owned()], "{name}");
        let ran: Vec<&str> = routing
            .execs_of("aaaaaaaaaaaaaaaaaaaa")
            .map(|exec| exec.target.as_str())
            .collect();
        assert_eq!(ran, vec!["pkg/lib/pkg"], "{name} names what ran");
    }
    assert!(
        route::GRANULARITIES.contains(&engine.routes[0].granularity.as_str()),
        "{:?}",
        engine.routes
    );
    assert!(
        route::GRANULARITIES.contains(&runner.routes[0].granularity.as_str()),
        "{:?}",
        runner.routes
    );
    assert_eq!(
        route::ENGINE_GRANULARITIES,
        route::RUNNER_GRANULARITIES,
        "one rule decides a route now, so both recordings say it in one vocabulary"
    );
}

#[test]
fn each_producer_keeps_the_fields_only_it_records() {
    let engine = route::read(ENGINE);
    let runner = route::read(RUNNER);
    let engine_route = engine.route("aaaaaaaaaaaaaaaaaaaa").expect("the route");
    assert_eq!(engine_route.index, Some(3));
    assert_eq!(engine_route.executed, vec!["pkg/lib/pkg".to_owned()]);
    assert!(engine_route.considered.is_empty());
    assert_eq!(engine.execs[0].tests_run, Some(2));
    assert_eq!(engine.execs[0].alone, None);

    let runner_route = runner.route("aaaaaaaaaaaaaaaaaaaa").expect("the route");
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
    assert_eq!(runner.execs[0].alone, Some(true));
    assert_eq!(runner.execs[0].tests_run, None);
}

#[test]
fn a_line_that_is_not_an_event_is_skipped_rather_than_believed() {
    let routing = route::read("not json\n\n{\"type\":\"route\"}\n");
    assert!(routing.routes.is_empty(), "{routing:?}");
    assert!(routing.execs.is_empty(), "{routing:?}");
}

#[test]
fn a_mutant_nothing_routed_has_no_route_and_no_executions() {
    let routing = route::read(ENGINE);
    assert!(routing.route("cccccccccccccccccccc").is_none());
    assert_eq!(routing.execs_of("bbbbbbbbbbbbbbbbbbbb").count(), 0);
}
