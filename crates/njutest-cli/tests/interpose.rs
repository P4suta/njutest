// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! An interposer between a test and what it talks to, recording what went past.

#![expect(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "a test reports a setup failure by panicking, asserts with panics, and reads as a table"
)]

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};

use njutest_cli::wire::interpose::{Interposer, Interposing};
use njutest_cli::wire::rule::Rule;
use njutest_cli::wire::{Spoken, Wire};

/// How long a test lets a held-up answer be held for, which is long enough to see and short enough to wait for.
const BRIEFLY: std::time::Duration = std::time::Duration::from_millis(50);

/// The status line of an answer nothing is wrong with.
const OK: &str = "HTTP/1.1 200 OK";

/// What an upstream says back, which is the whole of what a test has to decide about one.
#[derive(Clone, Copy)]
enum Answer {
    /// The same status line and body, however many times it is asked.
    Same(&'static str, &'static str),
    /// `200 OK` with a body numbered by which caller this was, so two answers differ.
    Numbered(&'static str),
}

impl Answer {
    /// The bytes the `which`th caller is handed.
    fn to(self, which: usize) -> String {
        let (line, body) = match self {
            Self::Same(line, body) => (line, body.to_owned()),
            Self::Numbered(stem) => (OK, format!("{stem}-{which}")),
        };
        format!("{line}\r\nContent-Length: {}\r\n\r\n{body}", body.len())
    }
}

/// An upstream that answers every caller until the test is done with it.
///
/// It is told what to say and never how often. A helper that stops at a number stops
/// answering the moment the number is wrong, and a caller left without an answer blocks,
/// so the test hangs until something outside it gives up and there is nothing to read.
/// The number is not a thing a test can know, either: a retry, a keep-alive, or a client
/// that opens two connections all move it without moving what is being measured. How
/// many there were is something to assert afterwards, by asking `served`.
struct Upstream {
    /// Where a caller dials it.
    address: std::net::SocketAddr,
    /// How many callers it has answered.
    served: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Set when the test is done with it, so the next accept returns and the thread ends.
    stopping: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// The thread doing the answering, taken when it is joined.
    serving: Option<std::thread::JoinHandle<()>>,
}

impl Upstream {
    /// An upstream at a port nobody chose, answering `answer` until it is dropped.
    fn answering(answer: Answer) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
        let address = listener.local_addr().expect("the address");
        let served = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let stopping = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let counted = std::sync::Arc::clone(&served);
        let told = std::sync::Arc::clone(&stopping);
        let serving = std::thread::spawn(move || {
            loop {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                if told.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                let which = counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut heard = [0_u8; 1024];
                let _read = stream.read(&mut heard);
                let _written = stream.write_all(answer.to(which).as_bytes());
                let _flushed = stream.flush();
            }
        });
        Self {
            address,
            served,
            stopping,
            serving: Some(serving),
        }
    }

    /// Where a caller dials it.
    const fn address(&self) -> std::net::SocketAddr {
        self.address
    }

    /// How many callers it has answered.
    fn served(&self) -> usize {
        self.served.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let _woken = TcpStream::connect(self.address);
        if let Some(serving) = self.serving.take() {
            let _joined = serving.join();
        }
    }
}

/// An upstream answering `200 OK` with `body`, until the test is done with it.
fn upstream(body: &'static str) -> Upstream {
    Upstream::answering(Answer::Same(OK, body))
}

/// One request through `address`, and what came back.
fn ask(address: std::net::SocketAddr, path: &str) -> String {
    let mut stream = TcpStream::connect(address).expect("the interposer is listening");
    let request = format!("GET {path} HTTP/1.1\r\nHost: test\r\n\r\n");
    stream.write_all(request.as_bytes()).expect("the request");
    stream.flush().expect("the request");
    let mut answer = String::new();
    let _read = stream.read_to_string(&mut answer);
    answer
}

#[test]
fn an_upstream_answers_however_many_callers_arrive_and_says_how_many_there_were() {
    let up = upstream("[]");
    for which in 0..5 {
        let answer = ask(up.address(), "/orders");
        assert!(
            answer.contains("200"),
            "caller {which} was answered, because a test that has to predict the \
             count predicts wrong the first time a client retries or keeps a \
             connection alive, and an unanswered caller blocks until something \
             outside the test gives up: {answer:?}"
        );
    }
    assert_eq!(
        up.served(),
        5,
        "and how many there were is read afterwards rather than declared up front, \
         so a count that surprises the test is an assertion it can fail on"
    );
}

