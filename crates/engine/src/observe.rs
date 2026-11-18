//! Diagnostics events: one per provider request, per engine call and per
//! answer served from the cache (DESIGN "Diagnostics events", the
//! contract: an embedder's summariser parses them).
//!
//! - **`request`** (target `aspero::net`): a provider request as it
//!   completes, emitted by [`Paced`](crate::transport::Paced) (live) and
//!   [`TapeClient`](crate::transport::TapeClient) (replayed). What it was
//!   for (`op`) and which attempt comes from the [`Engine::call`] it's
//!   part of, through a task-local.
//! - **`request`** with `cache=hit` or `cache=stale` (target
//!   `aspero::cache`): answers served from the cache, which never reach
//!   the transport ([`served`]).
//! - **`call`** (target `aspero::engine`): an engine call as it completes
//!   or is dropped ([`Engine::observed`]); calls made within one aren't
//!   counted again.
//!
//! Every engine call has an id, from one counter for the process, which
//! its `call` event and everything done for it carry as `call` (last):
//! its requests, cache answers, retries and debug companions. A task-local
//! holds the call in progress, so its parts (lookups within a verify) and
//! a paid request finished detached ([`carried`]) carry its id. A caller
//! can choose the id beforehand ([`CallId`]). A `verify_batch` of more
//! than one claim is a call only as an id: its claims' calls and its
//! prefetch take ids of their own and say it as `parent` ([`batch`]).
//!
//! Info-level fields are counts, codes, kinds and times, never what was
//! asked: queries, titles, identifiers and URLs go only in `debug`
//! companions (`request_url`, `call_input`). Field values are computed
//! inside the macros, so with no subscriber listening an event costs a
//! callsite check.

use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use aspero_core::ProviderId;
use tokio::time::Instant;

use crate::{Engine, lock};

/// What a provider is asked: the `op` of a `request` event.
pub(crate) mod op {
    pub const LOOKUP: &str = "lookup";
    pub const SEARCH: &str = "search";
    /// A page of the works citing others (a `cites:` search).
    pub const CITING: &str = "citing";
    /// A work's references, from the cache (looked up live as `lookup`).
    pub const REFERENCES: &str = "references";
    /// The free budget check (OpenAlex's `/rate-limit`).
    pub const BUDGET: &str = "budget";
    /// A request made outside an engine call (a key check).
    pub const OTHER: &str = "other";
}

tokio::task_local! {
    /// The provider call the requests polled in this task are part of.
    static REQUEST: Arc<RequestScope>;
    /// The engine call this task works for.
    static CALL: Arc<CallScope>;
    /// The id a caller gave the engine calls it makes ([`CallId::scope`]).
    static GIVEN: Arc<Given>;
}

/// The next engine call's id: one counter for the process, from 1.
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

struct Given {
    id: u64,
    taken: AtomicBool,
}

/// An engine call's id, chosen before the call is made, so a caller can
/// log it beside the call's diagnostics events (DESIGN "Diagnostics
/// events"): from the counter every engine call's id comes from, so
/// unique within the process.
///
/// Engine calls made in [`scope`](Self::scope), and not within another
/// engine call, take it as their `call`; a `verify_batch` of more than
/// one claim takes it as the `parent` of its claims' calls. Calls made
/// outside any scope take ids of their own.
#[derive(Clone)]
pub struct CallId(Arc<Given>);

impl CallId {
    /// A new id.
    pub fn new() -> Self {
        CallId(Arc::new(Given {
            id: next_id(),
            taken: AtomicBool::new(false),
        }))
    }

    /// The id, as the events say it.
    pub fn get(&self) -> u64 {
        self.0.id
    }

    /// Runs `work` with this id given to the engine calls it makes. Every
    /// such call takes it: give each call its own to tell them apart.
    pub fn scope<F: Future>(&self, work: F) -> impl Future<Output = F::Output> + use<F> {
        GIVEN.scope(self.0.clone(), work)
    }

    /// Whether an engine call (or batch) made in its scope has taken it.
    pub fn taken(&self) -> bool {
        self.0.taken.load(Ordering::Relaxed)
    }
}

impl Default for CallId {
    fn default() -> Self {
        CallId::new()
    }
}

impl fmt::Debug for CallId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "CallId({})", self.0.id)
    }
}

/// The id a caller gave the calls made here, now taken; else a new one.
fn given_or_next() -> u64 {
    GIVEN
        .try_with(|g| {
            g.taken.store(true, Ordering::Relaxed);
            g.id
        })
        .unwrap_or_else(|_| next_id())
}

