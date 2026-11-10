//! Wiring for the built-in providers, so front ends don't repeat it.

use std::sync::Arc;

use aspero_core::{Error, HttpClient, ResiliencePolicy};

use crate::transport::{Paced, ReqwestClient};
use crate::{Cache, Engine, EngineBuilder};

#[cfg(feature = "crossref")]
pub use aspero_crossref::CrossrefConfig;
#[cfg(feature = "datacite")]
pub use aspero_datacite::DataciteConfig;
#[cfg(feature = "doi")]
pub use aspero_doi::DoiConfig;
#[cfg(feature = "europepmc")]
pub use aspero_europepmc::EuropePmcConfig;
#[cfg(feature = "openalex")]
pub use aspero_openalex::OpenAlexConfig;
#[cfg(feature = "openlibrary")]
pub use aspero_openlibrary::OpenLibraryConfig;

/// Settings for the compiled-in providers. `None` disables a provider.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct StandardConfig {
    /// Contact address sent in the user agent (and as Crossref's `mailto`).
    pub mailto: Option<String>,
    /// Where provider answers are cached; `None` for no cache.
    pub cache: Option<Arc<dyn Cache>>,
    #[cfg(feature = "openalex")]
    pub openalex: Option<OpenAlexConfig>,
    #[cfg(feature = "crossref")]
    pub crossref: Option<CrossrefConfig>,
    #[cfg(feature = "datacite")]
    pub datacite: Option<DataciteConfig>,
    #[cfg(feature = "doi")]
    pub doi: Option<DoiConfig>,
    #[cfg(feature = "europepmc")]
    pub europepmc: Option<EuropePmcConfig>,
    #[cfg(feature = "openlibrary")]
    pub openlibrary: Option<OpenLibraryConfig>,
}

/// Builds an engine with each enabled provider behind its own paced
/// `reqwest` client.
///
/// Priority for lookups: OpenAlex first (free per-ID lookups, many identifier
/// kinds, indexes DataCite DOIs such as arXiv's), then Crossref, then
/// DataCite (arXiv, Zenodo, data and software; batched), then doi.org (any
/// registration agency, one DOI per request; its "not found" is
/// authoritative), then Europe PMC (PMIDs, PMCIDs, biomedical abstracts).
/// Open Library answers ISBNs (books) and adds book search.
pub fn standard(config: StandardConfig) -> Result<Engine, Error> {
    let user_agent = match &config.mailto {
        Some(mailto) => format!("aspero/{} (mailto:{mailto})", env!("CARGO_PKG_VERSION")),
        None => format!("aspero/{}", env!("CARGO_PKG_VERSION")),
    };
    // Unused only when every provider feature is off.
    #[allow(unused_variables)]
    let client = |label: &str, policy: &ResiliencePolicy| -> Result<Arc<dyn HttpClient>, Error> {
        let inner = Arc::new(ReqwestClient::new(&user_agent, policy.attempt_timeout)?);
        Ok(Arc::new(Paced::new(label, inner, policy.min_interval)))
    };

    let mut builder: EngineBuilder = Engine::builder();
    if let Some(cache) = config.cache {
        builder = builder.cache(cache);
    }

    #[cfg(feature = "openalex")]
    if let Some(oa) = config.openalex {
        use aspero_openalex::OpenAlex;
        let policy = OpenAlex::default_policy();
        let provider = OpenAlex::new(oa, client("openalex", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 10);
    }

    #[cfg(feature = "crossref")]
    if let Some(mut cr) = config.crossref {
        use aspero_crossref::Crossref;
        if cr.mailto.is_none() {
            cr.mailto = config.mailto.clone();
        }
        let policy = Crossref::default_policy();
        let provider = Crossref::new(cr, client("crossref", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 20);
    }

    #[cfg(feature = "datacite")]
    if let Some(dc) = config.datacite {
        use aspero_datacite::Datacite;
        let policy = Datacite::default_policy();
        let provider = Datacite::new(dc, client("datacite", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 25);
    }

    #[cfg(feature = "doi")]
    if let Some(doi) = config.doi {
        use aspero_doi::DoiOrg;
        let policy = DoiOrg::default_policy();
        let provider = DoiOrg::new(doi, client("doi", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 30);
    }

    #[cfg(feature = "europepmc")]
    if let Some(epmc) = config.europepmc {
        use aspero_europepmc::EuropePmc;
        let policy = EuropePmc::default_policy();
        let provider = EuropePmc::new(epmc, client("europepmc", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 35);
    }

    #[cfg(feature = "openlibrary")]
    if let Some(ol) = config.openlibrary {
        use aspero_openlibrary::OpenLibrary;
        let policy = OpenLibrary::default_policy();
        let provider = OpenLibrary::new(ol, client("openlibrary", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 40);
    }

    Ok(builder.build())
}
