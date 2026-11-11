//! Sharing lookups already in flight: callers asking one provider about the
//! same identifier at the same time get one request between them.
//!
//! The first caller leads: it sends the request and publishes each answer.
//! Later callers join and wait for it. A leader that is dropped before
//! answering (its caller cancelled) closes its calls, and joiners then ask
//! themselves.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use aspero_core::{FieldSet, ProviderId, SourceRecord, WorkId};
use tokio::sync::watch;

use crate::Attempt;

pub(crate) type Answer = (Attempt, Option<SourceRecord>);

type Key = (ProviderId, WorkId);

#[derive(Default)]
pub(crate) struct InFlight {
    next_token: AtomicU64,
    calls: Mutex<HashMap<Key, Call>>,
}

struct Call {
    token: u64,
    fields: FieldSet,
    answer: watch::Receiver<Option<Answer>>,
}

/// The calls one caller leads, by its own index for each identifier.
pub(crate) struct Leads {
    inflight: Arc<InFlight>,
    provider: ProviderId,
    token: u64,
    calls: HashMap<usize, (WorkId, watch::Sender<Option<Answer>>)>,
}

/// An answer another caller is fetching.
pub(crate) struct Joined(watch::Receiver<Option<Answer>>);

impl InFlight {
    /// Leads the calls for `ids` (by index) that nobody is making yet, and
    /// joins the rest: those in flight for at least `fields`. A repeated
    /// identifier joins its own earlier lead.
    pub(crate) fn claim(
        self: &Arc<Self>,
        provider: &ProviderId,
        ids: &[(usize, WorkId)],
        fields: FieldSet,
    ) -> (Leads, Vec<(usize, Joined)>) {
        let token = self.next_token.fetch_add(1, Ordering::Relaxed);
        let mut leads = Leads {
            inflight: self.clone(),
            provider: provider.clone(),
            token,
            calls: HashMap::new(),
        };
        let mut joined = Vec::new();
        let mut calls = self.calls.lock().expect("in-flight map");
        for (i, id) in ids {
            let key = (provider.clone(), id.clone());
            match calls.get(&key) {
                Some(call) if fields.is_subset(call.fields) => {
                    joined.push((*i, Joined(call.answer.clone())));
                }
                _ => {
                    let (tx, rx) = watch::channel(None);
                    calls.insert(
                        key,
                        Call {
                            token,
                            fields,
                            answer: rx,
                        },
                    );
                    leads.calls.insert(*i, (id.clone(), tx));
                }
            }
        }
        (leads, joined)
    }

    fn finish(&self, provider: &ProviderId, id: &WorkId, token: u64) {
        let mut calls = self.calls.lock().expect("in-flight map");
        let key = (provider.clone(), id.clone());
        if calls.get(&key).is_some_and(|c| c.token == token) {
            calls.remove(&key);
        }
    }
}

impl Leads {
    /// The indices this caller leads.
    pub(crate) fn indices(&self) -> Vec<usize> {
        let mut led: Vec<usize> = self.calls.keys().copied().collect();
        led.sort_unstable();
        led
    }

    /// Hands every led call that now has an answer to its joiners.
    pub(crate) fn publish(&mut self, answers: &[Option<Answer>]) {
        let ready: Vec<usize> = self
            .calls
            .keys()
            .copied()
            .filter(|&i| answers[i].is_some())
            .collect();
        for i in ready {
            let (id, tx) = self.calls.remove(&i).expect("led call");
            tx.send_replace(answers[i].clone());
            self.inflight.finish(&self.provider, &id, self.token);
        }
    }
}

impl Drop for Leads {
    /// Calls never answered are closed, so their joiners ask themselves.
    fn drop(&mut self) {
        for (id, _) in self.calls.values() {
            self.inflight.finish(&self.provider, id, self.token);
        }
    }
}

impl Joined {
    /// The leader's answer, or `None` if it gave up without one.
    pub(crate) async fn answer(mut self) -> Option<Answer> {
        self.0
            .wait_for(Option::is_some)
            .await
            .ok()
            .and_then(|a| a.clone())
    }
}
