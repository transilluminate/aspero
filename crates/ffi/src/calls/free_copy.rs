//! A work's free copy that a program can fetch (#204).

use std::sync::Arc;

use crate::records::{FreeCopy, Work};
use crate::{Aspero, AsperoError, CallToken, Patience};

#[uniffi::export]
impl Aspero {
    /// The free copy of `work`, for an app about to fetch it: its open
    /// access, with a PDF a program can fetch where PubMed Central's open
    /// data has one. Asks PMC's open data
    /// (`pmc-oa-opendata.s3.amazonaws.com`, sending only the PMCID: two
    /// requests, one when PMC hasn't the article) when the work has a
    /// PMCID and no fetchable PDF; nothing for other works. `lookup` never
    /// asks, even with `full`. Cached like lookups.
    #[uniffi::method(default(cancel = None))]
    pub async fn free_copy(
        &self,
        work: Work,
        patience: Patience,
        cancel: Option<Arc<CallToken>>,
    ) -> Result<FreeCopy, AsperoError> {
        let engine = self.engine(patience);
        self.run(
            async move { FreeCopy::from(&engine.free_copy(&work.to_record()).await) },
            cancel,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use aspero_core::testing::{ReplayClient, fixture};
    use aspero_core::{
        Capabilities, FieldSet, IdKind, Lookup, OpenAccess, Provider, ProviderId, Raw,
        ResiliencePolicy, Response, Result, SourceRecord, WorkId,
    };
    use aspero_engine::Engine;
    use aspero_pmc::{Pmc, PmcConfig};

    use super::*;
    use crate::records::{FreeCopyKind, LookupStatus};

    const FIXTURES: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../providers/pmc/tests/fixtures"
    );
    const S3_PDF: &str = "https://pmc-oa-opendata.s3.amazonaws.com/PMC8114386.1/PMC8114386.1.pdf";

    /// Europe PMC as it answers for a PMC article: free, its PDF link the
    /// render endpoint that turns scripts away.
    struct EuropePmc(Capabilities);

    #[async_trait::async_trait]
    impl Provider for EuropePmc {
        fn id(&self) -> ProviderId {
            ProviderId::from_static("europepmc")
        }

        fn capabilities(&self) -> &Capabilities {
            &self.0
        }

        async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
            let lookups = ids
                .iter()
                .map(|id| {
                    let mut work = aspero_core::Work::default();
                    work.title = Some("Success in increasing physical activity".into());
                    let mut oa = OpenAccess::default();
                    oa.is_oa = true;
                    oa.pdf_url =
                        Some("https://europepmc.org/articles/PMC8114386?pdf=render".into());
                    oa.source = Some("europepmc".into());
                    work.open_access = Some(oa);
                    let ids = vec![id.clone(), "10.1080/21642850.2018.1462707".parse().unwrap()];
                    let raw = Raw {
                        media_type: "application/json".into(),
                        body: "{}".into(),
                    };
                    let record = SourceRecord::new(
                        self.id(),
                        ids,
                        work,
                        FieldSet::ALL,
                        raw,
                        jiff::Timestamp::UNIX_EPOCH,
                    );
                    Lookup {
                        id: id.clone(),
                        record: Some(record),
                    }
                })
                .collect();
            Ok(Response::new(lookups))
        }
    }

    fn block_on<T>(f: impl Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
            .block_on(f)
    }

    #[test]
    fn full_lookups_never_ask_pmc_and_free_copy_does() {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Pmcid];
        caps.max_batch = 1;
        caps.fields = FieldSet::ALL;
        let http = Arc::new(ReplayClient::new([
            fixture(FIXTURES, "list_oa"),
            fixture(FIXTURES, "meta_oa"),
        ]));
        let engine = Engine::builder()
            .provider(Arc::new(EuropePmc(caps)), ResiliencePolicy::default(), 0)
            .free_copies(
                Arc::new(Pmc::new(PmcConfig::default(), http.clone())),
                ResiliencePolicy::default(),
            )
            .build();
        let aspero = Aspero::over(engine);

        let found = block_on(aspero.lookup(vec!["PMC8114386".into()], true, Patience::Batch, None))
            .unwrap();
        let lookup = &found.results[0];
        assert_eq!(lookup.status, LookupStatus::Found);
        let work = lookup.work.clone().unwrap();
        let oa = work.open_access.clone().unwrap();
        assert_eq!(oa.free_copy, Some(FreeCopyKind::BrowserOnly));
        assert!(
            http.requests().is_empty(),
            "a full lookup asks nothing of PMC: {:?}",
            http.requests()
        );

        let copy = block_on(aspero.free_copy(work, Patience::Batch, None)).unwrap();
        assert_eq!(copy.kind, FreeCopyKind::Pdf);
        let oa = copy.open_access.unwrap();
        assert_eq!(oa.pdf_url.as_deref(), Some(S3_PDF));
        assert_eq!(oa.free_copy, Some(FreeCopyKind::Pdf));
        assert_eq!(copy.attempt.unwrap().provider, "pmc");
        assert_eq!(http.requests().len(), 2);
    }
}
