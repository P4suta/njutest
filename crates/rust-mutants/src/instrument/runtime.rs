// SPDX-FileCopyrightText: 2026 njutest contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! The module appended to every instrumented file, which decides at run time which mutant is live.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use super::Placement;

/// Selects the active mutant by its full identity.
pub const ACTIVE_ENV: &str = "RUST_MUTANTS_ACTIVE";

/// Names the catalog the activating run holds.
pub const CATALOG_ENV: &str = "RUST_MUTANTS_CATALOG";

/// The catalog identity embedded into binaries compiled from an instrumented tree.
pub const COMPILED_CATALOG_ENV: &str = "RUST_MUTANTS_COMPILED_CATALOG";

/// The exit status of a test process whose tree was built from a different catalog than the one activating it.
pub const STALE_CATALOG_EXIT: i32 = 97;

/// Names the number of times the active mutant's guard may be taken before the process is stopped.
///
/// The per-process allowance for takes of the selected mutant's guard.
/// It is an execution bound, not a proof that the program would not terminate.
/// Unset, or `0`, spends nothing and counts nothing.
pub const STEPS_ENV: &str = "RUST_MUTANTS_STEPS";

/// Names the fresh file through which the runtime reports that the step allowance was reached.
pub const STEP_NOTICE_ENV: &str = "RUST_MUTANTS_STEP_NOTICE";

/// Correlates one step notice with exactly one supervised execution.
pub const STEP_NONCE_ENV: &str = "RUST_MUTANTS_STEP_NONCE";

/// Names the fresh execution-private state shared by every generated module.
pub const STEP_STATE_ENV: &str = "RUST_MUTANTS_STEP_STATE";

/// The first field of the execution-private step state.
pub const STEP_STATE_SCHEMA: &str = "rust-mutants-step-state-v1";

/// The first field of every complete step notice.
pub const STEP_NOTICE_SCHEMA: &str = "rust-mutants-step-notice-v1";

/// The exit status of a bounded test process whose generated runtime could not publish its nonce-correlated step notice.
pub const STEP_PROTOCOL_EXIT: i32 = 94;

/// Names the file the guards append to, saying which of the process's threads reached them.
pub const TOUCH_ENV: &str = "RUST_MUTANTS_TOUCH";

/// Set to `1` beside [`TOUCH_ENV`], asks a process to record only the items it entered, which is all a mutant execution's union needs and a fraction of what a baseline writes.
pub const TOUCH_ITEMS_ENV: &str = "RUST_MUTANTS_TOUCH_ITEMS";

/// The variable every process a run starts carries, holding the directory its instrumented tree was built to report to; a process of that tree that does not carry it was started by a test that cleared what the run gave it.
pub const WATCHED_ENV: &str = "RUST_MUTANTS_WATCHED";

/// The prefix of the file a process that lost the run's environment leaves in the watched directory, followed by its own id and its parent's.
pub const ORPHAN_PREFIX: &str = "orphan-";

/// The trait the runtime names the types a probe may compare a value of.
pub(super) const OBSERVABLE: &str = "Observable";

/// The exit status of a test process asked to record what its guards saw that could not.
pub const TOUCH_UNAVAILABLE_EXIT: i32 = 96;

/// The name the generated module takes when the file does not already spell it; otherwise a digit is appended until one is free.
pub const MODULE_STEM: &str = "__rm";

/// Marks the generated module, for a person reading the snapshot and for the drift gate.
pub const RUNTIME_MARKER: &str = "rust-mutants-runtime-v1";

/// The first catalog index whose inclusive zero-based window cannot fit in a `u32` span.
pub(super) const FIRST_UNREPRESENTABLE_INDEX: u32 = u32::MAX;

/// Why an instrumented file could not receive a collision-free runtime module name.
#[derive(Debug, thiserror::Error)]
pub enum ModuleNameError {
    /// The source was not a Rust token stream.
    #[error("the source is not a Rust token stream: {0}")]
    Tokens(#[from] proc_macro2::LexError),
    /// The finite suffix namespace could not be searched without overflowing its representation.
    #[error("the generated runtime module suffix namespace is exhausted")]
    SuffixesExhausted,
}

/// Why the generated runtime could not represent its catalog-index window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeRenderError {
    /// The inclusive `u32` catalog window could not be represented without wraparound.
    #[error("the generated runtime's inclusive catalog-index window overflowed u32")]
    IndexWindowOverflow,
}

/// The name the generated module can take in `text`: [`MODULE_STEM`] with `path`'s digest appended, or that with the lowest free number after it.
///
/// # Errors
///
/// Returns [`ModuleNameError`] when `text` is not a Rust token stream or the collision suffix namespace cannot be searched without overflow.
pub fn module_name(path: &str, text: &str) -> Result<String, ModuleNameError> {
    module_named(text, &format!("{MODULE_STEM}_{}", short_digest(path)))
}

/// The first eight hex characters of the path's SHA-256, which is what makes one file's runtime module a different item from another's.
fn short_digest(path: &str) -> String {
    use sha2::Digest;
    let full = hex::encode(sha2::Sha256::digest(path.as_bytes()));
    full.chars().take(8).collect()
}

/// [`module_name`] for a module of another stem, so the witness tree can have one of its own without either shadowing the other.
pub(super) fn module_named(text: &str, stem: &str) -> Result<String, ModuleNameError> {
    let tokens = text.parse::<proc_macro2::TokenStream>()?;
    let mut taken = BTreeSet::new();
    collect_identifiers(tokens, &mut taken);
    if !taken.contains(stem) {
        return Ok(stem.to_owned());
    }
    let candidates = taken
        .len()
        .checked_add(1)
        .ok_or(ModuleNameError::SuffixesExhausted)?;
    let limit =
        u32::try_from(candidates).map_err(|_overflow| ModuleNameError::SuffixesExhausted)?;
    for suffix in 1..=limit {
        let candidate = format!("{stem}{suffix}");
        if !taken.contains(&candidate) {
            return Ok(candidate);
        }
    }
    Err(ModuleNameError::SuffixesExhausted)
}

fn collect_identifiers(tokens: proc_macro2::TokenStream, names: &mut BTreeSet<String>) {
    for tree in tokens {
        match tree {
            proc_macro2::TokenTree::Ident(ident) => {
                names.extend(std::iter::once(ident.to_string()));
            }
            proc_macro2::TokenTree::Group(group) => collect_identifiers(group.stream(), names),
            proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {}
        }
    }
}

