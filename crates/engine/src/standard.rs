//! Wiring for the built-in providers, so front ends don't repeat it.

use std::sync::Arc;

use aspero_core::{Error, HttpClient, ProviderId, ResiliencePolicy};

use crate::transport::{Paced, ReqwestClient};
use crate::{Cache, CachePolicy, Engine, EngineBuilder, Pacing, SecretString};

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

/// The built-in sources, in lookup priority order: each one's name (as
/// results' `provider` gives it) and what it's called.
pub const SOURCES: &[(&str, &str)] = &[
    ("openalex", "OpenAlex"),
    ("crossref", "Crossref"),
    ("datacite", "DataCite"),
    ("doi", "doi.org"),
    ("europepmc", "Europe PMC"),
    ("openlibrary", "Open Library"),
];

/// What a built-in source is called ("Europe PMC" for `europepmc`); other
/// names as they are.
pub fn source_name(id: &str) -> &str {
    SOURCES
        .iter()
        .find(|(name, _)| *name == id)
        .map_or(id, |(_, called)| called)
}

/// Why sources couldn't be turned off.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SourcesError {
    #[error("no source is named {0:?} (sources: {names})", names = names())]
    Unknown(String),
    #[error("every source is turned off: leave at least one")]
    NoneLeft,
}

fn names() -> String {
    let names: Vec<&str> = SOURCES.iter().map(|(name, _)| *name).collect();
    names.join(", ")
}

/// Settings for the compiled-in providers. `None` disables a provider.
#[derive(Clone, Default)]
#[non_exhaustive]
pub struct StandardConfig {
    /// Contact address sent in the user agent (and as Crossref's `mailto`).
    pub mailto: Option<String>,
    /// Where provider answers are cached; `None` for no cache.
    pub cache: Option<Arc<dyn Cache>>,
    /// How long cached answers are trusted.
    pub cache_policy: CachePolicy,
    /// Request pacing shared with other processes (usually the cache
    /// file); `None` paces this process alone.
    pub pacing: Option<Arc<dyn Pacing>>,
    /// Stops OpenAlex's paid calls once the day's use (as OpenAlex reports
    /// it, shared by everything using the key) reaches this many USD,
    /// until the budget resets; see [`EngineBuilder::daily_cap`].
    pub openalex_daily_cap: Option<f64>,
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
    /// Sources turned off with [`disable`](Self::disable): a search naming
    /// one fails saying so ([`EngineError::Disabled`](crate::EngineError::Disabled)).
    pub disabled: Vec<ProviderId>,
}

impl StandardConfig {
    /// Every compiled-in provider with its defaults, OpenAlex using
    /// `openalex_key`; no contact address, cache or shared pacing.
    // The key is unused only without the OpenAlex feature.
    #[allow(unused_variables)]
    pub fn all(openalex_key: Option<SecretString>) -> Self {
        StandardConfig {
            #[cfg(feature = "openalex")]
            openalex: Some({
                let mut openalex = OpenAlexConfig::default();
                openalex.api_key = openalex_key;
                openalex
            }),
            #[cfg(feature = "crossref")]
            crossref: Some(CrossrefConfig::default()),
            #[cfg(feature = "datacite")]
            datacite: Some(DataciteConfig::default()),
            #[cfg(feature = "doi")]
            doi: Some(DoiConfig::default()),
            #[cfg(feature = "europepmc")]
            europepmc: Some(EuropePmcConfig::default()),
            #[cfg(feature = "openlibrary")]
            openlibrary: Some(OpenLibraryConfig::default()),
            ..StandardConfig::default()
        }
    }

