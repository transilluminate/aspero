//! Who is calling, and what their session has spent.

use std::sync::Mutex;

use aspero_core::{ProviderId, Usage};
use serde_json::{Value, json};

use crate::{ToolError, ToolErrorKind};

/// A session's spend cap unless the caller sets one, in USD. An OpenAlex
/// search costs $0.001, so this is about 250 searches; lookups are free.
pub const DEFAULT_BUDGET_USD: f64 = 0.25;

/// What a key may do over HTTP. Over stdio there are no keys: the agent
/// that launched the server may call every tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Scope {
    /// The engine tools: searching, looking up and verifying works in
    /// open sources (and spending on them).
    Sources,
    /// An app's tools that read its history.
    ReadHistory,
    /// An app's tools that add to its history.
    WriteHistory,
}

impl Scope {
    pub fn as_str(self) -> &'static str {
        match self {
            Scope::Sources => "sources",
            Scope::ReadHistory => "read_history",
            Scope::WriteHistory => "write_history",
        }
    }
}

/// Who is calling. Over stdio, the agent that launched the server, which
/// is all there is to know; over HTTP, also the key it presented.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct Caller {
    /// How the client introduced itself, for logs.
    pub client: Option<String>,
    /// The key's id and name, as the host gave them (HTTP only).
    pub key_id: Option<String>,
    pub key_name: Option<String>,
    /// What the key may do (HTTP only).
    pub scopes: Vec<Scope>,
}

/// One connection: its caller, and its spend against a cap.
///
/// Tools that may spend are refused once the session has spent its
/// budget. The check comes before each call and a call's cost is known only
/// after it, so the last call can take the session past its budget by that
/// call's cost (an OpenAlex search: $0.001; a verification: at most a few
/// searches), more with calls running at once.
#[derive(Debug)]
pub struct Session {
    caller: Caller,
    budget_usd: f64,
    spent_usd: Mutex<f64>,
    /// How the user raises the budget, for the budget-spent error.
    raise: &'static str,
}

impl Session {
    pub fn new(caller: Caller, budget_usd: f64) -> Self {
        Session {
            caller,
            budget_usd,
            spent_usd: Mutex::new(0.0),
            raise: "ask the user to raise `aspero mcp --budget`",
        }
    }

    /// Says how the user raises the budget when it's spent ("ask the user
    /// to raise the session budget in the app's settings").
    pub fn with_raise_hint(mut self, hint: &'static str) -> Self {
        self.raise = hint;
        self
    }

    pub fn caller(&self) -> &Caller {
        &self.caller
    }

    pub fn budget_usd(&self) -> f64 {
        self.budget_usd
    }

    pub fn spent_usd(&self) -> f64 {
        *self
            .spent_usd
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub fn left_usd(&self) -> f64 {
        (self.budget_usd - self.spent_usd()).max(0.0)
    }

    /// Refuses a call that may spend once the budget is spent.
    pub(crate) fn admit(&self) -> Result<(), ToolError> {
        if self.spent_usd() < self.budget_usd {
            return Ok(());
        }
        Err(ToolError::new(
            ToolErrorKind::BudgetSpent,
            format!(
                "this session's budget is spent (${:.4} of ${:.2}); free tools \
                 (search_supports) still work. Start a new session, or {}",
                self.spent_usd(),
                self.budget_usd,
                self.raise
            ),
        ))
    }

    /// Adds a call's usage to the session; returns what the call cost.
    /// Costs are in each provider's units: the paid ones report USD.
    pub(crate) fn charge(&self, usage: &[(ProviderId, Usage)]) -> f64 {
        // A fold from 0.0: `sum` starts at -0.0, which prints as "-0.0000".
        let cost = usage
            .iter()
            .filter_map(|(_, u)| u.cost)
            .fold(0.0, |total, c| total + c);
        *self
            .spent_usd
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += cost;
        cost
    }

    /// `{"spent_usd", "budget_usd", "left_usd"}`.
    pub(crate) fn json(&self) -> Value {
        json!({
            "spent_usd": usd(self.spent_usd()),
            "budget_usd": usd(self.budget_usd),
            "left_usd": usd(self.left_usd()),
        })
    }

    /// One line: "Session: $0.0020 spent of $1.00, $0.9980 left."
    pub(crate) fn line(&self) -> String {
        format!(
            "Session: ${:.4} spent of ${:.2}, ${:.4} left.",
            self.spent_usd(),
            self.budget_usd,
            self.left_usd()
        )
    }
}

/// Dollars to a millionth, without float noise (0.0030000000000000005).
pub(crate) fn usd(value: f64) -> f64 {
    (value * 1e6).round() / 1e6
}