/// Defines the transition once for both the engine's test/Kani surface and every generated runtime.
/// Adding a state or action makes the compiler reject both consumers until their exhaustive matches account for it.
macro_rules! step_machine {
    ($consumer:ident) => {
        $consumer! {
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            enum StepAction {
                Activate,
                Checkpoint,
            }
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            enum StepPhase {
                Dormant,
                Counting(usize),
                Active(usize),
                Stopping(usize),
            }
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            enum StepAdvance {
                Continue,
                Park,
                Reached { allowed: usize, observed: usize },
            }
            #[derive(Debug, Clone, Copy, PartialEq, Eq)]
            enum StepMachineError {
                Limit,
                Count,
            }
            const fn step_transition(
                phase: StepPhase,
                action: StepAction,
                allowed: usize,
            ) -> Result<(StepPhase, StepAdvance), StepMachineError> {
                if allowed == 0 || allowed == usize::MAX {
                    return Err(StepMachineError::Limit);
                }
                match (phase, action) {
                    (StepPhase::Dormant, StepAction::Activate) => {
                        Ok((StepPhase::Active(1), StepAdvance::Continue))
                    }
                    (StepPhase::Dormant, StepAction::Checkpoint) => {
                        Ok((StepPhase::Dormant, StepAdvance::Continue))
                    }
                    (StepPhase::Counting(seen), StepAction::Checkpoint) => {
                        match seen.checked_add(1) {
                            Some(next) => Ok((StepPhase::Counting(next), StepAdvance::Continue)),
                            None => Err(StepMachineError::Count),
                        }
                    }
                    (StepPhase::Counting(seen), StepAction::Activate) => {
                        Ok((StepPhase::Counting(seen), StepAdvance::Continue))
                    }
                    (StepPhase::Active(spent), StepAction::Activate)
                        if spent > 0 && spent <= allowed =>
                    {
                        Ok((StepPhase::Active(spent), StepAdvance::Continue))
                    }
                    (StepPhase::Active(spent), StepAction::Checkpoint)
                        if spent > 0 && spent < allowed =>
                    {
                        match spent.checked_add(1) {
                            Some(next) => Ok((StepPhase::Active(next), StepAdvance::Continue)),
                            None => Err(StepMachineError::Count),
                        }
                    }
                    (StepPhase::Active(spent), StepAction::Checkpoint) if spent == allowed => {
                        match allowed.checked_add(1) {
                            Some(observed) => Ok((
                                StepPhase::Stopping(observed),
                                StepAdvance::Reached { allowed, observed },
                            )),
                            None => Err(StepMachineError::Count),
                        }
                    }
                    (StepPhase::Stopping(spent), _) => match allowed.checked_add(1) {
                        Some(observed) if spent == observed => {
                            Ok((StepPhase::Stopping(spent), StepAdvance::Park))
                        }
                        Some(_) | None => Err(StepMachineError::Count),
                    },
                    (StepPhase::Active(_), _) => Err(StepMachineError::Count),
                }
            }
        }
    };
}

#[cfg(any(test, kani))]
macro_rules! compile_step_machine {
    ($($tokens:tt)*) => {
        $($tokens)*
    };
}

#[cfg(any(test, kani))]
step_machine!(compile_step_machine);

macro_rules! stringify_step_machine {
    ($($tokens:tt)*) => {
        stringify!($($tokens)*)
    };
}

const STEP_MACHINE_SOURCE: &str = step_machine!(stringify_step_machine);

/// The expression-grouping macro, emitted only into files whose guards call it.
/// A statement-only file has no unused generated macro to excuse.
const VALUE_MACRO: &str = r"    // The invocation is an expression boundary before expansion, while the
    // expansion is exactly the user's expression. That groups generated
    // boolean chains without adding lint-producing parentheses, a temporary
    // scope, a call boundary, or a new generic type-inference boundary.
    macro_rules! value {
        ($value:expr) => { $value };
    }
    pub(crate) use value;

";

