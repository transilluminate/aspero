//! Verifying citations, all at once or each as it finishes.

use std::sync::Arc;

use aspero_engine::Engine;
use aspero_json::InputClaim;
use futures::StreamExt;

use crate::records::{
    self, Candidate, Claim as ClaimRecord, ProviderUsage, Verification, Verifications, Work,
};
use crate::{Aspero, AsperoError, CallToken, Patience, VerifyListener};

#[uniffi::export]
impl Aspero {
    /// Scores works the app already has against a claim, offline, on
    /// `verify`'s terms for its own candidates: each work's `checks` and
    /// `confidence`, in the order given, the work as given (`foundBy`
    /// empty). For showing a search's results beside a citation, field by
    /// field. A claim that can't be read compares as empty, every check
    /// unknown.
    #[allow(clippy::needless_pass_by_value, clippy::unused_self)]
    pub fn compare(&self, claim: ClaimRecord, works: Vec<Work>) -> Vec<Candidate> {
        let claim = claim.into_input().claim.unwrap_or_default();
        let records = works.iter().map(Work::to_record).collect();
        aspero_engine::compare(&claim, records)
            .iter()
            .zip(works)
            .map(|(c, work)| Candidate {
                work,
                ..Candidate::from(c)
            })
            .collect()
    }

    /// Verifies citations, answering in claim order. With `full`, every
    /// candidate considered.
    #[uniffi::method(default(cancel = None))]
    pub async fn verify(
        &self,
        claims: Vec<ClaimRecord>,
        full: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Verifications, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut results: Vec<Option<Verification>> = vec![None; claims.len()];
                let usage = verify_all(&engine, claims, full, |v| {
                    let i = v.index as usize;
                    results[i] = Some(v);
                })
                .await;
                Verifications {
                    results: results.into_iter().flatten().collect(),
                    usage,
                }
            },
            cancel,
        )
        .await
    }

    /// As [`verify`](Self::verify), handing each result to `listener` as
    /// soon as it finishes, in any order. Returns the usage.
    #[uniffi::method(default(cancel = None))]
    pub async fn verify_each(
        &self,
        claims: Vec<ClaimRecord>,
        full: bool,
        patience: Patience,
        listener: Arc<dyn VerifyListener>,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Vec<ProviderUsage>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move { verify_all(&engine, claims, full, |v| listener.on_result(v)).await },
            cancel,
        )
        .await
    }
}

/// Verifies every claim, handing each result to `each` as it finishes:
/// unreadable claims first, then the rest as they complete.
async fn verify_all(
    engine: &Engine,
    claims: Vec<ClaimRecord>,
    full: bool,
    mut each: impl FnMut(Verification),
) -> Vec<ProviderUsage> {
    let claims: Vec<InputClaim> = claims.into_iter().map(ClaimRecord::into_input).collect();
    let (positions, valid) = InputClaim::valid(&claims);
    for (i, claim) in claims.iter().enumerate() {
        if let Err(e) = &claim.claim {
            each(Verification::invalid(i, claim, e));
        }
    }
    let mut verified = engine.verify_batch(&valid, aspero_engine::SEARCH_LIMIT);
    while let Some((k, v)) = verified.next().await {
        let i = positions[k];
        each(Verification::new(i, &claims[i], &v, full));
    }
    records::usage(verified.usage())
}
