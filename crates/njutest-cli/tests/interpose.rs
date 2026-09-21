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
                let Ok(which) = counted.fetch_update(
                    std::sync::atomic::Ordering::Relaxed,
                    std::sync::atomic::Ordering::Relaxed,
                    |value| value.checked_add(1),
                ) else {
                    return;
                };
                let mut heard = [0_u8; 1024];
                let read = stream.read(&mut heard).expect("the request is readable");
                if read == 0 {
                    return;
                }
                stream
                    .write_all(answer.to(which).as_bytes())
                    .expect("the answer is writable");
                stream.flush().expect("the answer is flushed");
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

    fn join(&mut self) -> Result<(), UpstreamFailure> {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Relaxed);
        TcpStream::connect(self.address)
            .map_err(|source| UpstreamFailure::CannotWake { source })?;
        let Some(serving) = self.serving.take() else {
            return Ok(());
        };
        match serving.join() {
            Ok(()) => Ok(()),
            Err(panic) => {
                drop(panic);
                Err(UpstreamFailure::Panicked)
            }
        }
    }
}

impl Drop for Upstream {
    fn drop(&mut self) {
        if self.join().is_err() {
            std::process::abort();
        }
    }
}

#[derive(Debug, thiserror::Error)]
enum UpstreamFailure {
    #[error("the upstream listener could not be woken: {source}")]
    CannotWake {
        #[source]
        source: std::io::Error,
    },
    #[error("the upstream listener panicked")]
    Panicked,
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
    stream
        .read_to_string(&mut answer)
        .expect("the complete answer is readable");
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

    let first = ask(interposer.address(), "/one");
    drop(first);
    let second = ask(interposer.address(), "/two");
    drop(second);
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
    let first = ask(interposer.address(), "/one");
    drop(first);
    interposer.during(None);
    let second = ask(interposer.address(), "/two");
    drop(second);

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

    let recorded = interposer.stop();
    drop(recorded);
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

    let recorded = interposer.stop();
    drop(recorded);
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

    let recorded = interposer.stop();
    drop(recorded);
}

#[test]
fn a_fault_names_one_exchange_and_the_next_one_goes_through() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("drop-connection", 0)));

    let dropped = ask(interposer.address(), "/orders");
    assert!(
        dropped.is_empty(),
        "the exchange the fault names is the one that meets it: {dropped:?}"
    );
    let after = ask(interposer.address(), "/orders");
    assert!(
        !after.is_empty(),
        "and the one after it does not: a run that measured two perturbed calls and \
         reported one fault would be measuring a program nobody described. The \
         interposer numbers an exchange only when it records one, so a rule that \
         records nothing left every later call answering to the same number: {after:?}"
    );

    let recorded = interposer.stop();
    drop(recorded);
}