/// The invariant text of the runtime, with the per-file parts as placeholders.
/// Written as one literal so a reader sees the generated module exactly as it will appear in the snapshot.
const TEMPLATE: &str = r#"#[doc(hidden)]
{{GENERATED_MODULE_ALLOW}}
mod {{MODULE}} {
    // {{MARKER}} - generated by rust-mutants; DO NOT EDIT.
    extern crate std as __rm_std;
    const CATALOG: &str = "{{CATALOG}}";
    const IDS: &[(&str, u32)] = &[
{{IDS}}    ];
    const TOUCH_BASE: u32 = {{BASE}};
    const TOUCH_SPAN: u32 = {{SPAN}};
    const ITEM_BASE: u32 = {{ITEM_BASE}};
    const ITEM_SPAN: u32 = {{ITEM_SPAN}};
    const WATCHED: &str = {{WATCHED}};
    #[derive(Clone, Copy)]
    enum Selection {
        None,
        Index(u32),
    }
    #[derive(Clone, Copy)]
    struct StepLimit(usize);
    impl StepLimit {
        fn new(value: usize) -> __rm_std::option::Option<Self> {
            if value == 0 || value == usize::MAX {
                __rm_std::option::Option::None
            } else {
                __rm_std::option::Option::Some(Self(value))
            }
        }
        fn value(self) -> usize {
            self.0
        }
        fn first_excess(self) -> usize {
            match self.0.checked_add(1) {
                __rm_std::option::Option::Some(value) => value,
                __rm_std::option::Option::None => protocol_failure(),
            }
        }
    }
    // What the configured allowance is: none, a bound, or a refusal. A
    // standard Result and Option rather than an enum of this module's own,
    // whose unequal variants a project denying variant_size_differences refuses.
    type Budget = __rm_std::result::Result<__rm_std::option::Option<StepLimit>, BudgetError>;
    #[derive(Clone, Copy)]
    enum BudgetError {
        NonUnicode,
        NotANumber,
        NonCanonical,
        UnrepresentableLimit,
    }
{{STEP_MACHINE}}
    enum StepStateError {
        MissingPath,
        MissingNonce,
        MissingMutant,
        Open,
        Metadata,
        NotRegular,
        Lock,
        Seek,
        Read,
        TooLarge,
        NotUtf8,
        NonCanonical,
        Mismatch,
        InvalidPhase,
        InvalidCount,
        Truncate,
        Write,
        Sync,
        Publish,
        Unlock,
        Poisoned,
    }
    #[derive(Clone, Copy)]
    enum TouchMode {
        Off,
        On,
        ItemsOnly,
    }
    enum StepNoticeError {
        MissingPath,
        MissingNonce,
        MissingMutant,
        Create,
        Write,
        Sync,
        Publish,
    }
    enum TouchAccess {
        Applied,
        AlreadyBorrowed,
    }
    // The three names the durable step protocol is addressed by. A process
    // cannot change its own environment under itself, so reading them once is
    // reading them as often as they can differ; doing it per take charged three
    // environment lookups for an answer that was already known.
    struct StepIdentity {
        path: __rm_std::option::Option<__rm_std::string::String>,
        nonce: __rm_std::option::Option<__rm_std::string::String>,
        mutant: __rm_std::option::Option<__rm_std::string::String>,
    }
    static ACTIVE: __rm_std::sync::OnceLock<Selection> = __rm_std::sync::OnceLock::new();
    // Boundaries this copy has already been granted by the shared state and
    // may spend in memory, and what it last learned of the shared phase. A
    // dormant copy asks the file only every DORMANT_POLL boundaries: another
    // copy's activation reaches it within that many, and until then it
    // charges nothing, which a stated bound says rather than hides.
    static LEASED: __rm_std::sync::atomic::AtomicUsize = __rm_std::sync::atomic::AtomicUsize::new(0);
    static POLLED: __rm_std::sync::atomic::AtomicU32 = __rm_std::sync::atomic::AtomicU32::new(0);
    static KNOWN: __rm_std::sync::atomic::AtomicU8 = __rm_std::sync::atomic::AtomicU8::new(KNOWN_UNASKED);
    const KNOWN_UNASKED: u8 = 0;
    const KNOWN_DORMANT: u8 = 1;
    const KNOWN_ACTIVE: u8 = 2;
    const KNOWN_OTHER: u8 = 3;
    const DORMANT_POLL: u32 = 256;
    const LEASE_SHARE: usize = 16;
    const LEASE_MOST: usize = 4096;
    static BUDGET: __rm_std::sync::OnceLock<Budget> = __rm_std::sync::OnceLock::new();
    static TOUCHING: __rm_std::sync::OnceLock<TouchMode> = __rm_std::sync::OnceLock::new();
    static TOUCH_SINK: __rm_std::sync::OnceLock<__rm_std::sync::Mutex<__rm_std::fs::File>> = __rm_std::sync::OnceLock::new();
    static STEP_IDENTITY: __rm_std::sync::OnceLock<StepIdentity> = __rm_std::sync::OnceLock::new();
    static WATCH: __rm_std::sync::OnceLock<()> = __rm_std::sync::OnceLock::new();
    // The step state this runtime copy opened, and the process that opened
    // it: one open and one check per copy and process, where reopening the
    // name at every boundary paid an open and a close per function entry and
    // loop turn. A child made by fork without exec shares the parent's open
    // file description and so its lock, which is why the process is recorded.
    struct BoundStepState {
        pid: u32,
        file: __rm_std::fs::File,
    }
    static STEP_STATE: __rm_std::sync::OnceLock<
        __rm_std::sync::Mutex<__rm_std::option::Option<BoundStepState>>,
    > = __rm_std::sync::OnceLock::new();

{{VALUE_MACRO}}
    #[inline(always)]
    pub(crate) fn active(index: u32) -> bool {
        watched();
        touch(index);
        match *ACTIVE.get_or_init(resolve) {
            Selection::None => false,
            Selection::Index(selected) if selected == index => {
                activate();
                true
            }
            Selection::Index(_) => false,
        }
    }

    /// Activates the process-wide allowance exactly once. Re-evaluating the
    /// selected guard does not spend twice; the boundaries it reaches do.
    fn activate() {
        if KNOWN.load(__rm_std::sync::atomic::Ordering::SeqCst) == KNOWN_ACTIVE {
            return;
        }
        advance(StepAction::Activate);
    }

    /// Charges a function or loop boundary only after the mutation is active.
    #[inline(always)]
    pub(crate) fn checkpoint() {
        let spent_in_memory = LEASED.fetch_update(
            __rm_std::sync::atomic::Ordering::SeqCst,
            __rm_std::sync::atomic::Ordering::SeqCst,
            |left| left.checked_sub(1),
        );
        if spent_in_memory.is_ok() {
            return;
        }
        if KNOWN.load(__rm_std::sync::atomic::Ordering::SeqCst) == KNOWN_DORMANT
            && POLLED.fetch_add(1, __rm_std::sync::atomic::Ordering::SeqCst) % DORMANT_POLL != 0
        {
            return;
        }
        advance(StepAction::Checkpoint);
    }

    /// How many more boundaries one reservation grants when `remaining` are left: a share of them, at least one and at most a bound.
    const fn lease_size(remaining: usize) -> usize {
        let share = match remaining.checked_div(LEASE_SHARE) {
            __rm_std::option::Option::Some(share) => share,
            __rm_std::option::Option::None => 0,
        };
        if share > LEASE_MOST {
            LEASE_MOST
        } else if share == 0 && remaining > 0 {
            1
        } else {
            share
        }
    }

    /// What `phase` says this copy knows.
    const fn known(phase: StepPhase) -> u8 {
        match phase {
            StepPhase::Dormant => KNOWN_DORMANT,
            StepPhase::Active(_) => KNOWN_ACTIVE,
            StepPhase::Counting(_) | StepPhase::Stopping(_) => KNOWN_OTHER,
        }
    }

    fn advance(action: StepAction) {
        let limit = match *BUDGET.get_or_init(configured_budget) {
            __rm_std::result::Result::Ok(__rm_std::option::Option::None) => return,
            __rm_std::result::Result::Ok(__rm_std::option::Option::Some(limit)) => limit,
            __rm_std::result::Result::Err(_) => protocol_failure(),
        };
        let advanced = match update_state(action, limit) {
            __rm_std::result::Result::Ok(advanced) => advanced,
            __rm_std::result::Result::Err(_) => protocol_failure(),
        };
        match advanced {
            StepAdvance::Continue => {}
            StepAdvance::Park => park_forever(),
            StepAdvance::Reached { .. } => park_forever(),
        }
    }

    fn configured_budget() -> Budget {
        let raw = match __rm_std::env::var_os("{{STEPS_ENV}}") {
            __rm_std::option::Option::None => return __rm_std::result::Result::Ok(__rm_std::option::Option::None),
            __rm_std::option::Option::Some(raw) => raw,
        };
        let text = match raw.to_str() {
            __rm_std::option::Option::Some(text) => text,
            __rm_std::option::Option::None => return __rm_std::result::Result::Err(BudgetError::NonUnicode),
        };
        match text.parse::<usize>() {
            __rm_std::result::Result::Ok(0) => __rm_std::result::Result::Ok(__rm_std::option::Option::None),
            __rm_std::result::Result::Ok(allowed) => {
                if __rm_std::string::ToString::to_string(&allowed) != text {
                    return __rm_std::result::Result::Err(BudgetError::NonCanonical);
                }
                match StepLimit::new(allowed) {
                    __rm_std::option::Option::Some(limit) => __rm_std::result::Result::Ok(__rm_std::option::Option::Some(limit)),
                    __rm_std::option::Option::None => {
                        __rm_std::result::Result::Err(BudgetError::UnrepresentableLimit)
                    }
                }
            }
            __rm_std::result::Result::Err(_) => __rm_std::result::Result::Err(BudgetError::NotANumber),
        }
    }

    fn update_state(
        action: StepAction,
        limit: StepLimit,
    ) -> __rm_std::result::Result<StepAdvance, StepStateError> {
        let identity = STEP_IDENTITY.get_or_init(read_step_identity);
        let path = match identity.path.as_deref() {
            __rm_std::option::Option::Some(path) => path,
            __rm_std::option::Option::None => {
                return __rm_std::result::Result::Err(StepStateError::MissingPath);
            }
        };
        let nonce = match identity.nonce.as_deref() {
            __rm_std::option::Option::Some(nonce) => nonce,
            __rm_std::option::Option::None => {
                return __rm_std::result::Result::Err(StepStateError::MissingNonce);
            }
        };
        let mutant = match identity.mutant.as_deref() {
            __rm_std::option::Option::Some(mutant) => mutant,
            __rm_std::option::Option::None => {
                return __rm_std::result::Result::Err(StepStateError::MissingMutant);
            }
        };
        let cell = STEP_STATE.get_or_init(|| __rm_std::sync::Mutex::new(__rm_std::option::Option::None));
        let mut bound = cell.lock().map_err(|_| StepStateError::Poisoned)?;
        let pid = __rm_std::process::id();
        let reopen = match &*bound {
            __rm_std::option::Option::Some(state) => state.pid != pid,
            __rm_std::option::Option::None => true,
        };
        if reopen {
            let opened = open_step_state(path)?;
            let metadata = opened.metadata().map_err(|_| StepStateError::Metadata)?;
            if !metadata.file_type().is_file() {
                return __rm_std::result::Result::Err(StepStateError::NotRegular);
            }
            *bound = __rm_std::option::Option::Some(BoundStepState { pid, file: opened });
        }
        let file = match &mut *bound {
            __rm_std::option::Option::Some(state) => &mut state.file,
            __rm_std::option::Option::None => {
                return __rm_std::result::Result::Err(StepStateError::Open);
            }
        };
        file.lock().map_err(|_| StepStateError::Lock)?;
        let transitioned = (|| {
            let phase = read_step_state(file, nonce, mutant, limit)?;
            let (mut next, advanced) = step_transition(phase, action, limit.value())
                .map_err(|_| StepStateError::InvalidCount)?;
            let mut granted = 0_usize;
            if let (StepAction::Checkpoint, StepAdvance::Continue, StepPhase::Active(spent)) =
                (action, advanced, next)
            {
                let wanted = lease_size(limit.value().saturating_sub(spent));
                while granted < wanted {
                    let (further, reserved) = step_transition(next, action, limit.value())
                        .map_err(|_| StepStateError::InvalidCount)?;
                    match (reserved, further) {
                        (StepAdvance::Continue, StepPhase::Active(_)) => {
                            next = further;
                            granted = granted.saturating_add(1);
                        }
                        _ => break,
                    }
                }
            }
            KNOWN.store(known(next), __rm_std::sync::atomic::Ordering::SeqCst);
            LEASED.store(granted, __rm_std::sync::atomic::Ordering::SeqCst);
            if let StepAdvance::Reached { allowed, observed } = advanced {
                // A persisted Stopping state is a proof that the final notice
                // was published. Publication happens while the shared state
                // lock is held, before Stopping is written. If publication
                // fails or this process dies, the state remains Active at the
                // boundary so another observer fails or retries instead of
                // parking forever behind a notice that never existed.
                publish_step_notice(allowed, observed).map_err(|_| StepStateError::Publish)?;
            }
            if next != phase {
                write_step_state(file, nonce, mutant, limit, next)?;
            }
            __rm_std::result::Result::Ok(advanced)
        })();
        let unlocked = file.unlock().map_err(|_| StepStateError::Unlock);
        match (transitioned, unlocked) {
            (__rm_std::result::Result::Err(error), _) => {
                __rm_std::result::Result::Err(error)
            }
            (__rm_std::result::Result::Ok(_), __rm_std::result::Result::Err(error)) => {
                __rm_std::result::Result::Err(error)
            }
            (__rm_std::result::Result::Ok(advanced), __rm_std::result::Result::Ok(())) => {
                __rm_std::result::Result::Ok(advanced)
            }
        }
    }

    #[cfg(unix)]
    fn open_step_state(
        path: &str,
    ) -> __rm_std::result::Result<__rm_std::fs::File, StepStateError> {
        use __rm_std::os::unix::fs::OpenOptionsExt as _;

        __rm_std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(no_follow_flag())
            .open(path)
            .map_err(|_| StepStateError::Open)
    }

    #[cfg(windows)]
    fn open_step_state(
        path: &str,
    ) -> __rm_std::result::Result<__rm_std::fs::File, StepStateError> {
        use __rm_std::os::windows::fs::OpenOptionsExt as _;

        // FILE_FLAG_OPEN_REPARSE_POINT makes the final component itself the
        // opened object. The regular-file check below then rejects links and
        // junctions instead of following them.
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        __rm_std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)
            .map_err(|_| StepStateError::Open)
    }

    #[cfg(not(any(unix, windows)))]
    compile_error!("the step-state protocol has no no-follow open for this platform");

    #[cfg(all(unix, any(
        target_os = "macos",
        target_os = "ios",
        target_os = "tvos",
        target_os = "watchos",
        target_os = "visionos",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly",
    )))]
    const fn no_follow_flag() -> i32 {
        0x100
    }

    #[cfg(all(
        unix,
        target_os = "linux",
        any(
            target_arch = "arm",
            target_arch = "aarch64",
            target_arch = "powerpc",
            target_arch = "powerpc64",
            target_arch = "m68k",
        )
    ))]
    const fn no_follow_flag() -> i32 {
        0x8000
    }

    #[cfg(all(
        unix,
        target_os = "linux",
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "mips",
            target_arch = "mips32r6",
            target_arch = "mips64",
            target_arch = "mips64r6",
            target_arch = "riscv32",
            target_arch = "riscv64",
            target_arch = "s390x",
            target_arch = "sparc",
            target_arch = "sparc64",
            target_arch = "csky",
            target_arch = "hexagon",
        )
    ))]
    const fn no_follow_flag() -> i32 {
        0x2_0000
    }

    #[cfg(all(
        unix,
        target_os = "linux",
        target_arch = "loongarch64",
        target_env = "gnu",
    ))]
    const fn no_follow_flag() -> i32 {
        0x40_0000
    }

    #[cfg(all(
        unix,
        target_os = "linux",
        target_arch = "loongarch64",
        target_env = "musl",
    ))]
    const fn no_follow_flag() -> i32 {
        0x2_0000
    }

    #[cfg(all(
        unix,
        target_os = "android",
        any(target_arch = "arm", target_arch = "aarch64")
    ))]
    const fn no_follow_flag() -> i32 {
        0x8000
    }

    #[cfg(all(
        unix,
        target_os = "android",
        any(target_arch = "x86", target_arch = "x86_64")
    ))]
    const fn no_follow_flag() -> i32 {
        0x2_0000
    }

    #[cfg(all(unix, target_os = "android", target_arch = "riscv64"))]
    const fn no_follow_flag() -> i32 {
        0x40_0000
    }

    #[cfg(all(unix, any(target_os = "solaris", target_os = "illumos")))]
    const fn no_follow_flag() -> i32 {
        0x2_0000
    }

    #[cfg(all(unix, target_os = "aix"))]
    const fn no_follow_flag() -> i32 {
        0x100_0000
    }

    #[cfg(all(unix, target_os = "haiku"))]
    const fn no_follow_flag() -> i32 {
        0x8_0000
    }

    #[cfg(all(unix, target_os = "hurd"))]
    const fn no_follow_flag() -> i32 {
        0x10_0000
    }

    #[cfg(all(unix, target_os = "nto"))]
    const fn no_follow_flag() -> i32 {
        0o10_000
    }

    #[cfg(all(unix, target_os = "fuchsia"))]
    const fn no_follow_flag() -> i32 {
        0x80
    }

    #[cfg(all(unix, target_os = "emscripten"))]
    const fn no_follow_flag() -> i32 {
        0x2_0000
    }

    fn read_step_state(
        file: &mut __rm_std::fs::File,
        expected_nonce: &str,
        expected_mutant: &str,
        expected_limit: StepLimit,
    ) -> __rm_std::result::Result<StepPhase, StepStateError> {
        const MAX_STATE_BYTES: u64 = 1024;
        if __rm_std::io::Seek::seek(file, __rm_std::io::SeekFrom::Start(0))
            .map_err(|_| StepStateError::Seek)?
            != 0
        {
            return __rm_std::result::Result::Err(StepStateError::Seek);
        }
        let mut bytes = __rm_std::vec::Vec::new();
        let mut capped = __rm_std::io::Read::take(&mut *file, MAX_STATE_BYTES + 1);
        let read = __rm_std::io::Read::read_to_end(&mut capped, &mut bytes)
            .map_err(|_| StepStateError::Read)?;
        if read > MAX_STATE_BYTES as usize {
            return __rm_std::result::Result::Err(StepStateError::TooLarge);
        }
        let text = __rm_std::str::from_utf8(&bytes).map_err(|_| StepStateError::NotUtf8)?;
        let line = text
            .strip_suffix('\n')
            .ok_or(StepStateError::NonCanonical)?;
        if line.bytes().any(|byte| byte == b'\r' || byte == b'\n') {
            return __rm_std::result::Result::Err(StepStateError::NonCanonical);
        }
        let mut fields = line.split('\t');
        let schema = fields.next();
        let nonce = fields.next();
        let catalog = fields.next();
        let mutant = fields.next();
        let limit_field = fields.next();
        let phase_field = fields.next();
        let spent_field = fields.next();
        if fields.next().is_some()
            || schema != __rm_std::option::Option::Some("{{STEP_STATE_SCHEMA}}")
            || nonce != __rm_std::option::Option::Some(expected_nonce)
            || catalog != __rm_std::option::Option::Some(CATALOG)
            || mutant != __rm_std::option::Option::Some(expected_mutant)
        {
            return __rm_std::result::Result::Err(StepStateError::Mismatch);
        }
        let limit_field = limit_field.ok_or(StepStateError::InvalidCount)?;
        let limit = limit_field
            .parse::<usize>()
            .map_err(|_| StepStateError::InvalidCount)?;
        let spent_field = spent_field.ok_or(StepStateError::InvalidCount)?;
        let spent = spent_field
            .parse::<usize>()
            .map_err(|_| StepStateError::InvalidCount)?;
        if __rm_std::string::ToString::to_string(&limit) != limit_field
            || __rm_std::string::ToString::to_string(&spent) != spent_field
            || limit != expected_limit.value()
        {
            return __rm_std::result::Result::Err(StepStateError::InvalidCount);
        }
        let phase = match phase_field {
            __rm_std::option::Option::Some("dormant") if spent == 0 => StepPhase::Dormant,
            __rm_std::option::Option::Some("counting") => StepPhase::Counting(spent),
            __rm_std::option::Option::Some("active")
                if spent > 0 && spent <= expected_limit.value() => StepPhase::Active(spent),
            __rm_std::option::Option::Some("stopping")
                if spent == expected_limit.first_excess() => StepPhase::Stopping(spent),
            __rm_std::option::Option::Some(_) | __rm_std::option::Option::None => {
                return __rm_std::result::Result::Err(StepStateError::InvalidPhase);
            }
        };
        __rm_std::result::Result::Ok(phase)
    }

    fn read_step_identity() -> StepIdentity {
        StepIdentity {
            path: __rm_std::result::Result::ok(__rm_std::env::var("{{STEP_STATE_ENV}}")),
            nonce: __rm_std::result::Result::ok(__rm_std::env::var("{{STEP_NONCE_ENV}}")),
            mutant: __rm_std::result::Result::ok(__rm_std::env::var("{{ACTIVE_ENV}}")),
        }
    }

    fn write_step_state(
        file: &mut __rm_std::fs::File,
        nonce: &str,
        mutant: &str,
        limit: StepLimit,
        phase: StepPhase,
    ) -> __rm_std::result::Result<(), StepStateError> {
        // A persisted Stopping state is the proof that the notice was published,
        // so that write is the one that has to survive losing the machine. A
        // count on its way up is progress: losing it costs a process its place
        // and costs no verdict its exactness, because the count still passes
        // through this locked file every time and the boundary is still read
        // from it. Paying for durability per take is what made a take cost
        // 7.3ms on Windows, which is a clock the count then had to race.
        let (name, spent, durable) = match phase {
            StepPhase::Dormant => ("dormant", 0, false),
            StepPhase::Counting(seen) => ("counting", seen, false),
            StepPhase::Active(spent) => ("active", spent, false),
            StepPhase::Stopping(spent) => ("stopping", spent, true),
        };
        let state = __rm_std::format!(
            "{{STEP_STATE_SCHEMA}}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            nonce,
            CATALOG,
            mutant,
            limit.value(),
            name,
            spent,
        );
        if __rm_std::io::Seek::seek(file, __rm_std::io::SeekFrom::Start(0))
            .map_err(|_| StepStateError::Seek)?
            != 0
        {
            return __rm_std::result::Result::Err(StepStateError::Seek);
        }
        file.set_len(0).map_err(|_| StepStateError::Truncate)?;
        __rm_std::io::Write::write_all(file, state.as_bytes())
            .map_err(|_| StepStateError::Write)?;
        if durable {
            file.sync_data().map_err(|_| StepStateError::Sync)?;
        }
        __rm_std::result::Result::Ok(())
    }

    fn publish_step_notice(
        allowed: usize,
        observed: usize,
    ) -> __rm_std::result::Result<(), StepNoticeError> {
        let path = __rm_std::env::var("{{STEP_NOTICE_ENV}}")
            .map_err(|_| StepNoticeError::MissingPath)?;
        let nonce = __rm_std::env::var("{{STEP_NONCE_ENV}}")
            .map_err(|_| StepNoticeError::MissingNonce)?;
        let wanted = __rm_std::env::var("{{ACTIVE_ENV}}")
            .map_err(|_| StepNoticeError::MissingMutant)?;
        let partial = __rm_std::format!("{}.partial", path);
        {
            let mut file = __rm_std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&partial)
                .map_err(|_| StepNoticeError::Create)?;
            let notice = __rm_std::format!(
                "{{STEP_NOTICE_SCHEMA}}\t{}\t{}\t{}\t{}\t{}\n",
                nonce, CATALOG, wanted, allowed, observed
            );
            __rm_std::io::Write::write_all(&mut file, notice.as_bytes())
                .map_err(|_| StepNoticeError::Write)?;
            file.sync_data().map_err(|_| StepNoticeError::Sync)?;
        }
        __rm_std::fs::rename(partial, path).map_err(|_| StepNoticeError::Publish)?;
        __rm_std::result::Result::Ok(())
    }

    #[cold]
    fn protocol_failure() -> ! {
        __rm_std::process::exit({{STEP_PROTOCOL_EXIT}})
    }

    fn park_forever() -> ! {
        loop {
            __rm_std::thread::park();
        }
    }

    fn window(span: u32) -> usize {
        match <usize as __rm_std::convert::TryFrom<u32>>::try_from(span) {
            __rm_std::result::Result::Ok(span) => span,
            __rm_std::result::Result::Err(_) => touch_failure(),
        }
    }

    fn offset(index: u32, base: u32) -> usize {
        let relative = match index.checked_sub(base) {
            __rm_std::option::Option::Some(relative) => relative,
            __rm_std::option::Option::None => touch_failure(),
        };
        match <usize as __rm_std::convert::TryFrom<u32>>::try_from(relative) {
            __rm_std::result::Result::Ok(offset) => offset,
            __rm_std::result::Result::Err(_) => touch_failure(),
        }
    }

    fn mark(
        bits: &mut __rm_std::vec::Vec<bool>,
        indices: &mut __rm_std::vec::Vec<u32>,
        at: usize,
        index: u32,
    ) -> bool {
        let slot = match bits.get_mut(at) {
            __rm_std::option::Option::Some(slot) => slot,
            __rm_std::option::Option::None => touch_failure(),
        };
        if *slot {
            return false;
        }
        *slot = true;
        indices.push(index);
        true
    }

    struct Seen {
        name: __rm_std::string::String,
        bits: __rm_std::vec::Vec<bool>,
        touched: __rm_std::vec::Vec<u32>,
        entered_bits: __rm_std::vec::Vec<bool>,
        entered: __rm_std::vec::Vec<u32>,
        differed_bits: __rm_std::vec::Vec<bool>,
        differed: __rm_std::vec::Vec<u32>,
        item_bits: __rm_std::vec::Vec<bool>,
        items: __rm_std::vec::Vec<u32>,
    }

    impl Seen {
        fn new() -> Seen {
            let named = __rm_std::thread::current().name().map(__rm_std::string::ToString::to_string);
            let attributed = match &named {
                __rm_std::option::Option::Some(name) => name != "main",
                __rm_std::option::Option::None => false,
            };
            let span = window(TOUCH_SPAN);
            let mut bits = __rm_std::vec::Vec::new();
            bits.resize(span, false);
            let mut entered_bits = __rm_std::vec::Vec::new();
            entered_bits.resize(span, false);
            let mut differed_bits = __rm_std::vec::Vec::new();
            differed_bits.resize(span, false);
            let mut item_bits = __rm_std::vec::Vec::new();
            item_bits.resize(window(ITEM_SPAN), false);
            Seen {
                name: match named {
                    __rm_std::option::Option::Some(name) if attributed => name,
                    _ => __rm_std::string::String::from("{{UNATTRIBUTED}}"),
                },
                bits,
                touched: __rm_std::vec::Vec::new(),
                entered_bits,
                entered: __rm_std::vec::Vec::new(),
                differed_bits,
                differed: __rm_std::vec::Vec::new(),
                item_bits,
                items: __rm_std::vec::Vec::new(),
            }
        }

        fn saw(&mut self, index: u32) {
            if !mark(&mut self.bits, &mut self.touched, offset(index, TOUCH_BASE), index) {
                return;
            }
            self.flush();
        }

        fn entered_body(&mut self, index: u32) {
            if !mark(&mut self.entered_bits, &mut self.entered, offset(index, TOUCH_BASE), index) {
                return;
            }
            self.flush();
        }

        fn saw_a_difference(&mut self, index: u32) {
            if !mark(&mut self.differed_bits, &mut self.differed, offset(index, TOUCH_BASE), index) {
                return;
            }
            self.flush();
        }

        fn entered_item(&mut self, index: u32) {
            if !mark(&mut self.item_bits, &mut self.items, offset(index, ITEM_BASE), index) {
                return;
            }
            self.flush();
        }

        fn flush(&mut self) {
            written("{{SITES}}", &self.name, &mut self.touched);
            written("{{BODIES}}", &self.name, &mut self.entered);
            written("{{INFECTED}}", &self.name, &mut self.differed);
            written("{{ENTERED}}", &self.name, &mut self.items);
        }
    }

    fn written(kind: &str, name: &str, indices: &mut __rm_std::vec::Vec<u32>) {
        if indices.is_empty() {
            return;
        }
        let mut line = __rm_std::string::String::from(kind);
        line.push_str("\t");
        line.push_str(name);
        let mut separator = "\t";
        for index in indices.iter() {
            line.push_str(separator);
            line.push_str(&__rm_std::format!("{}", index));
            separator = ",";
        }
        line.push_str("\n");
        indices.clear();
        append(&line);
    }

    impl __rm_std::ops::Drop for Seen {
        fn drop(&mut self) {
            self.flush();
        }
    }

    __rm_std::thread_local! {
        static SEEN: __rm_std::cell::RefCell<Seen> = __rm_std::cell::RefCell::new(Seen::new());
    }

    fn with_seen<F>(apply: F)
    where
        F: __rm_std::ops::FnOnce(&mut Seen),
    {
        let accessed = SEEN.try_with(|seen| match seen.try_borrow_mut() {
            __rm_std::result::Result::Ok(mut seen) => {
                apply(&mut seen);
                TouchAccess::Applied
            }
            __rm_std::result::Result::Err(_) => TouchAccess::AlreadyBorrowed,
        });
        match accessed {
            __rm_std::result::Result::Ok(TouchAccess::Applied) => {}
            __rm_std::result::Result::Ok(TouchAccess::AlreadyBorrowed)
            | __rm_std::result::Result::Err(_) => touch_failure(),
        }
    }

    #[inline(never)]
    fn touch(index: u32) {
        if !touching() {
            return;
        }
        with_seen(|seen| seen.saw(index));
    }

    #[inline(always)]
    pub(crate) fn body(index: u32) {
        entered(index);
    }

    #[inline(always)]
    pub(crate) fn differing<F: __rm_std::ops::FnOnce() -> bool>(index: u32, original: bool, alternative: F) -> bool {
        if touching() && original != alternative() {
            difference(index);
        }
        original
    }

