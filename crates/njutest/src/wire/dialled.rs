// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! What a test is told to dial, once an interposer sits in front of what it would have.

/// What one variable of a lease named, and what an interposer would sit in front of.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg(feature = "testkit")]
pub struct Dialled {
    /// The variable the test reads the address out of.
    pub variable: String,
    /// The value as the provider gave it.
    pub given: String,
    /// The authority the value names, which is what an interposer takes the place of.
    pub upstream: String,
}

#[cfg(feature = "testkit")]
impl Dialled {
    /// What `value` names, or nothing where it names no authority to sit in front of.
    #[must_use]
    pub fn of(variable: &str, value: &str) -> Option<Self> {
        Some(Self {
            variable: variable.to_owned(),
            given: value.to_owned(),
            upstream: upstream_of(value)?,
        })
    }
}

/// The authority `value` names — `host:port`, without any credential — or nothing where it names none.
///
/// A value that names no authority is left alone rather than guessed at: a run that invented one would put an interposer in front of something nobody dials.
#[must_use]
pub fn upstream_of(value: &str) -> Option<String> {
    let (scheme, rest) = value.split_once("://")?;
    if scheme.is_empty() {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() || !host.contains(':') {
        return None;
    }
    Some(host.to_owned())
}

/// `value` with its authority replaced by `interposer`, and everything else as it was.
///
/// A credential, a path or a query the run dropped here would be a different program from the one the baseline measured.
#[must_use]
pub fn redirected(value: &str, interposer: &str) -> Option<String> {
    let (scheme, rest) = value.split_once("://")?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = rest.get(..end)?;
    let tail = rest.get(end..).unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or(authority);
    if host.is_empty() || !host.contains(':') {
        return None;
    }
    let credential = authority
        .strip_suffix(host)
        .filter(|before| !before.is_empty())
        .unwrap_or_default();
    Some(format!("{scheme}://{credential}{interposer}{tail}"))
}

/// A seam an interposer is watching: what the tests are told, and the interposer itself.
#[derive(Debug)]
pub struct Watching {
    /// The capability the seam serves, which is what a fault names when it says where to go.
    ///
    /// Read in earnest as well as by a test: a recording names the seams it came from so pairing it back to them is checked rather than assumed.
    pub capability: String,
    /// The environment the tests are given, with the named variable pointing at the interposer.
    pub environment: Vec<(String, String)>,
    /// How long a held-up answer is held for, which a run putting that question must wait out.
    pub held_up: std::time::Duration,
    /// The interposer, which holds what went past.
    pub interposer: super::interpose::Interposer,
}

/// Why a seam the configuration named is not one this run watched.
///
/// Five things can stop an interposer going in, and a run that answered all of them with silence measured no seam and said nothing about it, so a reader read the wire dimension as covered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, njutest_macros::AllVariants)]
pub enum NotWatched {
    /// The lease carries no variable of that name.
    NoSuchVariable,
    /// The value names no authority, so there is nothing to put an interposer in front of.
    NamesNoAuthority,
    /// The authority names a host this machine could not resolve to an address.
    AuthorityUnresolved,
    /// The interposer would not start.
    WouldNotListen,
    /// The value could not be rewritten to point at the interposer.
    NotRedirected,
}

impl NotWatched {
    /// One sentence a reader can act on.
    #[must_use]
    pub const fn why(self) -> &'static str {
        match self {
            Self::NoSuchVariable => {
                "the provider's lease carries no variable of the name the configuration interposes on"
            }
            Self::NamesNoAuthority => {
                "the variable's value names no authority, so there is nothing to stand in front of"
            }
            Self::AuthorityUnresolved => {
                "the authority names a host this machine could not resolve to an address"
            }
            Self::WouldNotListen => "the interposer could not take a port on this machine",
            Self::NotRedirected => {
                "the variable's value could not be rewritten to name the interposer"
            }
        }
    }
}

/// Puts an interposer in front of what `variable` of `lease` names, and says what the tests are told instead.
///
/// # Errors
/// Which of the five ways the seam is one this run did not watch, so the caller states it rather than measuring nothing and saying nothing.
pub fn interposed(
    lease: &crate::resource::Lease,
    variable: &str,
    (wire, held_up): (super::Wire, std::time::Duration),
) -> Result<Watching, NotWatched> {
    let given = lease
        .environment
        .iter()
        .find(|(name, _)| name == variable)
        .map(|(_, value)| value.clone())
        .ok_or(NotWatched::NoSuchVariable)?;
    let authority = upstream_of(&given).ok_or(NotWatched::NamesNoAuthority)?;
    let upstream = resolved(&authority).ok_or(NotWatched::AuthorityUnresolved)?;
    let interposer = super::interpose::Interposer::start(&super::interpose::Interposing {
        capability: lease.capability.clone(),
        upstream,
        wire,
        injecting: None,
        held_up,
    })
    .map_err(|_would_not_listen| NotWatched::WouldNotListen)?;
    let told =
        redirected(&given, &interposer.address().to_string()).ok_or(NotWatched::NotRedirected)?;
    let environment = lease
        .environment
        .iter()
        .map(|(name, value)| {
            if name == variable {
                (name.clone(), told.clone())
            } else {
                (name.clone(), value.clone())
            }
        })
        .collect();
    Ok(Watching {
        capability: lease.capability.clone(),
        environment,
        held_up,
        interposer,
    })
}

/// The address `authority` names, asking the machine where a host is rather than requiring a literal.
///
/// A provider that answers `localhost:5432` has named where it is the way everything else does.
fn resolved(authority: &str) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs as _;
    match authority.to_socket_addrs() {
        Ok(mut found) => found.next(),
        Err(_this_machine_knows_no_address_for_it) => None,
    }
}
