//! aspero's JSON contract: the shapes agents and apps read, shared by the
//! CLI and any other front end so they can't drift. [`graph`] has citation
//! links: references pages and analyses of a set.
//!
//! Fields are omitted when unknown rather than `null`; the lean default set
//! is enough to identify and cite, and `full` adds everything known. Every
//! result echoes what it answers (`input`, a claim's `key`), so results can
//! arrive in any order.

pub mod check;
mod claim;
pub mod graph;
mod lookup;
mod query;
mod scan;
mod verify;
mod work;

pub use claim::{ClaimInput, InputClaim};
pub use lookup::{Status, disagreements, input as input_lookup, invalid, item as lookup_item};
pub use query::{parse_query, query_json};
pub use scan::mention;
pub use verify::{invalid_claim, verification};
pub use work::{candidate, set_abstract, usage, usage_one, work};

use serde_json::Value;

/// `base` with `extra`'s fields added (both JSON objects).
pub fn merged(mut base: Value, extra: Value) -> Value {
    if let (Value::Object(b), Value::Object(e)) = (&mut base, extra) {
        b.extend(e);
    }
    base
}