{{OBSERVABLE}}

    #[inline(always)]
    pub(crate) fn undefaulted<T: {{PROBE_BOUND}}>(index: u32, value: T) -> T {
        if touching() && value != <T as __rm_std::default::Default>::default() {
            difference(index);
        }
        value
    }

    #[inline(always)]
    pub(crate) fn untrue(index: u32, value: bool) -> bool {
        if touching() && !value {
            difference(index);
        }
        value
    }

    #[inline(always)]
    pub(crate) fn unokdefault<T: {{PROBE_BOUND}}, E>(index: u32, value: __rm_std::result::Result<T, E>) -> __rm_std::result::Result<T, E> {
        if touching() {
            let parted = match &value {
                __rm_std::result::Result::Ok(held) => *held != <T as __rm_std::default::Default>::default(),
                __rm_std::result::Result::Err(_) => true,
            };
            if parted {
                difference(index);
            }
        }
        value
    }

    #[inline(always)]
    pub(crate) fn unsomedefault<T: {{PROBE_BOUND}}>(index: u32, value: __rm_std::option::Option<T>) -> __rm_std::option::Option<T> {
        if touching() {
            let parted = match &value {
                __rm_std::option::Option::Some(held) => *held != <T as __rm_std::default::Default>::default(),
                __rm_std::option::Option::None => true,
            };
            if parted {
                difference(index);
            }
        }
        value
    }

    #[inline(never)]
    fn difference(index: u32) {
        with_seen(|seen| seen.saw_a_difference(index));
    }

    #[inline(always)]
    pub(crate) fn item(index: u32) {
        watched();
        if touching_items() {
            entered_item(index);
        }
    }

    #[inline(never)]
    fn entered_item(index: u32) {
        let recorded = SEEN.try_with(|seen| match seen.try_borrow_mut() {
            __rm_std::result::Result::Ok(mut seen) => {
                seen.entered_item(index);
                TouchAccess::Applied
            }
            __rm_std::result::Result::Err(_) => TouchAccess::AlreadyBorrowed,
        });
        match recorded {
            __rm_std::result::Result::Ok(TouchAccess::Applied) => {}
            __rm_std::result::Result::Ok(TouchAccess::AlreadyBorrowed) => touch_failure(),
            __rm_std::result::Result::Err(_) => {
                let mut indices = __rm_std::vec![index];
                written("{{ENTERED}}", "{{UNATTRIBUTED}}", &mut indices);
            }
        }
    }

    #[inline(never)]
    fn entered(index: u32) {
        if !touching() {
            return;
        }
        with_seen(|seen| seen.entered_body(index));
    }

    // A process of this tree that does not carry the variable naming where
    // it reports was started by a test that cleared what the run gave it: no
    // mutant can be active in it and nothing it enters is recorded. It says
    // so where the run looks, once, and a process that cannot say so stops.
    #[inline(always)]
    fn watched() {
        let () = *WATCH.get_or_init(noticed);
    }

    #[cold]
    fn noticed() {
        let carried = match __rm_std::env::var_os("{{WATCHED_ENV}}") {
            __rm_std::option::Option::Some(value) => value.as_os_str() == __rm_std::ffi::OsStr::new(WATCHED),
            __rm_std::option::Option::None => false,
        };
        if carried {
            return;
        }
        #[cfg(unix)]
        let parent = __rm_std::os::unix::process::parent_id();
        #[cfg(not(unix))]
        let parent = 0_u32;
        let name = __rm_std::format!("{{ORPHAN_PREFIX}}{}-{}", __rm_std::process::id(), parent);
        let path = __rm_std::path::Path::new(WATCHED).join(name);
        if __rm_std::fs::create_dir_all(WATCHED).is_err() || __rm_std::fs::File::create(path).is_err() {
            __rm_std::process::exit({{TOUCH_EXIT}});
        }
    }

    fn touching() -> bool {
        matches!(*TOUCHING.get_or_init(configured_touch), TouchMode::On)
    }

    fn touching_items() -> bool {
        matches!(
            *TOUCHING.get_or_init(configured_touch),
            TouchMode::On | TouchMode::ItemsOnly
        )
    }

    fn configured_touch() -> TouchMode {
        let asked = match __rm_std::env::var("{{TOUCH_ENV}}") {
            __rm_std::result::Result::Ok(value) => !value.is_empty(),
            __rm_std::result::Result::Err(_) => false,
        };
        let ours = match __rm_std::env::var("{{CATALOG_ENV}}") {
            __rm_std::result::Result::Ok(value) => value == CATALOG,
            __rm_std::result::Result::Err(_) => false,
        };
        let items_only = match __rm_std::env::var("{{TOUCH_ITEMS_ENV}}") {
            __rm_std::result::Result::Ok(value) => value == "1",
            __rm_std::result::Result::Err(_) => false,
        };
        match (asked && ours, items_only) {
            (true, true) => TouchMode::ItemsOnly,
            (true, false) => TouchMode::On,
            (false, _) => TouchMode::Off,
        }
    }

    #[cold]
    fn touch_failure() -> ! {
        __rm_std::process::exit({{TOUCH_EXIT}})
    }

    fn append(line: &str) {
        let sink = TOUCH_SINK.get_or_init(opened);
        let mut file = match sink.lock() {
            __rm_std::result::Result::Ok(guard) => guard,
            __rm_std::result::Result::Err(_) => touch_failure(),
        };
        if __rm_std::io::Write::write_all(&mut *file, line.as_bytes()).is_err() {
            __rm_std::process::exit({{TOUCH_EXIT}});
        }
    }

    #[cold]
    fn opened() -> __rm_std::sync::Mutex<__rm_std::fs::File> {
        let path = match __rm_std::env::var("{{TOUCH_ENV}}") {
            __rm_std::result::Result::Ok(value) => value,
            __rm_std::result::Result::Err(_) => __rm_std::process::exit({{TOUCH_EXIT}}),
        };
        let opened = __rm_std::fs::OpenOptions::new().create(true).append(true).open(&path);
        let mut file = match opened {
            __rm_std::result::Result::Ok(file) => file,
            __rm_std::result::Result::Err(_) => __rm_std::process::exit({{TOUCH_EXIT}}),
        };
        let header = __rm_std::format!("{{TOUCH_SCHEMA}} {}\n", CATALOG);
        if __rm_std::io::Write::write_all(&mut file, header.as_bytes()).is_err() {
            __rm_std::process::exit({{TOUCH_EXIT}});
        }
        __rm_std::sync::Mutex::new(file)
    }

    #[cold]
    fn resolve() -> Selection {
        let wanted = match __rm_std::env::var("{{ACTIVE_ENV}}") {
            __rm_std::result::Result::Ok(value) => value,
            __rm_std::result::Result::Err(_) => return Selection::None,
        };
        if wanted.is_empty() {
            return Selection::None;
        }
        let catalog = match __rm_std::env::var("{{CATALOG_ENV}}") {
            __rm_std::result::Result::Ok(value) => value,
            __rm_std::result::Result::Err(_) => stale_catalog("<unset>"),
        };
        if catalog != CATALOG {
            stale_catalog(&catalog);
        }
        for &(id, index) in IDS {
            if id == wanted {
                return Selection::Index(index);
            }
        }
        Selection::None
    }

    #[cold]
    fn stale_catalog(active: &str) -> ! {
        let said = __rm_std::format!(
            "rust-mutants: this binary was built from catalog {} but {} is active\n",
            CATALOG,
            active,
        );
        if __rm_std::io::Write::write_all(&mut __rm_std::io::stderr(), said.as_bytes()).is_err() {
            __rm_std::process::exit({{EXIT}});
        }
        __rm_std::process::exit({{EXIT}})
    }
}
"#;

