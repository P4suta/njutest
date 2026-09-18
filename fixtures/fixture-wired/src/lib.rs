// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A client of two services, one whose answer this program acts on and one whose answer it does not.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

/// Places an order with the service at `base`, and says what it was called.
///
/// # Panics
/// Never: a service that cannot be reached, or that answers something other
/// than `201`, is reported as no order rather than as a crash.
#[must_use]
pub fn place(base: &str) -> Option<String> {
    let answer = ask(base, "POST", "/orders")?;
    let (head, body) = answer.split_once("\r\n\r\n")?;
    if !head.starts_with("HTTP/1.1 201") {
        return None;
    }
    Some(body.to_owned())
}

/// Tells the service at `base` this program is alive, and says nothing about how that went.
pub fn ping(base: &str) {
    let _answer = ask(base, "GET", "/health");
}

/// One request, and everything that came back.
fn ask(base: &str, method: &str, path: &str) -> Option<String> {
    let authority = base.split_once("://").map_or(base, |(_, rest)| rest);
    let authority = authority.split_once('/').map_or(authority, |(at, _)| at);
    let mut stream = TcpStream::connect(authority).ok()?;
    let request = format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    stream.flush().ok()?;
    let mut answer = String::new();
    let _read = stream.read_to_string(&mut answer).ok()?;
    Some(answer)
}