#[test]
fn a_test_dials_the_interposer_and_gets_what_the_upstream_said() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = Interposer::start(&Interposing {
        capability: "api".to_owned(),
        upstream: upstream_at,
        wire: Wire::Http,
        injecting: None,
        held_up: BRIEFLY,
    })
    .expect("an interposer");

    let answer = ask(interposer.address(), "/orders");
    assert!(
        answer.contains("200 OK") && answer.ends_with("[]"),
        "the test has to get what the upstream said, byte for byte: an interposer \
         that changes an answer nobody asked it to change is measuring a different \
         program: {answer:?}"
    );

    let recorded = interposer.stop();
    assert_eq!(recorded.len(), 1, "one exchange went past: {recorded:?}");
    assert_eq!(recorded[0].capability, "api");
    assert_eq!(recorded[0].seq, 0);
    let spoken = &recorded[0].spoken;
    let Spoken::Http {
        method,
        path,
        status,
        response_bytes,
        ..
    } = spoken
    else {
        panic!("an HTTP interposer records HTTP: {spoken:?}");
    };
    assert_eq!(method, "GET");
    assert_eq!(path, "/orders");
    assert_eq!(*status, 200);
    assert!(
        *response_bytes > 0,
        "how much came back is what a derivation truncates, so it is recorded \
         even where nothing parsed the body"
    );
}

#[test]
fn every_exchange_on_one_seam_is_numbered_in_the_order_it_happened() {
    let up = upstream("ok");
    let upstream_at = up.address();
    let interposer = Interposer::start(&Interposing {
        capability: "api".to_owned(),
        upstream: upstream_at,
        wire: Wire::Http,
        injecting: None,
        held_up: BRIEFLY,
    })
    .expect("an interposer");

    let _first = ask(interposer.address(), "/one");
    let _second = ask(interposer.address(), "/two");
    let recorded = interposer.stop();

    let asked: Vec<String> = recorded.iter().map(|one| one.spoken.asked().0).collect();
    assert_eq!(
        asked,
        vec!["GET /one".to_owned(), "GET /two".to_owned()],
        "a fault that reorders two answers has to name which two, and a recording \
         that does not keep the order cannot say: {recorded:?}"
    );
    assert_eq!(recorded[0].seq, 0);
    assert_eq!(recorded[1].seq, 1);
}

#[test]
fn a_seam_nobody_spoke_to_records_nothing_rather_than_guessing() {
    let up = upstream("ok");
    let upstream_at = up.address();
    let interposer = Interposer::start(&Interposing {
        capability: "api".to_owned(),
        upstream: upstream_at,
        wire: Wire::Http,
        injecting: None,
        held_up: BRIEFLY,
    })
    .expect("an interposer");
    let recorded = interposer.stop();
    assert!(
        recorded.is_empty(),
        "a catalogue derived from this would otherwise hold a fault about an \
         exchange that never happened: {recorded:?}"
    );
}

#[test]
fn an_exchange_is_stamped_with_whoever_the_run_says_is_running() {
    let up = upstream("ok");
    let upstream_at = up.address();
    let interposer = Interposer::start(&Interposing {
        capability: "api".to_owned(),
        upstream: upstream_at,
        wire: Wire::Http,
        injecting: None,
        held_up: BRIEFLY,
    })
    .expect("an interposer");

    interposer.during(Some("pkg/test/it".to_owned()));
    let _first = ask(interposer.address(), "/one");
    interposer.during(None);
    let _second = ask(interposer.address(), "/two");

    let recorded = interposer.stop();
    assert_eq!(
        recorded[0].during.as_deref(),
        Some("pkg/test/it"),
        "which test was running is how a fault derived from an exchange is routed \
         back to the tests that could notice it: {recorded:?}"
    );
    assert_eq!(
        recorded[1].during, None,
        "and where the run cannot tell — several targets at once — it says so \
         rather than stamping the last one it happened to know: {recorded:?}"
    );
}