/// `text` with every name the runtime and the engine agree on filled in: the variables it reads, the records it writes, and the codes it exits with.
fn with_protocol(text: &str) -> String {
    text.replace("{{ACTIVE_ENV}}", ACTIVE_ENV)
        .replace("{{CATALOG_ENV}}", CATALOG_ENV)
        .replace("{{TOUCH_ENV}}", TOUCH_ENV)
        .replace("{{TOUCH_ITEMS_ENV}}", TOUCH_ITEMS_ENV)
        .replace("{{TOUCH_SCHEMA}}", crate::touch::SCHEMA)
        .replace("{{UNATTRIBUTED}}", crate::touch::UNATTRIBUTED)
        .replace("{{SITES}}", crate::touch::SITES)
        .replace("{{BODIES}}", crate::touch::BODIES)
        .replace("{{INFECTED}}", crate::touch::INFECTED)
        .replace("{{ENTERED}}", crate::touch::ENTERED)
        .replace("{{EXIT}}", &STALE_CATALOG_EXIT.to_string())
        .replace("{{TOUCH_EXIT}}", &TOUCH_UNAVAILABLE_EXIT.to_string())
        .replace("{{STEPS_ENV}}", STEPS_ENV)
        .replace("{{STEP_NOTICE_ENV}}", STEP_NOTICE_ENV)
        .replace("{{STEP_NONCE_ENV}}", STEP_NONCE_ENV)
        .replace("{{STEP_STATE_ENV}}", STEP_STATE_ENV)
        .replace("{{STEP_STATE_SCHEMA}}", STEP_STATE_SCHEMA)
        .replace("{{STEP_NOTICE_SCHEMA}}", STEP_NOTICE_SCHEMA)
        .replace("{{STEP_PROTOCOL_EXIT}}", &STEP_PROTOCOL_EXIT.to_string())
        .replace("{{WATCHED_ENV}}", WATCHED_ENV)
        .replace("{{ORPHAN_PREFIX}}", ORPHAN_PREFIX)
}

