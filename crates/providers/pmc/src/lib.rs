//! PubMed Central's open data on AWS (`pmc-oa-opendata`): where a PMC
//! article's PDF can be fetched by a program (#204).
//!
//! PMC's own site and Europe PMC's PDF links turn scripts away (a browser
//! challenge, a 403 or a 404). NLM publishes PMC's articles that allow
//! reuse in a public S3 bucket instead, one prefix per article version
//! (`PMC8114386.1/`), each with a metadata JSON naming the version's PDF,
//! if it has one. No key, no contact address: a request carries only the
//! PMCID.
//!
//! **Not a lookup source.** The engine holds this provider apart from its
//! lookup chain and asks it only for a work's free copy
//! (`Engine::free_copy`), never in a lookup, verify or search. Its records
//! say only where the free copy is, under [`Field::OpenAccess`].
//!
//! **Two requests per work** (one when the article isn't there): the
//! version listing (`?list-type=2&prefix=PMC….&delimiter=/`), then the
//! latest version's JSON. A work can have several versions (an author
//! manuscript, then the published article), and only the listing says
//! which is latest: trying `.1` first would give the manuscript's answer
//! for a work whose `.2` has the PDF.
//!
//! **What counts** (observed 2026-11-22): a PDF only where the JSON names
//! one and `is_pmc_openaccess` is true; PMC's author manuscripts
//! (`license_code: "TDM"`) have none. Articles outside the dataset (most
//! of PMC's non-reusable ones) list no versions. A retracted article
//! (`is_retracted`) keeps its PDF: the retraction is told by the notices
//! other sources report, not by withholding the copy.

use std::sync::Arc;
use std::time::Duration;

use aspero_core::http::status_error;
use aspero_core::{
    Capabilities, Doi, Error, Field, FieldSet, HttpClient, HttpRequest, HttpResponse, IdKind,
    Lookup, OpenAccess, Pmcid, Pmid, ProviderId, Raw, ResiliencePolicy, Response, Result,
    SourceRecord, Work, WorkId,
};
use async_trait::async_trait;
use serde::Deserialize;

pub const ID: ProviderId = ProviderId::from_static("pmc");

pub const DEFAULT_BASE_URL: &str = "https://pmc-oa-opendata.s3.amazonaws.com";

/// The bucket's `s3://` URLs, as the metadata gives them.
const S3_PREFIX: &str = "s3://pmc-oa-opendata/";

/// What this provider's records hold.
pub const FIELDS: FieldSet = FieldSet::of(&[Field::OpenAccess]);

#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct PmcConfig {
    pub base_url: String,
}

impl Default for PmcConfig {
    fn default() -> Self {
        PmcConfig {
            base_url: DEFAULT_BASE_URL.to_owned(),
        }
    }
}

pub struct Pmc {
    config: PmcConfig,
    http: Arc<dyn HttpClient>,
    capabilities: Capabilities,
}

impl Pmc {
    pub fn new(config: PmcConfig, http: Arc<dyn HttpClient>) -> Self {
        let mut capabilities = Capabilities::default();
        capabilities.lookup_kinds = vec![IdKind::Pmcid];
        capabilities.max_batch = 1;
        capabilities.fields = FIELDS;
        Pmc {
            config,
            http,
            capabilities,
        }
    }

    /// S3 takes thousands of requests a second; a free copy is asked for
    /// one work at a time, so pace gently all the same.
    pub fn default_policy() -> ResiliencePolicy {
        let mut policy = ResiliencePolicy::default();
        policy.min_interval = Duration::from_millis(200);
        policy
    }

    fn base(&self) -> &str {
        self.config.base_url.trim_end_matches('/')
    }

    async fn get(&self, url: String) -> Result<Option<HttpResponse>> {
        let resp = self.http.send(HttpRequest::get(url)).await?;
        if resp.is_success() {
            Ok(Some(resp))
        } else if resp.status == 404 {
            Ok(None)
        } else {
            Err(status_error(&resp, None))
        }
    }

    async fn fetch(&self, pmcid: Pmcid) -> Result<Option<SourceRecord>> {
        let listing = format!("{}/?list-type=2&prefix={pmcid}.&delimiter=/", self.base());
        let Some(resp) = self.get(listing).await? else {
            return Ok(None);
        };
        let Some(version) = latest_version(&resp.text(), pmcid) else {
            return Ok(None);
        };
        let meta = format!("{}/{pmcid}.{version}/{pmcid}.{version}.json", self.base());
        let Some(resp) = self.get(meta).await? else {
            return Ok(None);
        };
        let wire: Metadata = serde_json::from_slice(&resp.body).map_err(Error::decode)?;
        let raw = Raw {
            media_type: "application/json".into(),
            body: resp.text(),
        };
        Ok(Some(wire.into_record(pmcid, raw, resp.received_at)))
    }
}

/// The highest version a bucket listing names for `pmcid`: its common
/// prefixes are `PMC….<version>/`.
fn latest_version(listing: &str, pmcid: Pmcid) -> Option<u32> {
    let wanted = format!("<Prefix>{pmcid}.");
    listing
        .match_indices(&wanted)
        .filter_map(|(at, m)| {
            let rest = &listing[at + m.len()..];
            rest.split_once("/</Prefix>")?.0.parse::<u32>().ok()
        })
        .max()
}

