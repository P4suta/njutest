// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Test threads whose handles cannot be detached by an early return.

/// Why a test thread could not hand its answer back to its owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JoinError {
    /// The worker panicked before answering.
    #[error("the supervised test thread panicked")]
    Panicked,
    /// An earlier terminal operation already consumed the join handle.
    #[error("the supervised test thread had already been joined")]
    AlreadyJoined,
}

/// An unscoped thread that is joined even when its caller returns early.
#[derive(Debug)]
pub struct JoinedThread<T> {
    owner: ThreadOwner<T>,
}

impl<T: Send + 'static> JoinedThread<T> {
    /// Starts `work` behind a handle-owning boundary.
    pub fn launch(work: impl FnOnce() -> T + Send + 'static) -> Self {
        Self {
            owner: ThreadOwner::launch(work),
        }
    }

    /// Joins the worker and returns its answer.
    ///
    /// # Errors
    /// Returns [`JoinError::Panicked`] when the worker panicked.
    pub fn join(mut self) -> Result<T, JoinError> {
        self.owner.join()
    }
}

/// A scoped thread. The scope itself is the early-return cleanup proof; this
/// value additionally makes the normal-path join and panic branch explicit.
#[derive(Debug)]
pub struct ScopedThread<'scope, T> {
    owner: ScopedThreadOwner<'scope, T>,
}

impl<'scope, T: Send + 'scope> ScopedThread<'scope, T> {
    /// Starts `work` in `scope` and owns its join handle.
    pub fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        work: impl FnOnce() -> T + Send + 'scope,
    ) -> Self {
        Self {
            owner: ScopedThreadOwner::launch(scope, work),
        }
    }

    /// Joins the worker and returns its answer.
    ///
    /// # Errors
    /// Returns [`JoinError::Panicked`] when the worker panicked.
    pub fn join(mut self) -> Result<T, JoinError> {
        self.owner.join()
    }
}

#[derive(Debug)]
struct ThreadOwner<T> {
    handle: Option<std::thread::JoinHandle<T>>,
}

impl<T: Send + 'static> ThreadOwner<T> {
    fn launch(work: impl FnOnce() -> T + Send + 'static) -> Self {
        let handle = std::thread::spawn(work);
        Self {
            handle: Some(handle),
        }
    }

    fn join(&mut self) -> Result<T, JoinError> {
        let handle = self.handle.take().ok_or(JoinError::AlreadyJoined)?;
        match handle.join() {
            Ok(answer) => Ok(answer),
            Err(panic) => {
                drop(panic);
                Err(JoinError::Panicked)
            }
        }
    }
}

impl<T> Drop for ThreadOwner<T> {
    fn drop(&mut self) {
        let Some(handle) = self.handle.take() else {
            return;
        };
        if let Err(panic) = handle.join() {
            drop(panic);
            std::process::abort();
        }
    }
}

#[derive(Debug)]
struct ScopedThreadOwner<'scope, T> {
    handle: Option<std::thread::ScopedJoinHandle<'scope, T>>,
}

impl<'scope, T: Send + 'scope> ScopedThreadOwner<'scope, T> {
    fn launch(
        scope: &'scope std::thread::Scope<'scope, '_>,
        work: impl FnOnce() -> T + Send + 'scope,
    ) -> Self {
        let handle = scope.spawn(work);
        Self {
            handle: Some(handle),
        }
    }

    fn join(&mut self) -> Result<T, JoinError> {
        let handle = self.handle.take().ok_or(JoinError::AlreadyJoined)?;
        match handle.join() {
            Ok(answer) => Ok(answer),
            Err(panic) => {
                drop(panic);
                Err(JoinError::Panicked)
            }
        }
    }
}
