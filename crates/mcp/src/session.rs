//! Who is calling, and what their session has spent.

use std::sync::Mutex;

use aspero_core::{ProviderId, Usage};
use serde_json::{Value, json};

use crate::{ToolError, ToolErrorKind};

/// A session's spend cap unless the caller sets one, in USD. An OpenAlex
/// search costs $0.001, so this is about 250 searches; lookups are free.
pub const DEFAULT_BUDGET_USD: f64 = 0.25;

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
    /// What the key may do (HTTP only): [`scope::SOURCES`](crate::scope::SOURCES)
    /// and the host's scopes it holds, as the host gave them.
    pub scopes: Vec<String>,
}

/// One connection: its caller, and its spend against a cap.
///
/// Tools that may spend are refused once the session has spent its
/// budget. The check comes before each call and a call's cost is known only
/// after it, so the last call can take the session past its budget by that
/// call's cost (an OpenAlex search: $0.001; a verification: at most a few
/// searches). Calls running at once hold their estimated cost
/// (`Reservations`), so together they pass it by no more than one call's
/// estimate.
#[derive(Debug)]
pub struct Session {
    caller: Caller,
    budget_usd: f64,
    spent_usd: Mutex<f64>,
    held: Reservations,
    /// How the user raises the budget, for the budget-spent error.
    raise: &'static str,
}

/// What calls in flight may spend against a budget, held from when each
/// starts until its cost is known.
///
/// A call starts at once when its estimate fits in what's left of the
/// budget beside the estimates held; otherwise it waits for the calls in
/// flight, and runs once it fits or nothing else is running. So calls
/// costing no more than their estimates pass the budget by at most one
/// call's cost (the one that ran alone), as calls one at a time do. At most
/// `most` run at once, which bounds what calls costing more than estimated
/// can add.
#[derive(Debug)]
pub(crate) struct Reservations {
    held: Mutex<Held>,
    released: tokio::sync::Notify,
    most: usize,
}

#[derive(Debug, Default)]
struct Held {
    usd: f64,
    calls: usize,
}

/// A call's place among the calls in flight: let go when dropped, once
/// its cost has been recorded.
#[must_use]
pub(crate) struct Hold<'a> {
    from: &'a Reservations,
    usd: f64,
}

impl Reservations {
    pub(crate) fn new(most: usize) -> Self {
        Reservations {
            held: Mutex::default(),
            released: tokio::sync::Notify::new(),
            most,
        }
    }

    /// Waits for a call estimated to cost `estimate` to be let start.
    /// `left` says what's left of the budget, or why no call may start (it
    /// is spent), and is asked under the lock each time.
    pub(crate) async fn hold(
        &self,
        estimate: f64,
        left: impl Fn() -> Result<f64, ToolError>,
    ) -> Result<Hold<'_>, ToolError> {
        loop {
            // Before looking, so a release in between isn't missed.
            let released = self.released.notified();
            {
                let mut held = lock(&self.held);
                let left = left()?;
                if held.calls == 0 || (held.calls < self.most && held.usd + estimate <= left) {
                    held.calls += 1;
                    held.usd += estimate;
                    return Ok(Hold {
                        from: self,
                        usd: estimate,
                    });
                }
            }
            released.await;
        }
    }
}

impl Drop for Hold<'_> {
    fn drop(&mut self) {
        {
            let mut held = lock(&self.from.held);
            held.calls -= 1;
            // Exactly nothing once nothing's in flight, whatever the floats.
            held.usd = match held.calls {
                0 => 0.0,
                _ => held.usd - self.usd,
            };
        }
        self.from.released.notify_waiters();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Session {
    pub fn new(caller: Caller, budget_usd: f64) -> Self {
        Session {
            caller,
            budget_usd,
            spent_usd: Mutex::new(0.0),
            held: Reservations::new(usize::MAX),
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

    /// Refuses a call that may spend once the budget is spent; otherwise
    /// holds `estimate` for it until it's charged and the hold dropped,
    /// waiting first if the calls in flight might spend what's left.
    pub(crate) async fn admit(&self, estimate: f64) -> Result<Hold<'_>, ToolError> {
        self.held.hold(estimate, || self.left()).await
    }

    /// What's left of the budget, or why no paid call may start.
    fn left(&self) -> Result<f64, ToolError> {
        let spent = self.spent_usd();
        if spent < self.budget_usd {
            return Ok(self.budget_usd - spent);
        }
        Err(ToolError::new(
            ToolErrorKind::BudgetSpent,
            format!(
                "this session's budget is spent (${:.4} of ${:.2}); free tools \
                 (search_supports) still work. Start a new session, or {}",
                spent, self.budget_usd, self.raise
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
