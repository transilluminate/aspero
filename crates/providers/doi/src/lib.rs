//! doi.org content-negotiation provider.
//!
//! Asks the DOI system itself for CSL-JSON (`Accept:
//! application/vnd.citationstyles.csl+json`); doi.org redirects to the DOI's
//! registration agency — Crossref, DataCite, mEDRA, JaLC, … — which answers.
//! So it covers every registered DOI, not just one agency's, and its 404 is
//! the authoritative "this DOI does not exist".
//!
//! One DOI per request (no batching), so it sits after the batch-capable
//! providers as a fallback. Observed 2026-11-10: Crossref-registered DOIs
//! answer with Crossref's own type names and a 10 req/s limit header;
//! DataCite's answers (e.g. arXiv) carry abstracts.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::csl::CslItem;
use aspero_core::http::path_segment;
use aspero_core::{
    Capabilities, Doi, Error, Field, FieldSet, HttpClient, HttpRequest, HttpResponse, IdKind,
    Lookup, ProviderId, Raw, ResiliencePolicy, Response, Result, SourceRecord, WorkId,
};
use async_trait::async_trait;

pub const ID: ProviderId = ProviderId::from_static("doi");

pub const DEFAULT_BASE_URL: &str = "https://doi.org";

const CSL_JSON: &str = "application/vnd.citationstyles.csl+json";

/// arXiv's DataCite prefix: `10.48550/arXiv.<id>`.
const ARXIV_DOI_PREFIX: &str = "10.48550";

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct DoiConfig {
    pub base_url: String,
}

impl Default for DoiConfig {
    fn default() -> Self {
        DoiConfig {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct DoiOrg {
    config: DoiConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl DoiOrg {
    pub fn new(config: DoiConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv];
        capabilities.max_batch = 1;
        // Every registered DOI resolves here, whatever its agency.
        capabilities.final_misses = vec![IdKind::Doi];
        // Records say nothing about free copies.
        capabilities.fields = FieldSet::ALL.difference(FieldSet::of(&[Field::OpenAccess]));
        DoiOrg {
            config,
            http,
            capabilities,
        }
    }

    /// Agencies advertise about 10 req/s; stay well under.
    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(200);
        policy
    }

    async fn fetch(&self, doi: &Doi) -> Result<Option<SourceRecord>> {
        let url = format!(
            "{}/{}",
            self.config.base_url.trim_end_matches('/'),
            path_segment(doi.as_str())
        );
        let resp = self
            .http
            .send(HttpRequest::get(url).header("accept", CSL_JSON))
            .await?;
        match check(&resp) {
            Ok(()) => {}
            Err(Error::NotFound) => return Ok(None),
            Err(e) => return Err(e),
        }
        let value: serde_json::Value = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let item: CslItem = serde_json::from_value(value.clone()).map_err(Error::decode)?;
        let raw = Raw {
            media_type: CSL_JSON.into(),
            body: resp.text(),
        };
        Ok(Some(SourceRecord::new(
            ID,
            ids(doi, &item),
            item.to_work(),
            FieldSet::ALL,
            raw,
            resp.received_at,
        )))
    }
}

/// The record's identifiers: its DOI as served (else as asked), plus the
/// arXiv ID behind an arXiv DataCite DOI.
fn ids(asked: &Doi, item: &CslItem) -> Vec<WorkId> {
    let doi = item
        .doi
        .as_deref()
        .and_then(|d| d.parse::<Doi>().ok())
        .unwrap_or_else(|| asked.clone());
    let arxiv = (doi.prefix() == ARXIV_DOI_PREFIX)
        .then(|| doi.suffix().strip_prefix("arxiv.")?.parse().ok())
        .flatten()
        .map(WorkId::Arxiv);
    std::iter::once(WorkId::Doi(doi)).chain(arxiv).collect()
}

fn check(resp: &HttpResponse) -> Result<()> {
    if resp.is_success() {
        return Ok(());
    }
    Err(match resp.status {
        404 => Error::NotFound,
        // The DOI exists, but its agency offers no CSL-JSON.
        406 => Error::Unsupported("CSL-JSON from this DOI's registration agency"),
        429 => Error::RateLimited {
            retry_after: resp.retry_after(),
        },
        status => Error::Server {
            status,
            message: None,
        },
    })
}

#[async_trait]
impl aspero_core::Provider for DoiOrg {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let mut lookups = Vec::with_capacity(ids.len());
        for id in ids {
            let doi = match id {
                WorkId::Doi(doi) => doi.clone(),
                WorkId::Arxiv(arxiv) => {
                    format!("{ARXIV_DOI_PREFIX}/arxiv.{}", arxiv.id()).parse()?
                }
                _ => return Err(Error::Unsupported("doi.org lookup of non-DOI identifiers")),
            };
            lookups.push(Lookup {
                id: id.clone(),
                record: self.fetch(&doi).await?,
            });
        }
        Ok(Response::new(lookups))
    }
}