/// Renders the runtime module for one file.
///
/// # Errors
///
/// Returns [`RuntimeRenderError`] when the inclusive catalog-index window cannot be represented without overflow.
pub fn render(rendering: &Rendering<'_>) -> Result<String, RuntimeRenderError> {
    let Rendering {
        module,
        catalog_digest,
        placements,
        markers,
        first_item,
        item_count,
        newline,
        watched,
    } = *rendering;
    let ids: BTreeSet<(&str, u32)> = placements
        .iter()
        .map(|placement| (placement.id.as_str(), placement.index))
        .collect();
    let mut table = String::new();
    for (id, index) in &ids {
        let written = writeln!(table, "        ({id:?}, {index}),");
        debug_assert!(written.is_ok(), "writing to a String cannot fail");
    }
    let reach = touched(placements, markers)?;
    let value_macro = if placements
        .iter()
        .any(|placement| placement.hint.form != crate::syntax::Form::S)
    {
        VALUE_MACRO
    } else {
        ""
    };

    let text = TEMPLATE
        .replace(
            "{{GENERATED_MODULE_ALLOW}}",
            super::GENERATED_MODULE_ALLOW_ATTRIBUTE,
        )
        .replace("{{VALUE_MACRO}}", value_macro)
        .replace("{{MODULE}}", module)
        .replace("{{MARKER}}", RUNTIME_MARKER)
        .replace("{{CATALOG}}", catalog_digest)
        .replace("{{IDS}}", &table)
        .replace("{{BASE}}", &reach.base.to_string())
        .replace("{{SPAN}}", &reach.span.to_string())
        .replace("{{ITEM_BASE}}", &first_item.to_string())
        .replace("{{ITEM_SPAN}}", &item_count.to_string())
        .replace("{{STEP_MACHINE}}", STEP_MACHINE_SOURCE)
        .replace(
            "{{OBSERVABLE}}",
            &format!(
                "    {}",
                super::observable::declaration(OBSERVABLE, "__rm_std")
            ),
        )
        .replace(
            "{{PROBE_BOUND}}",
            &super::observable::bound(OBSERVABLE, "__rm_std"),
        )
        .replace("{{WATCHED}}", &format!("{watched:?}"));
    let text = with_protocol(&text);
    if newline == "\n" {
        Ok(text)
    } else {
        Ok(text.replace('\n', newline))
    }
}

