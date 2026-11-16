//! Budgets and spending: what a provider has left and what its calls
//! cost, each call's usage, and the daily caps reached.

use std::time::SystemTime;

use aspero_engine as engine;

use super::time;

/// A daily cap reached: paid calls to the provider stop until `resetsAt`.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BudgetCap {
    /// The cap reached in US dollars: `Config.openalexDailyCapUsd`, or for
    /// `capKind` `.credit`, `Config.openalexDailyCreditCapUsd`.
    pub cap_usd: f64,
    /// Spent today against that cap in US dollars: the allowance's use as
    /// the provider reports it, by everything using the key; or for
    /// `.credit`, the prepaid credit spent today through aspero here
    /// (`Budget.creditSpentTodayHereUsd`), not other tools using the key.
    pub spent_usd: f64,
    /// When the provider's budget resets (OpenAlex: midnight UTC).
    pub resets_at: Option<SystemTime>,
    /// Which cap was reached. Always given by aspero; optional only so
    /// records made before it existed still build.
    #[uniffi(default = None)]
    pub cap_kind: Option<BudgetCapKind>,
}

/// Which of the daily caps was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum BudgetCapKind {
    /// The cap on the daily allowance (`Config.openalexDailyCapUsd`),
    /// reached with some of the allowance left, or, with no credit cap,
    /// once it's used up.
    Allowance,
    /// The cap on prepaid credit spent today through aspero here
    /// (`Config.openalexDailyCreditCapUsd`), once the allowance is used
    /// up.
    Credit,
}

impl From<aspero_core::CapKind> for BudgetCapKind {
    fn from(kind: aspero_core::CapKind) -> Self {
        match kind {
            aspero_core::CapKind::Credit => BudgetCapKind::Credit,
            _ => BudgetCapKind::Allowance,
        }
    }
}

