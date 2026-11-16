//! What the app sets up a handle with (`Config`), and how patient each
//! call is (`Patience`).

use aspero_engine::{CachePolicy, CallOptions};

#[derive(Debug, Clone, uniffi::Record)]
pub struct Config {
    /// Contact address for providers' polite pools: the app's support
    /// address.
    pub mailto: Option<String>,
    pub openalex_key: Option<String>,
    /// Where to keep the answer cache (`cache.sqlite` inside); `None` for
    /// no cache.
    pub cache_dir: Option<String>,
    /// Stops OpenAlex's paid calls (searches, batch lookups) once today's
    /// use, as OpenAlex reports it for the key (shared with anything else
    /// using it), reaches this many US dollars, until the budget resets.
    /// Free lookups go on and checks fall back to the free sources; a
    /// search that needs OpenAlex fails as `budgetCapped`, its
    /// `Failure.budgetCap` saying the cap, what's used and when it resets.
    /// With a key, a free budget refresh decides the first paid call of
    /// the day. `None`: no cap but OpenAlex's own.
    #[uniffi(default = None)]
    pub openalex_daily_cap_usd: Option<f64>,
    /// Sources never to ask, for lookups, checks or searches, by the names
    /// `SearchResults.provider` gives: `openalex`, `crossref`, `datacite`,
    /// `doi`, `europepmc`, `openlibrary`. `searchCapabilities` leaves them
    /// out, and a search naming one fails as `unsupported` ("Europe PMC is
    /// turned off"). An unknown name, or every source, is a setup error.
    #[uniffi(default = [])]
    pub disabled_sources: Vec<String>,
    /// How many days a found record (with its notices: retractions,
    /// corrections) is trusted before the source is asked again; older,
    /// it's still served if the source fails. 0 asks every time.
    #[uniffi(default = 7)]
    pub cache_found_days: u32,
    /// How many days a "not found" is trusted before asking again (works
    /// get registered). 0 asks every time.
    #[uniffi(default = 1)]
    pub cache_not_found_days: u32,
    /// The cache's size limit, in megabytes (1,000,000 bytes) as
    /// `CacheStats.bytes` counts them: past it, the least recently used
    /// answers go first, checked after each write. `None`: no limit. At
    /// least 1.
    #[uniffi(default = None)]
    pub cache_max_mb: Option<u64>,
    /// Once today's OpenAlex allowance is used up (by anything using the
    /// key), lets paid calls go on, spending prepaid credit, only while the
    /// credit spent today through aspero here, sharing `cacheDir`
    /// (`Budget.creditSpentTodayHereUsd`; not other tools using the key),
    /// is under this many US dollars; `0`
    /// never spends credit, more allows more. Until the allowance is used
    /// up, `openalexDailyCapUsd` applies, holding the rest of it back. A
    /// search stopped here fails as `budgetCapped`, its
    /// `Failure.budgetCap.capKind` `.credit`. `None` (nil): the default,
    /// $0.25 a day. There's no unlimited setting: more takes a larger
    /// number.
    #[uniffi(default = None)]
    pub openalex_daily_credit_cap_usd: Option<f64>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            mailto: None,
            openalex_key: None,
            cache_dir: None,
            openalex_daily_cap_usd: None,
            disabled_sources: Vec::new(),
            cache_found_days: 7,
            cache_not_found_days: 1,
            cache_max_mb: None,
            openalex_daily_credit_cap_usd: None,
        }
    }
}

impl Config {
    /// How long cached answers are trusted, from the days given.
    pub(crate) fn cache_policy(&self) -> CachePolicy {
        CachePolicy::from_days(self.cache_found_days, self.cache_not_found_days)
    }
}

/// How patient a call is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum Patience {
    /// Providers' own retries and deadlines: for batches.
    Batch,
    /// Someone is waiting: give up on a provider within seconds and fall
    /// back.
    Interactive,
    /// The cache only, however old.
    Offline,
    /// Asks the sources again, past the cache, and caches their answers:
    /// for checking again before submitting. Falls back to the cached
    /// answer, marked stale, where a source fails.
    Fresh,
}

impl Patience {
    /// The engine's options for this patience.
    pub(crate) fn options(self) -> CallOptions {
        match self {
            Patience::Batch => CallOptions::default(),
            Patience::Interactive => CallOptions::interactive(),
            Patience::Offline => CallOptions::offline(),
            // Batch deadlines and retries, past the cache.
            Patience::Fresh => CallOptions::fresh(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_days_set_how_long_answers_are_trusted() {
        const DAY: u64 = 86_400;
        let default = Config::default().cache_policy();
        assert_eq!(default, CachePolicy::default());
        let config = Config {
            cache_found_days: 3,
            cache_not_found_days: 0,
            ..Config::default()
        };
        let policy = config.cache_policy();
        assert_eq!(policy.records.as_secs(), 3 * DAY);
        assert_eq!(policy.misses.as_secs(), 0);
        // The rest keep their defaults.
        assert_eq!(policy.searches, default.searches);
        assert_eq!(policy.references, default.references);
        assert_eq!(policy.cited_by, default.cited_by);
    }

    #[test]
    fn each_patience_has_its_options() {
        let batch = CallOptions::default();
        for patience in [
            Patience::Batch,
            Patience::Interactive,
            Patience::Offline,
            Patience::Fresh,
        ] {
            let options = patience.options();
            match patience {
                Patience::Batch => assert_eq!(options, batch),
                Patience::Interactive => assert_eq!(options, CallOptions::interactive()),
                Patience::Offline => assert!(options.offline && !options.fresh),
                Patience::Fresh => {
                    assert!(options.fresh && !options.offline);
                    // Waits as a batch does.
                    assert_eq!(options.deadline, batch.deadline);
                    assert_eq!(options.max_retries, batch.max_retries);
                }
            }
        }
    }
}