/// An interposer in front of `upstream_at` that is measuring one fault.
fn injecting(
    upstream_at: std::net::SocketAddr,
    fault: Option<njutest_cli::wire::derive::Fault>,
) -> Interposer {
    Interposer::start(&Interposing {
        capability: "api".to_owned(),
        upstream: upstream_at,
        wire: Wire::Http,
        injecting: fault,
        held_up: BRIEFLY,
    })
    .expect("an interposer")
}

/// The fault of `rule` about the exchange at `seq` of the `api` seam.
fn fault(rule: &str, seq: u64) -> njutest_cli::wire::derive::Fault {
    njutest_cli::wire::derive::Fault {
        id: "f".repeat(64),
        capability: "api".to_owned(),
        seq,
        during: None,
        rule: Rule::parse(rule).unwrap_or(Rule::DropConnection),
    }
}

#[test]
fn the_exchange_a_fault_names_is_answered_the_way_the_fault_says() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("status-server-error", 0)));

    let answer = ask(interposer.address(), "/orders");
    assert!(
        answer.contains("500"),
        "the upstream said 200 and the fault asks what happens if it had said 500, \
         so the caller has to see 500 or the question was never put: {answer:?}"
    );

    let _recorded = interposer.stop();
}

#[test]
fn an_exchange_the_fault_does_not_name_goes_past_untouched() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("status-server-error", 1)));

    let first = ask(interposer.address(), "/one");
    let second = ask(interposer.address(), "/two");

    assert!(
        first.contains("200"),
        "the fault is about the second exchange, and changing the first as well \
         would measure two faults and report one: {first:?}"
    );
    assert!(
        second.contains("500"),
        "and the one it names is the one that changes: {second:?}"
    );

    let _recorded = interposer.stop();
}

#[test]
fn a_dropped_connection_gives_the_caller_nothing_at_all() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("drop-connection", 0)));

    let answer = ask(interposer.address(), "/orders");
    assert!(
        answer.is_empty(),
        "a connection that dies with nothing said is what a caller meets when the \
         other end goes away, and anything at all here would be a different fault \
         from the one named: {answer:?}"
    );

    let _recorded = interposer.stop();
}

/// A lease as a provider answered it.
fn lease(capability: &str, named: &[(&str, &str)]) -> njutest_cli::resource::Lease {
    njutest_cli::resource::Lease {
        capability: capability.to_owned(),
        instance: "one".to_owned(),
        environment: named
            .iter()
            .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
            .collect(),
    }
}

#[test]
fn a_seam_the_configuration_names_is_one_the_tests_dial_through() {
    let up = upstream("ok");
    let upstream_at = up.address();
    let held = lease("api", &[("BASE_URL", &format!("http://{upstream_at}/v1"))]);

    let Some(watching) =
        njutest_cli::wire::dialled::interposed(&held, "BASE_URL", (Wire::Http, BRIEFLY))
    else {
        panic!("a lease naming an authority is one an interposer can sit in front of");
    };

    let told = watching
        .environment
        .iter()
        .find(|(name, _)| name == "BASE_URL")
        .map(|(_, value)| value.clone())
        .unwrap_or_default();
    assert_ne!(
        told,
        format!("http://{upstream_at}/v1"),
        "the tests are told to dial the interposer, or nothing is watching the seam"
    );
    assert!(
        told.ends_with("/v1"),
        "and everything but the authority survives: {told}"
    );

    let answer = ask_url(&told);
    assert!(
        answer.contains("200"),
        "and what comes back is what the upstream said: {answer:?}"
    );

    let recorded = watching.interposer.stop();
    assert_eq!(recorded.len(), 1, "the exchange was recorded: {recorded:?}");
    assert_eq!(recorded[0].capability, "api");
}

#[test]
fn a_lease_with_no_such_variable_is_left_as_the_provider_gave_it() {
    let held = lease("api", &[("OTHER", "http://127.0.0.1:9/x")]);
    assert!(
        njutest_cli::wire::dialled::interposed(&held, "BASE_URL", (Wire::Http, BRIEFLY)).is_none(),
        "a run that rewrote something else here would send the tests somewhere the \
         configuration never named"
    );
}

/// One request to a URL, and what came back.
fn ask_url(url: &str) -> String {
    let authority = njutest_cli::wire::dialled::upstream_of(url).expect("an authority");
    let path = url.split_once("://").map_or("/", |(_, rest)| {
        rest.find('/').map_or("/", |at| rest.split_at(at).1)
    });
    ask(authority.parse().expect("an address"), path)
}