    /// Turns off the sources `names` names (as [`SOURCES`] and results'
    /// `provider` do: `europepmc`, `openlibrary`, …): they are never built
    /// or asked, for lookups, checks or searches, and a search naming one
    /// fails saying it's turned off. Names are trimmed and compared
    /// ignoring case; blanks are skipped. An unknown name, or turning every
    /// source off, is an error, and changes nothing.
    pub fn disable<S: AsRef<str>>(&mut self, names: &[S]) -> Result<(), SourcesError> {
        let mut ids = Vec::new();
        for name in names {
            let name = name.as_ref().trim().to_ascii_lowercase();
            if name.is_empty() {
                continue;
            }
            let Some((known, _)) = SOURCES.iter().find(|(n, _)| *n == name) else {
                return Err(SourcesError::Unknown(name));
            };
            let id = ProviderId::from_static(known);
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        let mut left = self.clone();
        for id in &ids {
            left.turn_off(id.as_str());
        }
        if left.enabled() == 0 {
            return Err(SourcesError::NoneLeft);
        }
        for id in ids {
            if !left.disabled.contains(&id) {
                left.disabled.push(id);
            }
        }
        *self = left;
        Ok(())
    }

    /// Removes one compiled-in source's settings.
    fn turn_off(&mut self, name: &str) {
        match name {
            #[cfg(feature = "openalex")]
            "openalex" => self.openalex = None,
            #[cfg(feature = "crossref")]
            "crossref" => self.crossref = None,
            #[cfg(feature = "datacite")]
            "datacite" => self.datacite = None,
            #[cfg(feature = "doi")]
            "doi" => self.doi = None,
            #[cfg(feature = "europepmc")]
            "europepmc" => self.europepmc = None,
            #[cfg(feature = "openlibrary")]
            "openlibrary" => self.openlibrary = None,
            // Not compiled in: off already.
            _ => {}
        }
    }

    /// How many sources are set to be built.
    fn enabled(&self) -> usize {
        #[allow(unused_mut)]
        let mut on = 0;
        #[cfg(feature = "openalex")]
        {
            on += usize::from(self.openalex.is_some());
        }
        #[cfg(feature = "crossref")]
        {
            on += usize::from(self.crossref.is_some());
        }
        #[cfg(feature = "datacite")]
        {
            on += usize::from(self.datacite.is_some());
        }
        #[cfg(feature = "doi")]
        {
            on += usize::from(self.doi.is_some());
        }
        #[cfg(feature = "europepmc")]
        {
            on += usize::from(self.europepmc.is_some());
        }
        #[cfg(feature = "openlibrary")]
        {
            on += usize::from(self.openlibrary.is_some());
        }
        on
    }
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
        let paced =
            Paced::new(label, inner, policy.min_interval).max_in_flight(policy.max_in_flight);
        Ok(Arc::new(match &config.pacing {
            Some(pacing) => paced.shared(pacing.clone()),
            None => paced,
        }))
    };

    let mut builder: EngineBuilder = Engine::builder();
    if let Some(cache) = config.cache {
        builder = builder.cache(cache);
    }
    builder = builder.cache_policy(config.cache_policy);
    for id in config.disabled {
        builder = builder.disabled(id);
    }

    #[cfg(feature = "openalex")]
    if let Some(oa) = config.openalex {
        use aspero_openalex::OpenAlex;
        let policy = OpenAlex::default_policy();
        let provider = OpenAlex::new(oa, client("openalex", &policy)?);
        builder = builder.provider(Arc::new(provider), policy, 10);
        if let Some(cap) = config.openalex_daily_cap {
            builder = builder.daily_cap(aspero_openalex::ID, cap);
        }
    }

    #[cfg(feature = "crossref")]
    if let Some(mut cr) = config.crossref {
        use aspero_crossref::Crossref;
        if cr.mailto.is_none() {
            cr.mailto = config.mailto.clone();
        }
        let mut policy = Crossref::default_policy();
        // Crossref answers a second concurrent request from its public
        // pool with 429; its polite pool (with a contact address) allows
        // a few.
        policy.max_in_flight = if cr.mailto.is_some() { 3 } else { 1 };
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
