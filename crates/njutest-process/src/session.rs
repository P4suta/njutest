// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Native session custody retained by the original launching owner.

use std::io::{self, Read as _, Write as _};
use std::os::fd::AsFd as _;
use std::os::unix::net::UnixStream;
use std::process::{Command, Stdio};

use super::GroupChild;

/// The explicit inherited channel offered by the original session owner.
pub const SESSION_CUSTODY: &str = "NJUTEST_SESSION_CUSTODY";

/// A native session whose leader and inherited producer groups remain owned.
#[derive(Debug)]
pub struct SessionOwner {
    child: GroupChild,
    lease: UnixStream,
}

impl SessionOwner {
    /// Starts the original session and passes its private custody endpoint as standard input.
    ///
    /// # Errors
    /// Creation or custody publication failed after mandatory native cleanup.
    pub fn launch(command: &mut Command) -> io::Result<Self> {
        let (mut lease, endpoint) = UnixStream::pair()?;
        command.stdin(Stdio::from(std::os::fd::OwnedFd::from(endpoint)));
        command.env(SESSION_CUSTODY, "stdin-v1");
        let child = GroupChild::start_session(command)?;
        let session = child
            .id()
            .ok_or_else(|| io::Error::other("the original session has no retained leader"))?;
        lease.write_all(b"NJSCOPE1")?;
        lease.write_all(&session.to_le_bytes())?;
        Ok(Self { child, lease })
    }

    /// Borrows the actual original native scope while its custody lease remains owned.
    #[must_use]
    pub const fn scope(&self) -> &GroupChild {
        &self.child
    }

    /// Borrows the actual original native scope without transferring its custody lease.
    pub const fn scope_mut(&mut self) -> &mut GroupChild {
        &mut self.child
    }

    /// Confirms the real foreground status without consuming the original session owner.
    ///
    /// # Errors
    /// Native leader observation failed after mandatory cleanup.
    pub fn observe_status(&mut self) -> io::Result<std::process::ExitStatus> {
        self.child.observe_status()
    }

    /// Checks the actual foreground event without disposing its retained producer session.
    ///
    /// # Errors
    /// Native leader observation failed after mandatory cleanup.
    pub fn try_observe_status(&mut self) -> io::Result<Option<std::process::ExitStatus>> {
        self.child.try_observe_status()
    }
}

impl Drop for SessionOwner {
    fn drop(&mut self) {
        if let Err(source) = self.child.stop() {
            super::group::terminal(&format!("the original session cleanup refused: {source}"));
        }
        if let Err(source) = self.lease.shutdown(std::net::Shutdown::Both) {
            eprintln!("the settled original session custody endpoint refused shutdown: {source}");
        }
    }
}

/// The acknowledged original recipient of naturally completed nested groups.
#[derive(Debug)]
pub struct ParentSession {
    session: rustix::process::Pid,
    lease: UnixStream,
    parent: super::ForeignProcess,
}

impl ParentSession {
    /// Accepts the actual native parent's private endpoint before nested work can start.
    ///
    /// # Errors
    /// The inherited channel, native parent generation or original session did not match.
    pub fn accept_stdin() -> io::Result<Self> {
        let endpoint = io::stdin().as_fd().try_clone_to_owned()?;
        let mut lease = UnixStream::from(endpoint);
        let parent_pid = super::sys::custody_peer(&lease)?;
        if Some(parent_pid) != rustix::process::getppid() {
            return Err(io::Error::other(
                "the custody endpoint is not the actual native parent",
            ));
        }
        let parent = super::ForeignProcess::retain(
            u32::try_from(parent_pid.as_raw_nonzero().get()).map_err(io::Error::other)?,
        )?
        .ok_or_else(|| io::Error::other("the original native parent already ended"))?;
        let began = std::time::Instant::now();
        let deadline = began
            .checked_add(super::REAPING_GRACE)
            .ok_or_else(|| io::Error::other("the native custody admission bound overflowed"))?;
        lease.set_read_timeout(Some(super::REAPING_GRACE))?;
        let mut magic = [0_u8; 8];
        lease.read_exact(&mut magic)?;
        if magic != *b"NJSCOPE1" {
            return Err(io::Error::other(
                "the original custody protocol did not match",
            ));
        }
        let left = deadline
            .checked_duration_since(std::time::Instant::now())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "the native custody offer did not complete",
                )
            })?;
        lease.set_read_timeout(Some(left))?;
        let mut bytes = [0_u8; 4];
        lease.read_exact(&mut bytes)?;
        lease.set_read_timeout(None)?;
        let session = i32::try_from(u32::from_le_bytes(bytes)).map_err(io::Error::other)?;
        let session = rustix::process::Pid::from_raw(session)
            .ok_or_else(|| io::Error::other("the original session identity is invalid"))?;
        if session != rustix::process::getpid() || session != rustix::process::getsid(None)? {
            return Err(io::Error::other(
                "the custody offer names another native session",
            ));
        }
        lease.write_all(b"NJSCOPE1")?;
        let claimed = Self {
            session,
            lease,
            parent,
        };
        claimed.confirm()?;
        Ok(claimed)
    }

    pub(crate) fn confirm_child(
        &self,
        child: &std::process::Child,
        original: rustix::process::Pid,
        status: std::process::ExitStatus,
    ) -> io::Result<()> {
        self.confirm()?;
        let raw = i32::try_from(child.id()).map_err(io::Error::other)?;
        let pid = rustix::process::Pid::from_raw(raw)
            .ok_or_else(|| io::Error::other("the nested leader has no native identity"))?;
        if original != self.session {
            return Err(io::Error::other(
                "the nested native creation session differs from its recipient",
            ));
        }
        match rustix::process::getsid(Some(pid)) {
            Ok(actual) if actual == original => Ok(()),
            Ok(actual) => Err(io::Error::other(format!(
                "the nested producer has session {} instead of original {}",
                actual.as_raw_nonzero(),
                original.as_raw_nonzero()
            ))),
            Err(rustix::io::Errno::SRCH) => {
                let terminal = super::sys::ExitHandle::status(child)?;
                if terminal == status {
                    Ok(())
                } else {
                    Err(io::Error::other(format!(
                        "the owned terminal status changed during session confirmation: {status} to {terminal}"
                    )))
                }
            }
            Err(source) => Err(source.into()),
        }
    }

    fn confirm(&self) -> io::Result<()> {
        if self.parent.wait(Some(std::time::Duration::ZERO))? {
            return Err(io::Error::other("the original native session owner ended"));
        }
        let mut events = [rustix::event::PollFd::new(
            &self.lease,
            rustix::event::PollFlags::IN,
        )];
        let timeout = rustix::event::Timespec::try_from(std::time::Duration::ZERO)
            .map_err(io::Error::other)?;
        loop {
            match rustix::event::poll(&mut events, Some(&timeout)) {
                Ok(_ready) => break,
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        }
        if !events[0].revents().is_empty() {
            return Err(io::Error::other(
                "the original custody lease ended or refused",
            ));
        }
        Ok(())
    }
}