#[test]
fn only_the_seams_the_configuration_names_are_watched() {
    use njutest_cli::assure::wire::watched;
    use std::collections::BTreeMap;

    let up = upstream("ok");
    let upstream_at = up.address();
    let held = lease("api", &[("BASE_URL", &format!("http://{upstream_at}/v1"))]);
    let quiet = lease("db", &[("DATABASE_URL", "postgres://u@127.0.0.1:1/x")]);

    let mut configured: BTreeMap<String, njutest_cli::config::Resource> = BTreeMap::new();
    configured.insert(
        "api".to_owned(),
        njutest_cli::config::Resource {
            command: vec!["x".to_owned()],
            timeout: std::time::Duration::from_secs(1),
            shared: false,
            exclusive: false,
            environment: Vec::new(),
            interpose: "BASE_URL".to_owned(),
            wire: Wire::Http,
        },
    );

    let seams = watched(&[&held, &quiet], &configured);
    assert_eq!(
        seams.watching.len(),
        1,
        "the configuration named one seam, and a run that watched the other as well \
         would be recording a program nobody asked it to look at"
    );

    let told: BTreeMap<String, String> = seams.environment.iter().cloned().collect();
    assert_ne!(
        told.get("BASE_URL").map(String::as_str),
        Some(format!("http://{upstream_at}/v1").as_str()),
        "the named one points at the interposer"
    );
    assert_eq!(
        told.get("DATABASE_URL").map(String::as_str),
        Some("postgres://u@127.0.0.1:1/x"),
        "and the unnamed one is exactly what the provider gave, because a run that \
         rewrote it would send the tests somewhere nobody chose"
    );

    let _answer = ask_url(told.get("BASE_URL").map_or("", String::as_str));
    let recorded = seams.recorded();
    assert_eq!(
        recorded.len(),
        1,
        "and what went past it is held: {recorded:?}"
    );
    assert_eq!(recorded[0].capability, "api");
}

#[test]
fn a_fault_put_after_the_first_names_an_exchange_a_second_run_of_the_suite_reaches() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, None);

    let _first = ask(interposer.address(), "/orders");
    let taken = interposer.taken();
    assert_eq!(
        taken.len(),
        1,
        "one exchange went past while nothing was put"
    );

    interposer.putting(Some(fault("status-server-error", 0)));
    let answer = ask(interposer.address(), "/orders");
    assert!(
        answer.contains("500"),
        "a fault names an exchange by its place in the order, so the run that puts \
         it starts the suite again and the count starts again with it; carrying the \
         old count over would name an exchange no run reaches and let every fault \
         after the first go past while the report said it had been put: {answer:?}"
    );

    interposer.putting(None);
    let untouched = ask(interposer.address(), "/orders");
    assert!(
        !untouched.contains("500"),
        "and a run told to put nothing puts nothing: {untouched:?}"
    );

    let _recorded = interposer.stop();
}

/// A seam named `capability`, watching `upstream_at`, with nothing put yet.
fn seam(
    capability: &str,
    upstream_at: std::net::SocketAddr,
) -> njutest_cli::wire::dialled::Watching {
    njutest_cli::wire::dialled::Watching {
        capability: capability.to_owned(),
        environment: Vec::new(),
        interposer: Interposer::start(&Interposing {
            capability: capability.to_owned(),
            upstream: upstream_at,
            wire: Wire::Http,
            injecting: None,
            held_up: BRIEFLY,
        })
        .expect("an interposer"),
    }
}

