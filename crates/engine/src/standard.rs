//! Wiring for the built-in providers, so front ends don't repeat it.

use std::sync::Arc;

use aspero_core::{Error, HttpClient, ResiliencePolicy};

use crate::transport::{Paced, ReqwestClient};
use crate::{Cache, Engine, EngineBuilder};

#[cfg(feature = "crossref")]
pub use aspero_crossref::CrossrefConfig;
#[cfg(feature = "openalex")]
pub use aspero_openalex::OpenAlexConfig;

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
}

/// Builds an engine with each enabled provider behind its own paced
/// `reqwest` client.
///
/// Priority for lookups: OpenAlex first (free per-ID lookups, many identifier
/// kinds, indexes DataCite DOIs such as arXiv's), then Crossref.
pub fn standard(config: StandardConfig) -> Result<Engine, Error> {
    let user_agent = match &config.mailto {
        Some(mailto) => format!("aspero/{} (mailto:{mailto})", env!("CARGO_PKG_VERSION")),
        None => format!("aspero/{}", env!("CARGO_PKG_VERSION")),
    };
    // Unused only when every provider feature is off.
    #[allow(unused_variables)]
    let client = |policy: &ResiliencePolicy| -> Result<Arc<dyn HttpClient>, Error> {
        let inner = Arc::new(ReqwestClient::new(&user_agent, policy.attempt_timeout)?);
        Ok(Arc::new(Paced::new(inner, policy.min_interval)))
    };

    let mut builder: EngineBuilder = Engine::builder();
    if let Some(cache) = config.cache {
        builder = builder.cache(cache);
    }

    #[cfg(feature = "openalex")]
    if let Some(oa) = config.openalex {
        use aspero_openalex::OpenAlex;
        let policy = OpenAlex::default_policy();
        let provider = OpenAlex::new(oa, client(&policy)?);
        builder = builder.provider(Arc::new(provider), policy, 10);
    }

    #[cfg(feature = "crossref")]
    if let Some(mut cr) = config.crossref {
        use aspero_crossref::Crossref;
        if cr.mailto.is_none() {
            cr.mailto = config.mailto.clone();
        }
        let policy = Crossref::default_policy();
        let provider = Crossref::new(cr, client(&policy)?);
        builder = builder.provider(Arc::new(provider), policy, 20);
    }

    Ok(builder.build())
}