/// The original output endpoint retained before its owned reader starts.
#[derive(Debug)]
pub struct OutputEndpoint(std::os::fd::OwnedFd);

impl OutputEndpoint {
    /// Retains the actual configured native pipe without keeping any writer alive.
    ///
    /// # Errors
    /// The operating system refused the original pipe descriptor lease.
    pub fn capture(reader: &io::PipeReader) -> io::Result<Self> {
        reader.as_fd().try_clone_to_owned().map(Self)
    }

    /// Confirms every writer ended within the inherited observation-relative OS backstop.
    ///
    /// # Errors
    /// Native readiness refused instead of proving output completion.
    pub fn closed(&self, deadline: std::time::Instant, attempts: &mut u64) -> io::Result<bool> {
        #[cfg(target_os = "linux")]
        {
            self.hup(Some(deadline), attempts)
        }
        #[cfg(target_os = "macos")]
        {
            use std::os::fd::AsRawFd as _;

            let mut watcher = kqueue::Watcher::new()?;
            watcher.add_fd(
                self.0.as_raw_fd(),
                kqueue::EventFilter::EVFILT_READ,
                kqueue::FilterFlag::empty(),
            )?;
            watcher.watch()?;
            if self.hup(None, attempts)? {
                return Ok(true);
            }
            loop {
                Self::count_attempt(attempts)?;
                match watcher.poll_forever(Some(
                    deadline.saturating_duration_since(std::time::Instant::now()),
                )) {
                    Some(kqueue::Event {
                        ident: kqueue::Ident::Fd(actual),
                        data: kqueue::EventData::ReadReady(_bytes),
                    }) if actual == self.0.as_raw_fd() => {
                        if self.hup(None, attempts)? {
                            return Ok(true);
                        }
                    }
                    Some(kqueue::Event {
                        data: kqueue::EventData::Error(source),
                        ..
                    }) if source.kind() == io::ErrorKind::Interrupted => {}
                    Some(kqueue::Event {
                        data: kqueue::EventData::Error(source),
                        ..
                    }) => return Err(source),
                    Some(event) => {
                        return Err(io::Error::other(format!(
                            "the owned output event did not match: {event:?}"
                        )));
                    }
                    None => return Ok(false),
                }
            }
        }
    }

    fn hup(&self, deadline: Option<std::time::Instant>, attempts: &mut u64) -> io::Result<bool> {
        let mut events = [rustix::event::PollFd::new(&self.0, {
            #[cfg(target_os = "linux")]
            {
                rustix::event::PollFlags::empty()
            }
            #[cfg(target_os = "macos")]
            {
                rustix::event::PollFlags::IN
            }
        })];
        loop {
            let left = match deadline {
                Some(deadline) => deadline.saturating_duration_since(std::time::Instant::now()),
                None => std::time::Duration::ZERO,
            };
            let timeout = rustix::event::Timespec::try_from(left).map_err(io::Error::other)?;
            Self::count_attempt(attempts)?;
            match rustix::event::poll(&mut events, Some(&timeout)) {
                Ok(_ready) => break,
                Err(rustix::io::Errno::INTR) => {}
                Err(source) => return Err(source.into()),
            }
        }
        if events[0]
            .revents()
            .intersects(rustix::event::PollFlags::ERR | rustix::event::PollFlags::NVAL)
        {
            return Err(io::Error::other(
                "the retained original output endpoint refused",
            ));
        }
        Ok(events[0].revents().contains(rustix::event::PollFlags::HUP))
    }

    fn count_attempt(attempts: &mut u64) -> io::Result<()> {
        *attempts = attempts.checked_add(1).ok_or_else(|| {
            io::Error::other("the native output observation call count overflowed")
        })?;
        Ok(())
    }
}
