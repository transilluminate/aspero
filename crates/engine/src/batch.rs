//! Verifying a batch of claims: every claimed identifier looked up in one
//! batched call first, then the claims verified several at a time.

use std::pin::Pin;
use std::task::{Context, Poll};

use aspero_core::{Claim, ProviderId, Usage};
use futures::stream::{self, BoxStream, Stream, StreamExt};

use crate::{Engine, Verification, add_usage};

/// Verifications as they complete, each with its claim's index, in any
/// order. [`usage`](Batch::usage) totals what the batch has cost so far,
/// its prefetch included.
pub struct Batch<'a> {
    steps: BoxStream<'a, Step>,
    usage: Vec<(ProviderId, Usage)>,
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
    pub fn verify_batch<'a>(&'a self, claims: &'a [Claim], limit: u32) -> Batch<'a> {
        let prefetch = async move {
            match claims.len() > 1 {
                true => self.prefetch(claims).await.usage,
                false => Vec::new(),
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
            let step = match self.steps.poll_next_unpin(cx) {
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