/// What one file's runtime module is generated from.
#[derive(Debug, Clone, Copy)]
pub struct Rendering<'a> {
    /// The module's name, which carries the file path's digest.
    pub module: &'a str,
    /// The catalog every guard names.
    pub catalog_digest: &'a str,
    /// The mutants placed in the file.
    pub placements: &'a [Placement],
    /// The markers the branch proofs put in it, whose indices the recording also carries.
    pub markers: &'a [crate::syntax::branch::Marker],
    /// The item index of the file's first item, which is where its entry markers start counting.
    pub first_item: u32,
    /// How many items the file holds, which sizes the per-thread record of what it already said it entered.
    pub item_count: u32,
    /// The newline the file uses.
    pub newline: &'a str,
    /// The absolute directory a process of the tree that lost the run's environment says so in.
    pub watched: &'a str,
}

/// The window of catalog indices one file's guards can report, which is what sizes the per-thread record of what it already said.
fn touched(
    placements: &[Placement],
    markers: &[crate::syntax::branch::Marker],
) -> Result<Window, RuntimeRenderError> {
    let every = || {
        placements
            .iter()
            .map(|placement| placement.index)
            .chain(markers.iter().map(|marker| marker.index))
    };
    let Some(lowest) = every().min() else {
        return Ok(Window { base: 0, span: 0 });
    };
    let Some(highest) = every().max() else {
        return Ok(Window { base: 0, span: 0 });
    };
    let distance = highest
        .checked_sub(lowest)
        .ok_or(RuntimeRenderError::IndexWindowOverflow)?;
    let span = distance
        .checked_add(1)
        .ok_or(RuntimeRenderError::IndexWindowOverflow)?;
    Ok(Window { base: lowest, span })
}