impl BudgetCap {
    pub(crate) fn of(error: &aspero_core::Error) -> Option<BudgetCap> {
        match error {
            aspero_core::Error::BudgetCapped {
                cap,
                used,
                resets_at,
                which,
            } => Some(BudgetCap {
                cap_usd: *cap,
                spent_usd: *used,
                resets_at: resets_at.map(time),
                cap_kind: Some((*which).into()),
            }),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderUsage {
    pub provider: String,
    /// Spent by the call, in US dollars.
    pub cost_usd: Option<f64>,
    /// Left in the budget period, in US dollars.
    pub remaining_usd: Option<f64>,
    pub resets_at: Option<SystemTime>,
    /// The budget for the period (OpenAlex: the daily allowance), in US
    /// dollars, as the provider says it.
    #[uniffi(default = None)]
    pub limit_usd: Option<f64>,
    /// A prepaid balance left beside the period's budget, in US dollars,
    /// where the provider says one.
    #[uniffi(default = None)]
    pub prepaid_remaining_usd: Option<f64>,
}

pub(crate) fn usage(usage: &[(aspero_core::ProviderId, aspero_core::Usage)]) -> Vec<ProviderUsage> {
    usage
        .iter()
        .map(|(p, u)| ProviderUsage {
            provider: p.to_string(),
            cost_usd: u.cost,
            remaining_usd: u.remaining,
            resets_at: u.resets_at.map(time),
            limit_usd: u.limit,
            prepaid_remaining_usd: u.prepaid_remaining,
        })
        .collect()
}

/// What a provider last said was left of its budget.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Budget {
    pub provider: String,
    /// Left today, in US dollars.
    pub remaining_usd: Option<f64>,
    pub resets_at: Option<SystemTime>,
    /// When the provider said so.
    pub seen_at: SystemTime,
    /// The daily allowance in US dollars, as the provider says it
    /// (OpenAlex: $1 with a key, $0.10 without). Spent today is
    /// `limitUsd - remainingUsd`.
    #[uniffi(default = None)]
    pub limit_usd: Option<f64>,
    /// A prepaid balance beside the daily allowance, in US dollars, where
    /// the provider says one.
    #[uniffi(default = None)]
    pub prepaid_remaining_usd: Option<f64>,
    /// Prepaid credit spent today, in US dollars, by everything using the
    /// key (the app, the CLI, scripts), as the allowance's use is: the
    /// day's first balance seen (by anything sharing the cache), raised by
    /// any top-up since, less the balance now. OpenAlex doesn't say it, so
    /// a day first seen part-way through misses what was spent before.
    /// For show: `Config.openalexDailyCreditCapUsd` counts
    /// `creditSpentTodayHereUsd`. `None` where no balance is known today.
    #[uniffi(default = None)]
    pub prepaid_spent_today_usd: Option<f64>,
    /// Prepaid credit spent today, in US dollars, "here": charged to calls
    /// made through aspero sharing this cache (`Config.cacheDir`: this app,
    /// and the CLI or MCP server if they share it), not other tools using
    /// the key. Each paid call's cost once the allowance is used up.
    /// Against `Config.openalexDailyCreditCapUsd`: OpenAlex gives one key
    /// per account, shared by scripts and other tools, so their credit
    /// never stops this app. `nil` until the provider reports a budget.
    #[uniffi(default = None)]
    pub credit_spent_today_here_usd: Option<f64>,
    /// When the prepaid balance expires, where the provider says (OpenAlex:
    /// asked by `refreshBudgets` with a key).
    #[uniffi(default = None)]
    pub prepaid_expires_at: Option<SystemTime>,
    /// What a page of a keyword search costs, in US dollars, as the
    /// provider says (OpenAlex: `$0.001`, asked by `refreshBudgets` with a
    /// key; `nil` until then). With `remainingUsd` and the caps, says how
    /// many searches are left.
    #[uniffi(default = None)]
    pub search_price_usd: Option<f64>,
    /// What each kind of call costs, as the provider says (OpenAlex, asked
    /// by `refreshBudgets` with a key: `singleton`, `list`, `search`,
    /// `search_rerank`, `rerank_extra`, `content`, `semantic`, `text`, as
    /// its `/rate-limit` names them), in a fixed order; empty until then.
    #[uniffi(default = [])]
    pub prices: Vec<BudgetPrice>,
    /// What this engine's paid calls may still spend today, in US dollars,
    /// under both caps: the allowance left (counting everything using the
    /// key) up to `Config.openalexDailyCapUsd`, then, once paid calls would
    /// use the allowance up, the credit left, up to `creditCapUsd` less
    /// `creditSpentTodayHereUsd`. As
    /// `aspero budget` counts it; over `searchPriceUsd`, the searches left.
    /// `nil` when the allowance left is unknown.
    #[uniffi(default = None)]
    pub spendable_today_usd: Option<f64>,
    /// The cap on prepaid credit spent today in force, in US dollars:
    /// `Config.openalexDailyCreditCapUsd`, or aspero's default ($0.25)
    /// when that's unset. `nil` for a provider without one.
    #[uniffi(default = None)]
    pub credit_cap_usd: Option<f64>,
}

/// What one kind of call costs.
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct BudgetPrice {
    /// The provider's name for it: OpenAlex's `search`, `list`, ….
    pub endpoint: String,
    pub usd: f64,
}

impl Budget {
    pub(crate) fn new(provider: &aspero_core::ProviderId, b: &engine::Budget) -> Self {
        Budget {
            provider: provider.to_string(),
            remaining_usd: b.remaining,
            resets_at: b.resets_at.map(time),
            seen_at: time(b.seen_at),
            limit_usd: b.limit,
            prepaid_remaining_usd: b.prepaid_remaining,
            prepaid_spent_today_usd: b.prepaid_spent_today,
            credit_spent_today_here_usd: b.credit_spent_today_here,
            prepaid_expires_at: b.prepaid_expires_at.map(time),
            search_price_usd: b.search_price(),
            prices: b
                .prices
                .iter()
                .flat_map(aspero_core::Prices::iter)
                .map(|(endpoint, usd)| BudgetPrice {
                    endpoint: endpoint.to_owned(),
                    usd,
                })
                .collect(),
            spendable_today_usd: None,
            credit_cap_usd: None,
        }
    }

    /// With what `engine`'s caps leave spendable today, and its credit cap.
    pub(crate) fn capped(
        mut self,
        engine: &engine::Engine,
        provider: &aspero_core::ProviderId,
        b: &engine::Budget,
    ) -> Self {
        self.spendable_today_usd = engine.spendable_today(provider, b);
        self.credit_cap_usd = engine.daily_credit_cap(provider);
        self
    }
}
