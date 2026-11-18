//! Verifying a batch of claims: every claimed identifier looked up in one
//! batched call first, then the claims verified several at a time.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use aspero_core::{Claim, ProviderId, Usage};
use futures::stream::{self, BoxStream, Stream, StreamExt};

use crate::observe::{self, CallScope};
use crate::{Engine, Verification, add_usage};

/// Verifications as they complete, each with its claim's index, in any
/// order. [`usage`](Batch::usage) totals what the batch has cost so far,
/// its prefetch included.
pub struct Batch<'a> {
    steps: BoxStream<'a, Step>,
    usage: Vec<(ProviderId, Usage)>,
    /// More than one claim, the batch's id not yet taken: it is at the
    /// first poll.
    unnamed: bool,
    /// With more than one claim, outside an engine call: the id its
    /// claims' calls and prefetch name as `parent`.
    scope: Option<Arc<CallScope>>,
}

// One step at a time, straight from the stream: not worth boxing.
#[allow(clippy::large_enum_variant)]
enum Step {
    Prefetched(Vec<(ProviderId, Usage)>),
    Verified(usize, Verification),
}

impl Engine {
    /// Verifies many claims as one batch: with more than one, their
    /// identifiers are looked up together first (warming the cache), then
    /// claims are verified as by [`verify_many`](Engine::verify_many).
    ///
    /// Each claim's verify is an engine call, and so is the prefetch. With
    /// more than one claim, they're the batch's parts: each says the
    /// batch's id as its `parent` (DESIGN "Diagnostics events"). The batch
    /// takes its id when it's first polled: a [`CallId`](crate::CallId)
    /// given there, else a new one. One claim's verify is the call itself.
    /// Within another engine call (a check), all are that call's parts.
    pub fn verify_batch<'a>(&'a self, claims: &'a [Claim], limit: u32) -> Batch<'a> {
        let prefetch = async move {
            if claims.len() > 1 {
                self.prefetch(claims).await.usage
            } else {
                Vec::new()
            }
        };
        let verified = self
            .verify_many(claims, limit)
            .map(|(i, v)| Step::Verified(i, v));
        Batch {
            steps: stream::once(prefetch)
                .map(Step::Prefetched)
                .chain(verified)
                .boxed(),
            usage: Vec::new(),
            unnamed: claims.len() > 1,
            scope: None,
        }
    }
}

impl Batch<'_> {
    /// Usage per provider so far.
    pub fn usage(&self) -> &[(ProviderId, Usage)] {
        &self.usage
    }

    /// Adds usage spent alongside the batch (abstracts fetched for its
    /// results) to its total.
    pub fn add_usage(&mut self, usage: &[(ProviderId, Usage)]) {
        for (p, u) in usage {
            add_usage(&mut self.usage, p, *u);
        }
    }

    /// The batch's total usage, once it's done.
    pub fn into_usage(self) -> Vec<(ProviderId, Usage)> {
        self.usage
    }
}

impl Stream for Batch<'_> {
    type Item = (usize, Verification);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            let this = &mut *self;
            if std::mem::take(&mut this.unnamed) {
                this.scope = observe::batch();
            }
            let polled = observe::within(this.scope.clone(), || this.steps.poll_next_unpin(cx));
            let step = match polled {
                Poll::Ready(Some(step)) => step,
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            };
            match step {
                Step::Prefetched(usage) => self.add_usage(&usage),
                Step::Verified(i, v) => {
                    self.add_usage(&v.report.usage);
                    return Poll::Ready(Some((i, v)));
                }
            }
        }
    }
}
