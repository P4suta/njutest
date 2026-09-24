// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! A client of two services, one whose answer this program acts on and one whose answer it does not.

use std::io::{Read as _, Write as _};
use std::net::TcpStream;

/// Where one exchange with a service stopped short of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// No connection was made, so nothing reached the service.
    Connect,
    /// The connection was made and the request could not be sent.
    Write,
    /// The request was sent and no whole answer came back.
    Read,
    /// An answer came back and it was not `201` with a body.
    Status,
}

/// Places an order with the service at `base`, and says what it was called.
///
/// # Errors
/// The stage the exchange stopped at, where it did not end in an order.
pub fn place(base: &str) -> Result<String, Stage> {
    let answer = ask(base, "POST", "/orders")?;
    let (head, body) = answer.split_once("\r\n\r\n").ok_or(Stage::Status)?;
    if !head.starts_with("HTTP/1.1 201") {
        return Err(Stage::Status);
    }
    Ok(body.to_owned())
}

/// Tells the service at `base` this program is alive, and says nothing about how that went.
pub fn ping(base: &str) {
    let _answer = ask(base, "GET", "/health");
}

/// One request, and everything that came back.
fn ask(base: &str, method: &str, path: &str) -> Result<String, Stage> {
    let authority = base.split_once("://").map_or(base, |(_, rest)| rest);
    let authority = authority.split_once('/').map_or(authority, |(at, _)| at);
    let mut stream = TcpStream::connect(authority).map_err(|_| Stage::Connect)?;
    let request = format!("{method} {path} HTTP/1.1\r\nHost: {authority}\r\n\r\n");
    stream.write_all(request.as_bytes()).map_err(|_| Stage::Write)?;
    stream.flush().map_err(|_| Stage::Write)?;
    let mut answer = String::new();
    let _read = stream.read_to_string(&mut answer).map_err(|_| Stage::Read)?;
    Ok(answer)
}
