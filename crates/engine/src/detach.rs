//! A paid request once sent is finished, whoever still waits for it (#139).
//!
//! OpenAlex charges for a request it has started, answered or not. A caller
//! that stops waiting (a claim verified while its other searches are still
//! out, a cancelled MCP call) would otherwise cancel the request mid-flight:
//! its cost charged but never counted (not in the usage, the credit spent
//! here or the caps), its answer lost to the cache and the tape.
//!
//! So a paid provider's call runs as a task of its own
//! ([`Engine::finish_paid`]), with everything it needs owned. The caller
//! awaits it; dropped, it leaves the task to finish only if one of its
//! requests is already on the wire: [`Paced`](crate::transport::Paced)
//! marks a request sent ([`may_send`]) once its pacing slot has come, just
//! before sending it. A task with nothing sent yet is cancelled with its
//! caller, so an early stop still saves the requests it stops; one left
//! finishing sends nothing more (a retry, the next batch). What the task
//! then does is what an awaited call does: [`Engine::call`] notes the
//! budget (and with it the credit spent here, which the caps count), the
//! answer is cached, and a [`TapeClient`](crate::transport::TapeClient)
//! records the response, as it's below the provider. Its usage, which the
//! caller's report would have carried, is kept for [`Engine::settle`].
//!
//! Free providers' calls are run in place, as before: dropped, they're
//! cancelled, which costs nothing.

use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use aspero_core::{Capabilities, ProviderId, Usage};
use tokio::sync::watch;
use tokio::task::{AbortHandle, JoinHandle};

use crate::{Engine, Entry, add_usage, lock, transport};

tokio::task_local! {
    /// The call a task runs for [`Engine::finish_paid`].
    static GATE: Arc<Gate>;
}

/// What a detached call and its caller share.
#[derive(Default)]
struct Gate {
    state: Mutex<GateState>,
}

#[derive(Default)]
struct GateState {
    /// A request of the call has gone on the wire.
    sent: bool,
    /// The caller stopped waiting.
    abandoned: bool,
    /// Counted as running in the background, for [`Engine::settle`].
    counted: bool,
    abort: Option<AbortHandle>,
}

/// Whether a request may go on the wire now, called just before it's sent:
/// yes, unless its call's caller has stopped waiting, in which case the
/// call's task is cancelled instead. Outside a detached call, always.
pub(crate) fn may_send() -> bool {
    GATE.try_with(|gate| {
        let mut state = lock(&gate.state);
        if state.abandoned {
            if let Some(abort) = &state.abort {
                abort.abort();
            }
            false
        } else {
            state.sent = true;
            true
        }
    })
    .unwrap_or(true)
}

/// Whether the request being made is of a detached call whose caller has
/// stopped waiting: finishing in the background (its `request` event says
/// `detached=true`).
pub(crate) fn detached() -> bool {
    GATE.try_with(|gate| lock(&gate.state).abandoned)
        .unwrap_or(false)
}

/// Whether `caps` say a provider charges: it can say its budget, or some
/// of its calls cost (OpenAlex, with a key or without).
fn charges(caps: &Capabilities) -> bool {
    caps.budget_status || caps.max_free_batch > 0
}

/// Paid calls finishing after their callers stopped waiting: how many are
/// still running, and the usage of those done.
pub(crate) struct Background {
    running: watch::Sender<usize>,
    usage: Mutex<Vec<(ProviderId, Usage)>>,
}

impl Default for Background {
    fn default() -> Self {
        Background {
            running: watch::channel(0).0,
            usage: Mutex::default(),
        }
    }
}

impl Background {
    fn spent(&self, provider: &ProviderId, usage: Option<Usage>) {
        if let Some(usage) = usage {
            add_usage(&mut lock(&self.usage), provider, usage);
        }
    }
}

/// How a detached call's usage is read from its answer.
type UsageOf<T> = fn(&T) -> Option<Usage>;

/// Dropped with the task's future, finished or cancelled: no longer
/// running in the background.
struct Finished {
    gate: Arc<Gate>,
    background: Arc<Background>,
}

impl Drop for Finished {
    fn drop(&mut self) {
        let mut state = lock(&self.gate.state);
        if std::mem::take(&mut state.counted) {
            self.background
                .running
                .send_modify(|n| *n = n.saturating_sub(1));
        }
    }
}