#[test]
fn every_question_a_seam_licensed_is_put_to_the_suite_one_at_a_time() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", upstream_at), seam("db", upstream_at)],
    };
    let _recorded = ask(seams.watching[0].interposer.address(), "/orders");

    let held = (
        rust_mutants::runner::Cancel::new(),
        njutest_cli::trace::Recorder::disabled(),
    );
    let seen = std::cell::RefCell::new(Vec::new());
    let done = njutest_cli::assure::wire::asking(
        &seams,
        || {
            let api = ask(seams.watching[0].interposer.address(), "/orders");
            let db = ask(seams.watching[1].interposer.address(), "/rows");
            seen.borrow_mut()
                .push((api.contains(" 200"), db.contains(" 200")));
            vec![njutest_cli::wire::settle::Answered {
                target: "pkg/test/it".to_owned(),
                passed: true,
            }]
        },
        njutest_cli::watch::Watch::new(&held.0, &held.1),
    );

    assert!(
        done.executed,
        "one exchange went past the api seam, so there was something to ask"
    );
    let seen = seen.borrow();
    assert_eq!(
        seen.len(),
        6,
        "one HTTP exchange licenses six questions, and the upstream here is sized \
         for exactly the connections those runs make, the replayed one included: \
         {seen:?}"
    );
    assert!(
        seen.iter().all(|(_api, db)| *db),
        "every fault here names the api seam, and a run that also changed the db \
         seam would measure two faults and report one: {seen:?}"
    );

    let after = ask(seams.watching[0].interposer.address(), "/orders");
    assert!(
        after.contains(" 200"),
        "the last fault a run puts has to be taken back out, or every phase after \
         this one measures a seam this phase broke: {after:?}"
    );

    for one in seams.watching {
        let _stopped = one.interposer.stop();
    }
}

#[test]
fn cutting_an_answer_with_no_body_short_hands_the_caller_what_it_would_have_had() {
    let up = upstream("");
    let upstream_at = up.address();
    let untouched = {
        let interposer = injecting(upstream_at, None);
        let answer = ask(interposer.address(), "/orders/1");
        let _recorded = interposer.stop();
        answer
    };
    let interposer = injecting(upstream_at, Some(fault("truncate-response", 0)));
    let cut = ask(interposer.address(), "/orders/1");

    assert_eq!(
        cut, untouched,
        "this is the premise the proof layer rests on, held against the code that \
         would have done the cutting rather than against a restatement of it. A layer \
         that removed this run while the two differed would report a gap as assured"
    );

    let _recorded = interposer.stop();
}

#[test]
fn a_suite_that_never_reached_the_exchange_a_fault_names_is_not_a_suite_that_missed_it() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, None);

    interposer.putting(Some(fault("status-server-error", 3)));
    let _first = ask(interposer.address(), "/orders");
    assert!(
        !interposer.was_put(),
        "one exchange came past and the fault names the fourth, so the question was \
         never put. A run that read the tests passing as nothing noticing would \
         report a gap where nobody was asked anything at all"
    );

    interposer.putting(Some(fault("status-server-error", 0)));
    let answer = ask(interposer.address(), "/orders");
    assert!(answer.contains("500") && interposer.was_put());

    let _recorded = interposer.stop();
}

#[test]
fn a_question_the_seam_never_reached_is_reported_as_a_hole_and_never_as_a_survivor() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", upstream_at)],
    };
    let _recorded = ask(seams.watching[0].interposer.address(), "/orders");

    let held = (
        rust_mutants::runner::Cancel::new(),
        njutest_cli::trace::Recorder::disabled(),
    );
    let done = njutest_cli::assure::wire::asking(
        &seams,
        || {
            vec![njutest_cli::wire::settle::Answered {
                target: "pkg/test/it".to_owned(),
                passed: true,
            }]
        },
        njutest_cli::watch::Watch::new(&held.0, &held.1),
    );

    assert!(
        done.findings
            .iter()
            .all(|one| one.kind == njutest_cli::report::FindingKind::NotMeasured),
        "the suite dialled nothing this time, so every question went unput and every \
         test passed for reasons of its own. Every one of those is a hole and not one \
         of them is a survivor: {:?}",
        done.findings
    );

    for one in seams.watching {
        let _stopped = one.interposer.stop();
    }
}

#[test]
fn a_request_the_run_replays_reaches_the_dependency_twice_and_the_caller_once() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("replay-request", 0)));

    let answer = ask(interposer.address(), "/orders");
    let _recorded = interposer.stop();

    assert!(
        answer.contains(" 200"),
        "the caller is handed the first answer, exactly as a retry after a lost \
         answer leaves it: {answer:?}"
    );
    assert_eq!(
        up.served(),
        2,
        "the dependency did the work twice and the caller never knew. What notices \
         is whatever holds the dependency's state, and a suite that notices nothing \
         is a suite that would not notice a double charge"
    );
}