/// A lease as a provider answered it.
fn lease(capability: &str, named: &[(&str, &str)]) -> njutest_cli::resource::Lease {
    njutest_cli::resource::Lease {
        capability: capability.to_owned(),
        instance: njutest_cli::provider::InstanceId::checked("one").expect("instance id"),
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

    let Ok(watching) =
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
    let Err(refused) =
        njutest_cli::wire::dialled::interposed(&held, "BASE_URL", (Wire::Http, BRIEFLY))
    else {
        panic!("a lease carrying no such variable is one nothing can be put in front of")
    };
    assert_eq!(
        refused,
        njutest_cli::wire::dialled::NotWatched::NoSuchVariable,
        "a run that rewrote something else here would send the tests somewhere the \
         configuration never named, and one that said nothing would leave the seam \
         unmeasured and the verdict reading as if it were covered"
    );
}

#[test]
fn a_seam_whose_authority_names_a_host_is_watched_like_any_other() {
    let up = upstream("[]");
    let named = format!("http://localhost:{}/orders", up.address().port());
    let held = lease("api", &[("BASE_URL", named.as_str())]);
    let watching = njutest_cli::wire::dialled::interposed(&held, "BASE_URL", (Wire::Http, BRIEFLY));
    assert!(
        watching.is_ok(),
        "`localhost:{}` is the ordinary way a provider names where it is, and a run that \
         could not read it started no interposer, recorded no seam, raised no finding and \
         stated no limitation: the wire dimension went unmeasured and the verdict read as \
         if it had been covered",
        up.address().port()
    );
}

#[test]
fn a_seam_the_run_could_not_watch_says_which_of_the_ways_it_could_not() {
    let held = lease("api", &[("BASE_URL", "not a url at all")]);
    let refused = njutest_cli::wire::dialled::interposed(&held, "BASE_URL", (Wire::Http, BRIEFLY))
        .expect_err("a value naming no authority");
    assert_eq!(
        refused,
        njutest_cli::wire::dialled::NotWatched::NamesNoAuthority
    );
    assert!(
        !refused.why().is_empty(),
        "and it says so in a sentence a reader can act on"
    );
}

/// One request to a URL, and what came back.
fn ask_url(url: &str) -> String {
    let authority = njutest_cli::wire::dialled::upstream_of(url).expect("an authority");
    let path = url.split_once("://").map_or("/", |(_, rest)| {
        rest.find('/').map_or("/", |at| rest.split_at(at).1)
    });
    ask(
        authority
            .parse::<std::net::SocketAddr>()
            .expect("an address"),
        path,
    )
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
            hold: BRIEFLY,
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

    let answer = ask_url(told.get("BASE_URL").map_or("", String::as_str));
    drop(answer);
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

    let first = ask(interposer.address(), "/orders");
    drop(first);
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

    let recorded = interposer.stop();
    drop(recorded);
}

/// A seam named `capability`, watching `upstream_at`, with nothing put yet.
fn seam(
    capability: &str,
    upstream_at: std::net::SocketAddr,
) -> njutest_cli::wire::dialled::Watching {
    njutest_cli::wire::dialled::Watching {
        capability: capability.to_owned(),
        environment: Vec::new(),
        held_up: std::time::Duration::ZERO,
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
        unwatched: Vec::new(),
    };
    let held = (
        rust_mutants::runner::Cancel::new(),
        njutest_cli::trace::Recorder::disabled(),
    );
    let seen = std::cell::RefCell::new(Vec::new());
    let went_past = seams.observing(|| {
        let recorded = ask(seams.watching[0].interposer.address(), "/orders");
        drop(recorded);
    });
    let done = njutest_cli::assure::wire::asking(
        &seams,
        &went_past,
        || {
            let api = ask(seams.watching[0].interposer.address(), "/orders");
            let db = ask(seams.watching[1].interposer.address(), "/rows");
            seen.borrow_mut()
                .push((api.contains(" 200"), db.contains(" 200")));
            njutest_cli::wire::settle::Asked::Answered(vec![njutest_cli::wire::settle::Answered {
                target: "pkg/test/it".to_owned(),
                passed: true,
            }])
        },
        njutest_cli::watch::Watch::new(&held.0, &held.1),
    );
    let done = match done {
        Ok(done) => done,
        Err(error) => panic!("the licensed seam catalogue should be derivable: {error}"),
    };

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
        let stopped = one.interposer.stop();
        drop(stopped);
    }
}

#[test]
fn cutting_an_answer_with_no_body_short_hands_the_caller_what_it_would_have_had() {
    let up = upstream("");
    let upstream_at = up.address();
    let untouched = {
        let interposer = injecting(upstream_at, None);
        let answer = ask(interposer.address(), "/orders/1");
        let recorded = interposer.stop();
        drop(recorded);
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

    let recorded = interposer.stop();
    drop(recorded);
}

#[test]
fn a_suite_that_never_reached_the_exchange_a_fault_names_is_not_a_suite_that_missed_it() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, None);

    interposer.putting(Some(fault("status-server-error", 3)));
    let first = ask(interposer.address(), "/orders");
    drop(first);
    assert!(
        !interposer.was_put(),
        "one exchange came past and the fault names the fourth, so the question was \
         never put. A run that read the tests passing as nothing noticing would \
         report a gap where nobody was asked anything at all"
    );

    interposer.putting(Some(fault("status-server-error", 0)));
    let answer = ask(interposer.address(), "/orders");
    assert!(answer.contains("500") && interposer.was_put());

    let recorded = interposer.stop();
    drop(recorded);
}

#[test]
fn a_question_the_seam_never_reached_is_reported_as_a_hole_and_never_as_a_survivor() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", upstream_at)],
        unwatched: Vec::new(),
    };
    let held = (
        rust_mutants::runner::Cancel::new(),
        njutest_cli::trace::Recorder::disabled(),
    );
    let went_past = seams.observing(|| {
        let recorded = ask(seams.watching[0].interposer.address(), "/orders");
        drop(recorded);
    });
    let done = njutest_cli::assure::wire::asking(
        &seams,
        &went_past,
        || {
            njutest_cli::wire::settle::Asked::Answered(vec![njutest_cli::wire::settle::Answered {
                target: "pkg/test/it".to_owned(),
                passed: true,
            }])
        },
        njutest_cli::watch::Watch::new(&held.0, &held.1),
    );
    let done = match done {
        Ok(done) => done,
        Err(error) => panic!("the unreached seam catalogue should be derivable: {error}"),
    };

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
        let stopped = one.interposer.stop();
        drop(stopped);
    }
}

#[test]
fn a_request_the_run_replays_reaches_the_dependency_twice_and_the_caller_once() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let interposer = injecting(upstream_at, Some(fault("replay-request", 0)));

    let answer = ask(interposer.address(), "/orders");
    let recorded = interposer.stop();
    drop(recorded);

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

    let recorded = interposer.stop();
    drop(recorded);
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

    let recorded = interposer.stop();
    drop(recorded);
}

