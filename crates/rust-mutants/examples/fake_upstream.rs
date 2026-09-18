// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A resource provider that starts a real HTTP dependency and says where it is.

#![expect(
    clippy::print_stdout,
    reason = "printing on the standard stream is how a provider answers, and this program is one"
)]
#![expect(
    clippy::expect_used,
    reason = "a provider that cannot bind a port has no answer to give, and saying so by dying \
              is what the run reads as a provider that could not start"
)]

use std::io::{BufRead as _, Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// The exit code a role nobody wrote leaves.
const UNKNOWN_ROLE_EXIT: u8 = 99;

/// What the orders service calls the order it was asked to place.
///
/// The same every time on purpose. A caller cannot tell a replayed request
/// from one delivery when the answer does not move, which is the whole of
/// what `replay-request` asks of a suite, and an identifier that counted up
/// would answer that question with an artifact of this program instead.
const PLACED: &str = "order-1";

fn main() -> ExitCode {
    match std::env::args().nth(1).unwrap_or_default().as_str() {
        "orders" => serve(Answer::Placed, "ORDERS_URL"),
        "health" => serve(Answer::Nothing, "HEALTH_URL"),
        other => {
            eprintln!("fake-upstream: {other:?} is not a service this program knows how to be");
            ExitCode::from(UNKNOWN_ROLE_EXIT)
        }
    }
}

/// What one of the two services this program can be says back.
#[derive(Clone, Copy)]
enum Answer {
    /// `201` naming the order, which is something a caller can be wrong about.
    Placed,
    /// `200` and no body at all, which is what a liveness check is.
    Nothing,
}

impl Answer {
    /// The bytes a caller is handed.
    fn bytes(self) -> Vec<u8> {
        let (line, body) = match self {
            Self::Placed => ("HTTP/1.1 201 Created", PLACED),
            Self::Nothing => ("HTTP/1.1 200 OK", ""),
        };
        format!("{line}\r\nContent-Length: {}\r\n\r\n{body}", body.len()).into_bytes()
    }
}

/// Starts the service, tells the run where it is under `names`, and answers until the run says to stop.
fn serve(answer: Answer, names: &str) -> ExitCode {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a port");
    let address = listener.local_addr().expect("the address");
    let stopping = Arc::new(AtomicBool::new(false));
    let told = Arc::clone(&stopping);
    let answering = std::thread::spawn(move || {
        loop {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            if told.load(Ordering::Relaxed) {
                return;
            }
            let mut heard = [0_u8; 4096];
            let _read = stream.read(&mut heard);
            let _written = stream.write_all(&answer.bytes());
            let _flushed = stream.flush();
        }
    });

    for line in std::io::stdin().lock().lines().map_while(Result::ok) {
        if line.contains(r#""action":"start""#) {
            say(&format!(
                r#"{{"version":1,"status":"ready","instance":"one","environment":{{"{names}":"http://{address}"}}}}"#
            ));
        } else if line.contains(r#""action":"stop""#) {
            stopping.store(true, Ordering::Relaxed);
            let _woken = TcpStream::connect(address);
            let _joined = answering.join();
            say(r#"{"version":1,"status":"stopped","instance":"one"}"#);
            return ExitCode::SUCCESS;
        }
    }
    ExitCode::SUCCESS
}

/// Says one line and lets the run read it now: a provider a run waits on says nothing while its output sits in a buffer.
fn say(line: &str) {
    println!("{line}");
    let _flushed = std::io::stdout().flush();
}
