//! Works the app already holds, marked in the engine tools' results: asked
//! once a result, by every identifier its works carry.

mod common;

use std::sync::{Arc, Mutex};

use aspero_core::{SourceRecord, WorkId};
use aspero_mcp::{Holdings, Output, Tools};
use common::{Scripted, attention, deep_learning, session, tools};
use futures::future::BoxFuture;
use serde_json::{Value, json};

/// The app's library: holds a work by any of these identifiers, and keeps
/// what it was asked.
struct Library {
    ids: Vec<&'static str>,
    asked: Mutex<Vec<Vec<Vec<String>>>>,
}

impl Library {
    fn holding(ids: &[&'static str]) -> Self {
        Library {
            ids: ids.to_vec(),
            asked: Mutex::default(),
        }
    }

    fn asked(&self) -> Vec<Vec<Vec<String>>> {
        self.asked.lock().unwrap().clone()
    }
}

impl Holdings for Library {
    fn held(&self, works: Vec<Vec<String>>) -> BoxFuture<'_, Option<Vec<bool>>> {
        let held = works
            .iter()
            .map(|ids| ids.iter().any(|id| self.ids.contains(&id.as_str())))
            .collect();
        self.asked.lock().unwrap().push(works);
        Box::pin(async move { Some(held) })
    }
}

/// `v` without its `held` marks and the session's spend, to compare with
/// an unmarked call's.
fn unmarked(mut v: Value) -> Value {
    fn strip(v: &mut Value) {
        match v {
            Value::Object(map) => {
                map.remove("held");
                map.values_mut().for_each(strip);
            }
            Value::Array(items) => items.iter_mut().for_each(strip),
            _ => {}
        }
    }
    strip(&mut v);
    v.as_object_mut().unwrap().remove("session");
    v
}

async fn both(tools: &Tools, library: &Library, name: &str, args: Value) -> (Output, Output) {
    let marked = tools
        .call_holding(&session(10.0), name, args.clone(), Some(library))
        .await
        .unwrap();
    let plain = tools.call(&session(10.0), name, args).await.unwrap();
    (marked, plain)
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|s| (*s).to_owned()).collect()
}

#[tokio::test]
async fn hits_the_app_holds_are_marked_and_nothing_else_changes() {
    let tools = tools(vec![common::source()]);
    // "Deep learning" by its OpenAlex id, back-propagation by its DOI.
    let library = Library::holding(&["openalex:W1", "doi:10.1038/323533a0"]);
    for (name, args, held) in [
        ("search", json!({"text": "Deep learning"}), true),
        ("references", json!({"id": "10.1038/nature14539"}), true),
        ("citing_works", json!({"id": "10.1038/nature14539"}), false),
    ] {
        let (marked, plain) = both(&tools, &library, name, args).await;
        let schema = Value::Object(tools.spec(name).unwrap().output_schema.clone());
        common::conforms(&marked.structured, &schema, name);
        let hit = &marked.structured["hits"][0];
        assert_eq!(
            hit.get("held"),
            held.then_some(&Value::Bool(true)),
            "{name}: {hit}"
        );
        assert!(plain.structured["hits"][0].get("held").is_none(), "{name}");
        assert_eq!(
            unmarked(marked.structured),
            unmarked(plain.structured),
            "{name}"
        );
        // Said in the text too, briefly; absent when none is held.
        assert_eq!(
            marked.summary.contains(" (held)"),
            held,
            "{}",
            marked.summary
        );
        assert_eq!(
            marked
                .summary
                .contains("\n1 already in the app's library (held).\n"),
            held,
            "{}",
            marked.summary
        );
        assert!(!plain.summary.contains("held"), "{}", plain.summary);
    }
    // Once a page, by every identifier of each work, the canonical first.
    assert_eq!(
        library.asked(),
        [
            vec![ids(&["doi:10.1038/nature14539", "openalex:W1"])],
            vec![ids(&["doi:10.1038/323533a0", "openalex:W3"])],
            vec![ids(&["arxiv:1706.03762", "openalex:W2"])],
        ]
    );
}

#[tokio::test]
async fn a_page_with_no_hits_asks_nothing() {
    let tools = tools(vec![common::source()]);
    let library = Library::holding(&["openalex:W1"]);
    let (marked, _) = both(
        &tools,
        &library,
        "search",
        json!({"text": "Nothing like it"}),
    )
    .await;
    assert_eq!(marked.structured["hits"], json!([]));
    assert!(library.asked().is_empty());
}

/// `record`, citing `references` (OpenAlex ids).
fn citing(mut record: SourceRecord, references: &[&str]) -> SourceRecord {
    record.references = Some(
        references
            .iter()
            .map(|r| r.parse::<WorkId>().unwrap())
            .collect(),
    );
    record
}

#[tokio::test]
async fn a_missing_work_is_marked_when_held_under_an_id_the_result_cuts() {
    // Both seeds cite back-propagation, which the set lacks: it's probably
    // missing. The analysis knows it by its OpenAlex id, the lean result
    // keeps its DOI, and the app holds it by its PMID alone.
    let backprop = common::work(
        &["10.1038/323533a0", "W3", "pmid:3409"],
        "Learning representations by back-propagating errors",
        &["David E. Rumelhart"],
        1986,
    );
    let source = Arc::new(Scripted::new(
        "openalex",
        vec![
            citing(deep_learning(), &["W3"]),
            citing(attention(), &["W3"]),
            backprop,
        ],
    ));
    let tools = tools(vec![source]);
    let library = Library::holding(&["pmid:3409"]);
    let args = json!({"ids": ["W1", "W2"], "kinds": ["probably_missing", "related"]});
    let (marked, plain) = both(&tools, &library, "analyse", args).await;
    let missing = &marked.structured["probably_missing"]["items"];
    assert_eq!(missing[0]["id"], "openalex:W3", "{}", marked.structured);
    assert_eq!(missing[0]["held"], true);
    // Cut down all the same, after it was asked about.
    assert_eq!(
        missing[0]["work"],
        json!({"canonical_id": "doi:10.1038/323533a0",
            "title": "Learning representations by back-propagating errors",
            "year": 1986})
    );
    assert!(
        plain.structured["probably_missing"]["items"][0]
            .get("held")
            .is_none()
    );
    assert_eq!(
        unmarked(marked.structured.clone()),
        unmarked(plain.structured)
    );
    assert!(
        marked
            .summary
            .contains("Learning representations by back-propagating errors (1986)"),
        "{}",
        marked.summary
    );
    assert!(
        marked.summary.contains(" (held), cited by seeds 0, 1"),
        "{}",
        marked.summary
    );
    // One question for the whole result: seeds aren't asked about.
    let asked = library.asked();
    assert_eq!(asked.len(), 1, "{asked:?}");
    assert_eq!(
        asked[0][0],
        ids(&["doi:10.1038/323533a0", "openalex:W3", "pmid:3409"])
    );

    // An estimate lists no works: nothing is asked.
    let library = Library::holding(&["pmid:3409"]);
    let estimate = json!({"ids": ["W1", "W2"], "estimate_only": true});
    both(&tools, &library, "analyse", estimate).await;
    assert!(library.asked().is_empty());
}