/// The catalog indices one file's guards can report.
struct Window {
    /// The lowest of them.
    base: u32,
    /// How many there are from `base` up to and including the highest.
    span: u32,
}

#[cfg(test)]
mod tests {
    use super::{StepAction, StepAdvance, StepMachineError, StepPhase, step_transition};

    #[test]
    fn activation_is_idempotent_and_a_dormant_checkpoint_is_inert() {
        for allowed in 1..=8 {
            assert_eq!(
                step_transition(StepPhase::Dormant, StepAction::Checkpoint, allowed),
                Ok((StepPhase::Dormant, StepAdvance::Continue))
            );
            assert_eq!(
                step_transition(StepPhase::Dormant, StepAction::Activate, allowed),
                Ok((StepPhase::Active(1), StepAdvance::Continue))
            );
            for spent in 1..=allowed {
                assert_eq!(
                    step_transition(StepPhase::Active(spent), StepAction::Activate, allowed),
                    Ok((StepPhase::Active(spent), StepAdvance::Continue))
                );
            }
        }
    }

    #[test]
    fn counting_counts_every_boundary_and_stops_at_none_of_them() {
        for allowed in 1..=8 {
            for seen in 0..12 {
                assert_eq!(
                    step_transition(StepPhase::Counting(seen), StepAction::Checkpoint, allowed),
                    Ok((StepPhase::Counting(seen + 1), StepAdvance::Continue)),
                    "a baseline is measured, not bounded: the allowance is what a mutation is \
                     held to, and holding the original to it would make the number a run \
                     derives depend on the number it started from"
                );
            }
        }
    }

    #[test]
    fn nothing_but_the_state_file_can_put_a_run_into_counting() {
        for allowed in 1..=4 {
            for spent in 0..6 {
                for action in [StepAction::Activate, StepAction::Checkpoint] {
                    for phase in [
                        StepPhase::Dormant,
                        StepPhase::Active(spent.max(1)),
                        StepPhase::Stopping(allowed + 1),
                    ] {
                        assert!(
                            !matches!(
                                step_transition(phase, action, allowed),
                                Ok((StepPhase::Counting(_), _))
                            ),
                            "counting is a mode the engine asks for by writing the initial \
                             state, so no sequence of actions can enter it and a mutation \
                             run cannot become an unbounded one"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_first_boundary_past_the_allowance_is_unique_and_stopping_is_absorbing() {
        for allowed in 1..=8 {
            for spent in 1..allowed {
                assert_eq!(
                    step_transition(StepPhase::Active(spent), StepAction::Checkpoint, allowed),
                    Ok((StepPhase::Active(spent + 1), StepAdvance::Continue))
                );
            }
            let observed = allowed + 1;
            assert_eq!(
                step_transition(StepPhase::Active(allowed), StepAction::Checkpoint, allowed),
                Ok((
                    StepPhase::Stopping(observed),
                    StepAdvance::Reached { allowed, observed }
                ))
            );
            for action in [StepAction::Activate, StepAction::Checkpoint] {
                assert_eq!(
                    step_transition(StepPhase::Stopping(observed), action, allowed),
                    Ok((StepPhase::Stopping(observed), StepAdvance::Park))
                );
            }
        }
        assert_eq!(
            step_transition(StepPhase::Dormant, StepAction::Activate, 0),
            Err(StepMachineError::Limit)
        );
        assert_eq!(
            step_transition(StepPhase::Dormant, StepAction::Activate, usize::MAX),
            Err(StepMachineError::Limit)
        );
    }
}

#[cfg(kani)]
mod kani_laws {
    use super::{StepAction, StepAdvance, StepPhase, step_transition};

    fn valid_limit() -> usize {
        let allowed = kani::any::<usize>();
        kani::assume(allowed > 0 && allowed < usize::MAX);
        allowed
    }

    #[kani::proof]
    fn activation_is_idempotent() {
        let allowed = valid_limit();
        let spent = kani::any::<usize>();
        kani::assume(spent > 0 && spent <= allowed);
        kani::assert(
            step_transition(StepPhase::Active(spent), StepAction::Activate, allowed)
                == Ok((StepPhase::Active(spent), StepAdvance::Continue)),
            "njutest-law-assertion:activation-idempotent",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn a_dormant_checkpoint_cannot_spend() {
        let allowed = valid_limit();
        kani::assert(
            step_transition(StepPhase::Dormant, StepAction::Checkpoint, allowed)
                == Ok((StepPhase::Dormant, StepAdvance::Continue)),
            "njutest-law-assertion:dormant-inert",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn a_counting_checkpoint_counts() {
        let allowed = valid_limit();
        let seen = kani::any::<usize>();
        kani::assume(seen < usize::MAX);
        kani::assert(
            step_transition(StepPhase::Counting(seen), StepAction::Checkpoint, allowed)
                == Ok((StepPhase::Counting(seen + 1), StepAdvance::Continue)),
            "njutest-law-assertion:counting-counts",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn a_counting_checkpoint_never_stops() {
        let allowed = valid_limit();
        let seen = kani::any::<usize>();
        kani::assume(seen < usize::MAX);
        kani::assert(
            !matches!(
                step_transition(StepPhase::Counting(seen), StepAction::Checkpoint, allowed),
                Ok((_, StepAdvance::Park)) | Ok((_, StepAdvance::Reached { .. }))
            ),
            "njutest-law-assertion:counting-never-stops",
        );
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn counting_is_not_reachable_from_dormant_or_active() {
        let allowed = valid_limit();
        let spent = kani::any::<usize>();
        let action = if kani::any::<bool>() {
            StepAction::Activate
        } else {
            StepAction::Checkpoint
        };
        for phase in [StepPhase::Dormant, StepPhase::Active(spent)] {
            kani::assert(
                !matches!(
                    step_transition(phase, action, allowed),
                    Ok((StepPhase::Counting(_), _))
                ),
                "njutest-law-assertion:counting-only-from-the-state-file",
            );
        }
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn an_active_checkpoint_advances_or_reaches_the_exact_boundary() {
        let allowed = valid_limit();
        let spent = kani::any::<usize>();
        kani::assume(spent > 0 && spent <= allowed);
        let result = step_transition(StepPhase::Active(spent), StepAction::Checkpoint, allowed);
        if spent < allowed {
            kani::assert(
                result == Ok((StepPhase::Active(spent + 1), StepAdvance::Continue)),
                "njutest-law-assertion:active-advance",
            );
            kani::cover!(true, "njutest-law-branch:advance");
        } else {
            kani::assert(
                result
                    == Ok((
                        StepPhase::Stopping(allowed + 1),
                        StepAdvance::Reached {
                            allowed,
                            observed: allowed + 1,
                        },
                    )),
                "njutest-law-assertion:active-boundary",
            );
            kani::cover!(true, "njutest-law-branch:boundary");
        }
        kani::cover!(true, "njutest-law-reached");
    }

    #[kani::proof]
    fn stopping_is_absorbing() {
        let allowed = valid_limit();
        let observed = allowed + 1;
        let action = if kani::any::<bool>() {
            StepAction::Activate
        } else {
            StepAction::Checkpoint
        };
        kani::assert(
            step_transition(StepPhase::Stopping(observed), action, allowed)
                == Ok((StepPhase::Stopping(observed), StepAdvance::Park)),
            "njutest-law-assertion:stopping-absorbing",
        );
        kani::cover!(
            matches!(action, StepAction::Activate),
            "njutest-law-branch:activate"
        );
        kani::cover!(
            matches!(action, StepAction::Checkpoint),
            "njutest-law-branch:checkpoint"
        );
        kani::cover!(true, "njutest-law-reached");
    }
}