/// The id of the engine call this task works for, if any: the `call` of
/// its events.
pub(crate) fn call_id() -> Option<u64> {
    CALL.try_with(|c| c.id).ok()
}

/// A batch's scope, for a `verify_batch` of more than one claim made
/// outside any engine call: an id (the caller's, if it gave one) its
/// claims' calls name as their `parent`, and no event. Within an engine
/// call (a check), the batch is that call's part: `None`.
pub(crate) fn batch() -> Option<Arc<CallScope>> {
    if CALL.try_with(|_| ()).is_ok() {
        return None;
    }
    Some(Arc::new(CallScope {
        id: given_or_next(),
        batch: true,
        asked: std::sync::Mutex::default(),
    }))
}

/// Runs `f` (a poll of a batch's work) within the batch's scope, if any.
pub(crate) fn within<R>(scope: Option<Arc<CallScope>>, f: impl FnOnce() -> R) -> R {
    match scope {
        Some(scope) => CALL.sync_scope(scope, f),
        None => f(),
    }
}

/// A provider call ([`Engine::call`]) in progress: its op and attempt.
pub(crate) struct RequestScope {
    provider: ProviderId,
    op: &'static str,
    attempt: AtomicU32,
}

/// What a request is for: its provider call's op and attempt (from 1), or
/// [`op::OTHER`] and 0 outside one.
pub(crate) fn request() -> (&'static str, u32) {
    REQUEST
        .try_with(|r| (r.op, r.attempt.load(Ordering::Relaxed)))
        .unwrap_or((op::OTHER, 0))
}

/// The provider and op of the provider call in progress, if any: for the
/// retry event.
pub(crate) fn retrying() -> Option<(ProviderId, &'static str)> {
    REQUEST.try_with(|r| (r.provider.clone(), r.op)).ok()
}

/// Runs `call` (a provider call with retries) as `op` at `provider`;
/// `next` is called before each attempt. Notes the provider as asked by
/// the engine call in progress.
pub(crate) async fn requesting<T>(
    provider: &ProviderId,
    op: &'static str,
    call: impl FnOnce(Arc<RequestScope>) -> T,
) -> T::Output
where
    T: Future,
{
    let _ = CALL.try_with(|c| {
        let mut asked = lock(&c.asked);
        if !asked.contains(provider) {
            asked.push(provider.clone());
        }
    });
    let scope = Arc::new(RequestScope {
        provider: provider.clone(),
        op,
        attempt: AtomicU32::new(0),
    });
    REQUEST.scope(scope.clone(), call(scope)).await
}

impl RequestScope {
    /// Counts an attempt starting.
    pub(crate) fn next(&self) {
        self.attempt.fetch_add(1, Ordering::Relaxed);
    }
}

/// Runs `work` in a spawned task with the engine call in progress here, so
/// its providers are still counted and its requests carry the call's id (a
/// detached paid call).
pub(crate) fn carried<F: Future>(work: F) -> impl Future<Output = F::Output> {
    let call = CALL.try_with(Arc::clone).ok();
    async move {
        match call {
            Some(call) => CALL.scope(call, work).await,
            None => work.await,
        }
    }
}

/// Answers served from the cache: a `request` event (target
/// `aspero::cache`) for `items` of them at `provider` for `op`, `cache`
/// `hit` (fresh) or `stale` (the provider failed, `status` its error kind,
/// or `offline`).
pub(crate) fn served(
    provider: &ProviderId,
    op: &'static str,
    cache: &'static str,
    status: &'static str,
    items: usize,
) {
    if items > 0 {
        tracing::info!(
            target: "aspero::cache",
            provider = %provider, op = %op, status = %status, ms = 0u64, waited_ms = 0u64,
            attempt = 0u32, cache = %cache, items, call = call_id(), "request"
        );
    }
}

/// Milliseconds from `since` to `until`.
pub(crate) fn ms(since: Instant, until: Instant) -> u64 {
    u64::try_from(until.duration_since(since).as_millis()).unwrap_or(u64::MAX)
}

/// A cost in US dollars, as event fields give it: six decimals.
pub(crate) struct Usd(pub f64);

impl fmt::Display for Usd {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:.6}", self.0)
    }
}

/// An engine call in progress: its id and the providers it has asked. Or
/// a batch ([`batch`]): an id only, its claims' calls' parent.
pub(crate) struct CallScope {
    id: u64,
    batch: bool,
    asked: std::sync::Mutex<Vec<ProviderId>>,
}

