//! The MCP server's `search_supports` tool with any JSON arguments: the
//! arguments' parsing (unknown keys refused, the query tree's own errors)
//! through to every source's offline answer and the call's summary. It is
//! the one tool that never makes a request, so the only one fuzzed whole.

#![no_main]

use std::sync::OnceLock;

use aspero_engine::StandardConfig;
use aspero_mcp::{Caller, Session, Tools};
use libfuzzer_sys::fuzz_target;

fn tools() -> &'static Tools {
    static TOOLS: OnceLock<Tools> = OnceLock::new();
    TOOLS.get_or_init(|| {
        Tools::new(aspero_engine::standard(StandardConfig::all(None)).expect("an engine"))
    })
}

fuzz_target!(|data: &[u8]| {
    let Ok(arguments) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    let tools = tools();
    assert!(!tools.spec("search_supports").expect("the tool").spends);
    let session = Session::new(Caller::default(), 1.0);
    let _ = futures::executor::block_on(tools.call(&session, "search_supports", arguments));
    assert!(session.spent_usd() == 0.0);
});
