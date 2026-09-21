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
    #[cfg(feature = "testkit")]
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
/// A value that names no authority is left alone rather than guessed at: a
/// run that invented one would put an interposer in front of something
/// nobody dials.
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
/// A credential, a path or a query the run dropped here would be a different
/// program from the one the baseline measured.
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
    #[cfg(feature = "testkit")]
    pub capability: String,
    /// The environment the tests are given, with the named variable pointing at the interposer.
    pub environment: Vec<(String, String)>,
    /// How long a held-up answer is held for, which a run putting that question must wait out.
    pub held_up: std::time::Duration,
    /// The interposer, which holds what went past.
    pub interposer: super::interpose::Interposer,
}

/// Puts an interposer in front of what `variable` of `lease` names, and says what the tests are told instead.
///
/// Nothing where the lease carries no such variable, or where its value names
/// no authority: a run that rewrote something else would send the tests
/// somewhere the configuration never named.
#[must_use]
pub fn interposed(
    lease: &crate::resource::Lease,
    variable: &str,
    (wire, held_up): (super::Wire, std::time::Duration),
) -> Option<Watching> {
    let given = lease
        .environment
        .iter()
        .find(|(name, _)| name == variable)
        .map(|(_, value)| value.clone())?;
    let upstream = match upstream_of(&given)?.parse::<std::net::SocketAddr>() {
        Ok(upstream) => upstream,
        Err(_) => return None,
    };
    let interposer = match super::interpose::Interposer::start(&super::interpose::Interposing {
        capability: lease.capability.clone(),
        upstream,
        wire,
        injecting: None,
        held_up,
    }) {
        Ok(interposer) => interposer,
        Err(_) => return None,
    };
    let told = redirected(&given, &interposer.address().to_string())?;
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
    Some(Watching {
        #[cfg(feature = "testkit")]
        capability: lease.capability.clone(),
        environment,
        held_up,
        interposer,
    })
}