/// Providers as a field: comma-separated, `none` for none.
struct Asked<'a>(&'a [ProviderId]);

impl fmt::Display for Asked<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("none");
        }
        for (i, p) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            f.write_str(p.as_str())?;
        }
        Ok(())
    }
}

/// Emits the `call` event when dropped: `cancelled` unless finished.
struct CallEvent {
    op: &'static str,
    items: usize,
    patience: &'static str,
    fresh: bool,
    scope: Arc<CallScope>,
    started: Instant,
    outcome: &'static str,
    parent: Option<u64>,
}

impl Drop for CallEvent {
    fn drop(&mut self) {
        let asked = lock(&self.scope.asked);
        tracing::info!(
            target: "aspero::engine",
            op = %self.op, items = self.items, patience = %self.patience, fresh = self.fresh,
            providers = %Asked(&asked), ms = ms(self.started, Instant::now()),
            outcome = %self.outcome, call = self.scope.id, parent = self.parent, "call"
        );
    }
}

impl Engine {
    /// Runs `work`, the engine call `op` on `items` inputs, emitting its
    /// `call` event as it completes (`outcome` says how, from its result:
    /// `None` for `ok`, else the error kind) or `cancelled` if it's
    /// dropped first. `input` describes what was asked, for the `debug`
    /// companion only. Within another engine call, it's that call's part:
    /// no event, and its requests carry that call's id. Within a batch,
    /// it's a call of its own, the batch its `parent`.
    pub(crate) async fn observed<T, F: Future<Output = T>>(
        &self,
        op: &'static str,
        items: usize,
        input: impl FnOnce() -> String,
        work: Pin<Box<F>>,
        outcome: impl FnOnce(&T) -> Option<&'static str>,
    ) -> T {
        let (id, parent) = match CALL.try_with(Arc::clone).ok() {
            Some(outer) if !outer.batch => return work.await,
            Some(batch) => (next_id(), Some(batch.id)),
            None => (given_or_next(), None),
        };
        tracing::debug!(
            target: "aspero::engine", op = %op, input = %input(), call = id, "call_input"
        );
        let scope = Arc::new(CallScope {
            id,
            batch: false,
            asked: std::sync::Mutex::default(),
        });
        let mut event = CallEvent {
            op,
            items,
            patience: self.options.patience(),
            fresh: self.options.fresh,
            scope: scope.clone(),
            started: Instant::now(),
            outcome: "cancelled",
            parent,
        };
        let result = CALL.scope(scope, work).await;
        event.outcome = outcome(&result).unwrap_or("ok");
        drop(event);
        result
    }
}

/// Why a request still in flight was dropped: the `why` of its
/// `status=cancelled` event. A drop the engine doesn't make is its caller's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Why {
    /// Its attempt's deadline passed ([`with_retries`](crate::retry::with_retries)).
    Deadline,
    /// The call no longer needed it: a verify found its verdict.
    Done,
}

impl Why {
    fn as_str(self) -> &'static str {
        match self {
            Why::Deadline => "deadline",
            Why::Done => "done",
        }
    }
}

std::thread_local! {
    /// Why the engine is dropping what it drops right now ([`drop_for`]).
    static DROPPING: std::cell::Cell<Option<Why>> = const { std::cell::Cell::new(None) };
}

/// Drops `work`, whose requests still in flight say `why` they were
/// cancelled. A drop is synchronous, so a thread-local is exact: no other
/// task runs on this thread until it's done.
pub(crate) fn drop_for<T>(why: Why, work: T) {
    let before = DROPPING.with(|d| d.replace(Some(why)));
    drop(work);
    DROPPING.with(|d| d.set(before));
}

/// Why a request is being dropped: as [`drop_for`] says, else its caller
/// stopped waiting (a token cancelled, a client gone, an outer timeout).
pub(crate) fn why_dropped() -> &'static str {
    DROPPING
        .with(std::cell::Cell::get)
        .map_or("caller", Why::as_str)
}

/// Every call's outcome is `ok`: reports that never fail as a whole.
pub(crate) fn never_fails<T>(_: &T) -> Option<&'static str> {
    None
}

/// A `Result`'s outcome: its error's kind.
pub(crate) fn kind_of<T>(result: &Result<T, crate::EngineError>) -> Option<&'static str> {
    result.as_ref().err().map(|e| e.kind().as_str())
}
