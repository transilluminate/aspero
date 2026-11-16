//! Query trees in their JSON form: parsing, writing back (which re-parses
//! to the same tree), normalising (idempotent), validating, and every
//! compiled-in source's offline `search_supports`, which expresses the
//! tree in its own syntax. No requests are made.

#![no_main]

use std::sync::OnceLock;

use aspero_core::QueryNode;
use aspero_engine::{Engine, StandardConfig};
use aspero_json::{QueryTree, parse_query};
use libfuzzer_sys::fuzz_target;

fn engine() -> &'static Engine {
    static ENGINE: OnceLock<Engine> = OnceLock::new();
    ENGINE.get_or_init(|| aspero_engine::standard(StandardConfig::all(None)).expect("an engine"))
}

fn check_tree(node: &QueryNode) {
    let json = serde_json::to_value(QueryTree(node.clone())).expect("serialises");
    assert_eq!(
        parse_query(&json).as_ref(),
        Ok(node),
        "written as {json}, read back otherwise"
    );
    let _ = node.leaves();
    let _ = node.single_field();
    let _ = node.leaf_names();
    let valid = node.validate();
    assert_eq!(node.validate(), valid);
    if let Some(normal) = node.clone().normalised() {
        assert_eq!(
            normal.clone().normalised().as_ref(),
            Some(&normal),
            "normalising {node:?} again changed it"
        );
        let _ = normal.validate();
    }
    let engine = engine();
    let _ = engine.search_supports(None, node);
    let since = jiff::civil::date(2024, 1, 1);
    for (source, _) in aspero_engine::SOURCES {
        let _ = engine.search_supports(Some(source), node);
        let _ = engine.search_supports_since(Some(source), node, Some(since));
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(data) else {
        return;
    };
    let parsed = parse_query(&value);
    let typed = serde_json::from_value::<QueryTree>(value).map(|t| t.0);
    assert_eq!(parsed.is_ok(), typed.is_ok());
    if let Ok(node) = parsed {
        assert_eq!(typed.ok().as_ref(), Some(&node));
        check_tree(&node);
    }
});