#[test]
fn the_exchange_answered_with_a_stale_one_gets_what_the_one_before_it_got() {
    let up = Upstream::answering(Answer::Numbered("version"));
    let upstream_at = up.address();

    let interposer = injecting(upstream_at, Some(fault("stale-response", 1)));
    let first = ask(interposer.address(), "/orders");
    let second = ask(interposer.address(), "/orders");

    assert!(first.contains("version-0"));
    assert_eq!(
        second, first,
        "the second caller is handed what the first was handed, which is what reading \
         a replica the writer has outrun looks like from inside the program: {second:?}"
    );
    assert!(interposer.was_put());

    let _recorded = interposer.stop();
}

#[test]
fn the_first_exchange_on_a_seam_has_nothing_stale_to_be_answered_with() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("stale-response", 0)));

    let answer = ask(interposer.address(), "/orders");
    assert!(
        answer.contains(" 200"),
        "nothing came before it, so nothing was put: {answer:?}"
    );
    assert!(
        !interposer.was_put(),
        "and a question the interposer could not put is one the run established \
         nothing about, never a survivor because the tests then passed"
    );

    let _recorded = interposer.stop();
}

#[test]
fn restating_the_status_an_upstream_already_gave_hands_the_caller_what_it_would_have_had() {
    let up = Upstream::answering(Answer::Same("HTTP/1.1 500 Internal Server Error", "boom"));
    let upstream_at = up.address();
    let untouched = {
        let interposer = injecting(upstream_at, None);
        let answer = ask(interposer.address(), "/orders");
        let _recorded = interposer.stop();
        answer
    };
    let interposer = injecting(upstream_at, Some(fault("status-server-error", 0)));
    let restated = ask(interposer.address(), "/orders");

    assert_eq!(
        restated, untouched,
        "this is the premise the proof rests on, held against the code that would \
         have done the restating. A layer that removed this run while the two \
         differed would report a gap as assured"
    );

    let _recorded = interposer.stop();
}

#[test]
fn restating_a_status_worded_differently_hands_the_caller_something_else() {
    let up = Upstream::answering(Answer::Same("HTTP/1.1 500 Server Error", "boom"));
    let upstream_at = up.address();
    let untouched = {
        let interposer = injecting(upstream_at, None);
        let answer = ask(interposer.address(), "/orders");
        let _recorded = interposer.stop();
        answer
    };
    let interposer = injecting(upstream_at, Some(fault("status-server-error", 0)));
    let restated = ask(interposer.address(), "/orders");

    assert_ne!(
        restated, untouched,
        "and where the phrases differ the bytes differ, which is why the proof asks \
         about the line rather than about the code"
    );

    let _recorded = interposer.stop();
}

#[test]
fn what_a_fault_run_drives_through_a_second_seam_is_not_that_seam_s_catalogue() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", upstream_at), seam("db", upstream_at)],
    };
    let _baseline = ask(seams.watching[0].interposer.address(), "/orders");

    let held = (
        rust_mutants::runner::Cancel::new(),
        njutest_cli::trace::Recorder::disabled(),
    );
    let runs = std::cell::Cell::new(0_u32);
    let done = njutest_cli::assure::wire::asking(
        &seams,
        || {
            runs.set(runs.get().saturating_add(1));
            let _api = ask(seams.watching[0].interposer.address(), "/orders");
            let _db = ask(seams.watching[1].interposer.address(), "/rows");
            vec![njutest_cli::wire::settle::Answered {
                target: "pkg/test/it".to_owned(),
                passed: true,
            }]
        },
        njutest_cli::watch::Watch::new(&held.0, &held.1),
    );

    assert_eq!(
        runs.get(),
        6,
        "the db seam was silent when the run began, so it licensed nothing. The \
         traffic the api seam's own fault runs drove through it is a program already \
         being perturbed, and deriving a catalogue from that measures the run rather \
         than the system: {:?}",
        done.seams.len()
    );
    assert!(
        done.seams.iter().all(|one| one.capability == "api"),
        "and every question is about the seam that was dialled before anything was \
         put: {:?}",
        done.seams
            .iter()
            .map(|one| one.capability.clone())
            .collect::<Vec<_>>()
    );

    for one in seams.watching {
        let _stopped = one.interposer.stop();
    }
}