/// The caller's side of a detached call.
struct Waiting<T> {
    gate: Arc<Gate>,
    /// The answer, until the caller takes it.
    slot: Arc<Mutex<Option<T>>>,
    /// Until the task is done and its answer taken.
    task: Option<JoinHandle<()>>,
    background: Arc<Background>,
    provider: ProviderId,
    usage: UsageOf<T>,
}

impl<T> Waiting<T> {
    async fn answer(mut self) -> T {
        let done = self.task.as_mut().expect("awaited once").await;
        // Done: dropped from here on (unwinding), it abandons nothing.
        self.task = None;
        if let Err(e) = done {
            if e.is_panic() {
                std::panic::resume_unwind(e.into_panic());
            }
            // Only a caller that stopped waiting cancels the task.
            unreachable!("a paid call's task was cancelled while awaited: {e}");
        }
        lock(&self.slot)
            .take()
            .expect("a finished call leaves its answer")
    }
}

impl<T> Drop for Waiting<T> {
    fn drop(&mut self) {
        if self.task.is_none() {
            return;
        }
        let mut state = lock(&self.gate.state);
        state.abandoned = true;
        if let Some(answer) = lock(&self.slot).take() {
            // Answered, but not yet read.
            self.background.spent(&self.provider, (self.usage)(&answer));
        } else if !state.sent {
            // Nothing on the wire: nothing to finish.
            if let Some(abort) = &state.abort {
                abort.abort();
            }
        } else {
            state.counted = true;
            self.background.running.send_modify(|n| *n += 1);
        }
    }
}

impl Engine {
    /// Runs `work`, a call to `entry`'s provider and what's done with its
    /// answer (caching it), to the end once a request of it is sent, even
    /// if the caller stops waiting first, when the provider charges (see
    /// the [module](self)). `usage` reads the answer's usage, kept for
    /// [`settle`](Self::settle) when nobody reads the answer. Free
    /// providers' calls, and calls asked only to replay (nothing is sent),
    /// are run in place.
    pub(crate) async fn finish_paid<T, F>(&self, entry: &Entry, work: F, usage: UsageOf<T>) -> T
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        if !charges(entry.provider.capabilities()) || transport::peeking() {
            return work.await;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return work.await;
        };
        let gate = Arc::new(Gate::default());
        let slot = Arc::new(Mutex::new(None));
        let provider = entry.provider.id();
        let task = {
            let finished = Finished {
                gate: gate.clone(),
                background: self.background.clone(),
            };
            let (gate, slot, provider) = (gate.clone(), slot.clone(), provider.clone());
            GATE.scope(gate.clone(), async move {
                let finished = finished;
                let answer = work.await;
                let state = lock(&gate.state);
                if state.abandoned {
                    finished.background.spent(&provider, usage(&answer));
                } else {
                    *lock(&slot) = Some(answer);
                }
                drop(state);
                drop(finished);
            })
        };
        let task = runtime.spawn(crate::observe::carried(task));
        lock(&gate.state).abort = Some(task.abort_handle());
        Waiting {
            gate,
            slot,
            task: Some(task),
            background: self.background.clone(),
            provider,
            usage,
        }
        .answer()
        .await
    }

    /// Waits up to `timeout` for paid requests still finishing after their
    /// callers stopped waiting (a claim verified while its other searches
    /// were out), then returns the usage of those done since the last
    /// settle, which no report carried. Their costs are already counted in
    /// [`budgets`](Self::budgets), the credit spent here and the caps; a
    /// front end adds this to the usage it shows. Call it before a process
    /// exits, which would cancel them: a request OpenAlex has started is
    /// charged whether or not it's answered. Long-lived embedders needn't:
    /// the requests finish on their own.
    pub async fn settle(&self, timeout: Duration) -> Vec<(ProviderId, Usage)> {
        let mut running = self.background.running.subscribe();
        if tokio::time::timeout(timeout, running.wait_for(|n| *n == 0))
            .await
            .is_err()
        {
            tracing::warn!(
                target: "aspero::net",
                running = *running.borrow(),
                "paid calls still finishing after their callers stopped waiting; exiting cancels them"
            );
        }
        std::mem::take(&mut *lock(&self.background.usage))
    }
}