#[test]
fn restating_the_status_an_upstream_already_gave_hands_the_caller_what_it_would_have_had() {
    let up = Upstream::answering(Answer::Same("HTTP/1.1 500 Internal Server Error", "boom"));
    let upstream_at = up.address();
    let untouched = {
        let interposer = injecting(upstream_at, None);
        let answer = ask(interposer.address(), "/orders");
        let recorded = interposer.stop();
        drop(recorded);
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

    let recorded = interposer.stop();
    drop(recorded);
}

#[test]
fn restating_a_status_worded_differently_hands_the_caller_something_else() {
    let up = Upstream::answering(Answer::Same("HTTP/1.1 500 Server Error", "boom"));
    let upstream_at = up.address();
    let untouched = {
        let interposer = injecting(upstream_at, None);
        let answer = ask(interposer.address(), "/orders");
        let recorded = interposer.stop();
        drop(recorded);
        answer
    };
    let interposer = injecting(upstream_at, Some(fault("status-server-error", 0)));
    let restated = ask(interposer.address(), "/orders");

    assert_ne!(
        restated, untouched,
        "and where the phrases differ the bytes differ, which is why the proof asks \
         about the line rather than about the code"
    );

    let recorded = interposer.stop();
    drop(recorded);
}

#[test]
fn an_exchange_says_which_target_caused_it_where_the_run_can_tell() {
    let up = upstream("[]");
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", up.address())],
        unwatched: Vec::new(),
    };
    let at = seams.watching[0].interposer.address();

    let went_past = seams.observing(|| {
        seams.during(Some("pkg/test/one"));
        let first = ask(at, "/orders");
        drop(first);
        seams.during(Some("pkg/test/two"));
        let second = ask(at, "/rows");
        drop(second);
        seams.during(None);
    });

    let who: Vec<Option<String>> = went_past
        .all()
        .iter()
        .map(|one| one.during.clone())
        .collect();
    assert_eq!(
        who,
        vec![
            Some("pkg/test/one".to_owned()),
            Some("pkg/test/two".to_owned())
        ],
        "a run told to measure only what changed can skip a question whose target \
         did not change, and only if the recording says whose it was. Naming the \
         wrong one would skip on a false premise, which is worse than asking again"
    );
}

#[test]
fn a_catalogue_holds_one_run_of_the_suite_and_never_the_runs_around_it() {
    let up = upstream("[]");
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", up.address())],
        unwatched: Vec::new(),
    };
    let at = seams.watching[0].interposer.address();
    for _verifying in 0..2 {
        let before = ask(at, "/orders");
        drop(before);
    }

    let went_past = seams.observing(|| {
        let measured = ask(at, "/orders");
        drop(measured);
    });
    for _mutation in 0..4 {
        let after = ask(at, "/orders");
        drop(after);
    }

    assert_eq!(
        went_past.all().len(),
        1,
        "the suite makes one exchange, and the phases around this one ran it six more \
         times. A fault names an exchange by its place in the order and is put by \
         running the suite once, so every exchange but the observed run's is one no \
         run can reach and the report would state each as a question nobody put: {:?}",
        went_past.all()
    );
    assert_eq!(
        went_past.all().first().map(|one| one.seq),
        Some(0),
        "and it is numbered from zero, because a count carried over from the runs \
         before would name an exchange the answering run never reaches"
    );

    for one in seams.watching {
        let stopped = one.interposer.stop();
        drop(stopped);
    }
}

#[test]
fn what_a_fault_run_drives_through_a_second_seam_is_not_that_seam_s_catalogue() {
    let up = upstream("[]");
    let upstream_at = up.address();
    let seams = njutest_cli::assure::wire::Seams {
        environment: Vec::new(),
        watching: vec![seam("api", upstream_at), seam("db", upstream_at)],
        unwatched: Vec::new(),
    };
    let held = (
        rust_mutants::runner::Cancel::new(),
        njutest_cli::trace::Recorder::disabled(),
    );
    let runs = std::cell::Cell::new(0_u32);
    let went_past = seams.observing(|| {
        let baseline = ask(seams.watching[0].interposer.address(), "/orders");
        drop(baseline);
    });
    let done = njutest_cli::assure::wire::asking(
        &seams,
        &went_past,
        || {
            let next = match runs.get().checked_add(1) {
                Some(next) => next,
                None => panic!("the six-run fixture count must remain representable"),
            };
            runs.set(next);
            let api = ask(seams.watching[0].interposer.address(), "/orders");
            drop(api);
            let db = ask(seams.watching[1].interposer.address(), "/rows");
            drop(db);
            njutest_cli::wire::settle::Asked::Answered(vec![njutest_cli::wire::settle::Answered {
                target: "pkg/test/it".to_owned(),
                passed: true,
            }])
        },
        njutest_cli::watch::Watch::new(&held.0, &held.1),
    );
    let done = match done {
        Ok(done) => done,
        Err(error) => panic!("the multi-seam catalogue should be derivable: {error}"),
    };

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
        let stopped = one.interposer.stop();
        drop(stopped);
    }
}
