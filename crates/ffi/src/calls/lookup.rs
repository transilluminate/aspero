//! Looking up identifiers, and the work a document is.

use std::sync::Arc;

use aspero_engine::Extras;
use aspero_json::InputClaim;

use crate::records::{self, Identification, Lookup, Lookups, Verification};
use crate::{Aspero, AsperoError, CallToken, Patience};

#[uniffi::export]
impl Aspero {
    /// Looks up identifiers as written (DOIs, URLs, PMIDs, arXiv IDs, …).
    /// With `full`: notices in both directions, open access, disagreements
    /// with the DOI's registry, and the latest version.
    #[uniffi::method(default(cancel = None))]
    pub async fn lookup(
        &self,
        ids: Vec<String>,
        full: bool,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Lookups, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let mut extras = Extras::default();
                extras.full = full;
                let report = engine.lookup_inputs(&ids, extras).await;
                Lookups {
                    results: ids
                        .iter()
                        .zip(&report.items)
                        .map(|(input, item)| Lookup::new(input, item))
                        .collect(),
                    usage: records::usage(&report.usage),
                }
            },
            cancel,
        )
        .await
    }

    /// The work a document is, from its first page's text: not the works
    /// it cites or links to. `title_hint` is the largest type at the top of
    /// the page, `title` the file's title metadata (ignored when junk).
    /// Each identifier on the page (header and footer first, at most six)
    /// must be confirmed by the title or the page; with none, the title
    /// and top of the page are searched. `None` when nothing is sure
    /// enough.
    #[uniffi::method(default(title = None, title_hint = None, cancel = None))]
    pub async fn identify(
        &self,
        first_page: String,
        title: Option<String>,
        title_hint: Option<String>,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<Option<Identification>, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move {
                let found = engine
                    .identify(&first_page, title.as_deref(), title_hint.as_deref())
                    .await
                    .identified?;
                let claim = InputClaim {
                    input: found.input(),
                    key: None,
                    claim: Ok(found.claim.clone()),
                };
                Some(Identification {
                    decided_by: found.decided_by.into(),
                    verification: Verification::new(0, &claim, &found.verification, false),
                })
            },
            cancel,
        )
        .await
    }
}