/// An article version's metadata JSON (the bucket's README lists the
/// fields).
#[derive(Debug, Deserialize)]
struct Metadata {
    #[serde(default)]
    pmid: Option<u64>,
    #[serde(default)]
    doi: Option<String>,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    is_pmc_openaccess: bool,
    #[serde(default)]
    is_manuscript: bool,
    #[serde(default)]
    license_code: Option<String>,
    #[serde(default)]
    pdf_url: Option<String>,
}

impl Metadata {
    fn into_record(self, pmcid: Pmcid, raw: Raw, at: jiff::Timestamp) -> SourceRecord {
        let mut ids = vec![WorkId::Pmcid(pmcid)];
        if let Some(doi) = self.doi.as_deref().and_then(|d| d.parse::<Doi>().ok()) {
            ids.push(WorkId::Doi(doi));
        }
        if let Some(pmid) = self.pmid.and_then(|p| p.to_string().parse::<Pmid>().ok()) {
            ids.push(WorkId::Pmid(pmid));
        }
        let mut work = Work::default();
        work.title.clone_from(&self.title);
        work.open_access = Some(self.open_access(pmcid));
        SourceRecord::new(ID, ids, work, FIELDS, raw, at)
    }

    /// Every article in PMC is free to read on its page; its PDF is
    /// offered only where PMC counts the article open access.
    fn open_access(&self, pmcid: Pmcid) -> OpenAccess {
        let mut oa = OpenAccess::default();
        oa.is_oa = true;
        oa.url = Some(format!("https://pmc.ncbi.nlm.nih.gov/articles/{pmcid}/"));
        if self.is_pmc_openaccess {
            oa.pdf_url = self.pdf_url.as_deref().and_then(https_url);
            oa.license = self.license_code.as_deref().and_then(license);
        }
        oa.version = Some(
            if self.is_manuscript {
                "acceptedVersion"
            } else {
                "publishedVersion"
            }
            .into(),
        );
        oa.source = Some(ID.to_string());
        oa
    }
}

/// An `s3://pmc-oa-opendata/<key>?md5=…` URL as the bucket's HTTPS
/// address, without the digest (the object's `ETag` carries it).
pub fn https_url(s3: &str) -> Option<String> {
    let key = s3.strip_prefix(S3_PREFIX)?;
    let key = key.split(['?', '#']).next().unwrap_or(key);
    (!key.is_empty()).then(|| format!("{DEFAULT_BASE_URL}/{key}"))
}

/// A licence code (`CC BY`, `CC BY-NC-ND`, `CC0`) in the form the rest of
/// `open_access` uses (`cc-by`); none for `TDM` (text mining only).
fn license(code: &str) -> Option<String> {
    let code = code.trim();
    if code.is_empty() || code.eq_ignore_ascii_case("tdm") {
        return None;
    }
    Some(code.to_ascii_lowercase().replace([' ', '_'], "-"))
}

#[async_trait]
impl aspero_core::Provider for Pmc {
    fn id(&self) -> ProviderId {
        ID
    }

    fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    async fn lookup(&self, ids: &[WorkId], _fields: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let mut lookups = Vec::with_capacity(ids.len());
        for id in ids {
            let WorkId::Pmcid(pmcid) = id else {
                return Err(Error::Unsupported("PMC open data lookup of non-PMCIDs"));
            };
            lookups.push(Lookup {
                id: id.clone(),
                record: self.fetch(*pmcid).await?,
            });
        }
        Ok(Response::new(lookups))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_version_is_the_highest_number() {
        let pmcid: Pmcid = "PMC12".parse().unwrap();
        let listing = "<ListBucketResult><CommonPrefixes><Prefix>PMC12.2/</Prefix>\
            </CommonPrefixes><CommonPrefixes><Prefix>PMC12.10/</Prefix></CommonPrefixes>\
            <CommonPrefixes><Prefix>PMC12.9/</Prefix></CommonPrefixes></ListBucketResult>";
        assert_eq!(latest_version(listing, pmcid), Some(10));
        assert_eq!(latest_version("<Prefix>PMC12.</Prefix>", pmcid), None);
        assert_eq!(latest_version("<Prefix>PMC123.1/</Prefix>", pmcid), None);
    }

    #[test]
    fn s3_urls_become_https_without_the_digest() {
        assert_eq!(
            https_url("s3://pmc-oa-opendata/PMC1.1/PMC1.1.pdf?md5=abc").as_deref(),
            Some("https://pmc-oa-opendata.s3.amazonaws.com/PMC1.1/PMC1.1.pdf")
        );
        assert_eq!(https_url("s3://other-bucket/PMC1.1/PMC1.1.pdf"), None);
        assert_eq!(https_url("s3://pmc-oa-opendata/?md5=abc"), None);
    }

    #[test]
    fn licence_codes_read_as_the_rest_do() {
        assert_eq!(license("CC BY").as_deref(), Some("cc-by"));
        assert_eq!(license("CC BY-NC-ND").as_deref(), Some("cc-by-nc-nd"));
        assert_eq!(license("CC0").as_deref(), Some("cc0"));
        assert_eq!(license("TDM"), None);
    }
}
