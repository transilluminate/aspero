//! Works the embedding app already holds (a reference manager's library):
//! asked once per result, so `search`, `citing_works` and `references`
//! hits, and `analyse`'s probably missing and related works, can say so
//! (`held: true`). An agent then doesn't propose, add or fetch what the
//! user has.

use futures::future::BoxFuture;
use serde_json::Value;

/// What the app holds, asked by identifier: a local call, no network and
/// no budget.
pub trait Holdings: Send + Sync {
    /// For each work, given by every identifier its result carries as
    /// aspero writes them (`doi:10.1097/…`, `pmid:…`, `openalex:W…`, the
    /// canonical first), whether the app holds it under any of them: one
    /// answer per work, in order. `None` when the app can't say: nothing
    /// is marked.
    fn held(&self, works: Vec<Vec<String>>) -> BoxFuture<'_, Option<Vec<bool>>>;
}

/// The analyses whose items are works.
const ANALYSES: [&str; 2] = ["probably_missing", "related"];

/// Marks the works in `v`, tool `tool`'s result, that `holdings` holds
/// with `held: true`, asking once for all of them; says how many it
/// marked. Run before works are cut down, so every identifier is asked.
pub(crate) async fn mark(tool: &str, v: &mut Value, holdings: &dyn Holdings) -> usize {
    let mut items = items(tool, v);
    if items.is_empty() {
        return 0;
    }
    let asked: Vec<Vec<String>> = items.iter().map(|i| ids(i)).collect();
    let Some(held) = holdings.held(asked).await else {
        return 0;
    };
    let mut marked = 0;
    for (item, held) in items.iter_mut().zip(held) {
        if let Some(map) = item.as_object_mut().filter(|_| held) {
            map.insert("held".into(), Value::Bool(true));
            marked += 1;
        }
    }
    marked
}

/// The items of `v` that are works: a page's hits, or the analyses'
/// items.
fn items<'a>(tool: &str, v: &'a mut Value) -> Vec<&'a mut Value> {
    let Some(map) = v.as_object_mut() else {
        return Vec::new();
    };
    let lists: Vec<&mut Value> = match tool {
        "search" | "citing_works" | "references" => map.get_mut("hits").into_iter().collect(),
        "analyse" => map
            .iter_mut()
            .filter(|(k, _)| ANALYSES.contains(&k.as_str()))
            .filter_map(|(_, section)| section.get_mut("items"))
            .collect(),
        _ => Vec::new(),
    };
    lists
        .into_iter()
        .filter_map(Value::as_array_mut)
        .flatten()
        .collect()
}

/// Every identifier an item carries, once each: its work's canonical id
/// and ids, then an analysis' OpenAlex `id` and merged `ids`.
fn ids(item: &Value) -> Vec<String> {
    let work = &item["work"];
    let listed = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    let all = [work["canonical_id"].clone()]
        .into_iter()
        .chain(listed(&work["ids"]))
        .chain([item["id"].clone()])
        .chain(listed(&item["ids"]));
    for id in all {
        if let Some(id) = id.as_str().map(str::trim).filter(|s| !s.is_empty())
            && !out.iter().any(|o| o == id)
        {
            out.push(id.to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use serde_json::json;

    use super::*;

    /// Holds the works with any of its ids; keeps what it was asked.
    struct Library {
        ids: Vec<&'static str>,
        asked: Mutex<Vec<Vec<Vec<String>>>>,
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

    fn library(ids: &[&'static str]) -> Library {
        Library {
            ids: ids.to_vec(),
            asked: Mutex::default(),
        }
    }

    #[tokio::test]
    async fn an_analysis_asks_once_by_every_id_of_its_works() {
        // As `aspero_json::graph::analysis` writes it, before works are cut.
        let mut doc = json!({
            "seeds": [{"index": 0, "work": {"canonical_id": "doi:10.1/seed"}}],
            "probably_missing": {"items": [
                {"id": "openalex:W1", "work": {"canonical_id": "doi:10.1/a",
                    "ids": ["doi:10.1/a", "pmid:11", "openalex:W1"]}},
                {"id": "openalex:W2", "ids": ["openalex:W2", "openalex:W9"]},
            ]},
            "related": {"items": [{"id": "openalex:W3"}]},
            "odd_one_out": {"items": [{"seed": 0, "reason": "no_links"}]},
        });
        // Held by its PMID, by a merged version's id, and not at all.
        let library = library(&["pmid:11", "openalex:W9"]);
        assert_eq!(mark("analyse", &mut doc, &library).await, 2);
        assert_eq!(
            library.asked.lock().unwrap().as_slice(),
            [vec![
                vec!["doi:10.1/a", "pmid:11", "openalex:W1"],
                vec!["openalex:W2", "openalex:W9"],
                vec!["openalex:W3"],
            ]]
        );
        assert_eq!(doc["probably_missing"]["items"][0]["held"], true);
        assert_eq!(doc["probably_missing"]["items"][1]["held"], true);
        // Absent, not false; seeds and odd ones aren't asked about.
        assert!(doc["related"]["items"][0].get("held").is_none());
        assert!(doc["seeds"][0].get("held").is_none());
        assert!(doc["odd_one_out"]["items"][0].get("held").is_none());
    }

    #[tokio::test]
    async fn nothing_is_asked_without_works_or_marked_when_the_app_cant_say() {
        struct Silent;
        impl Holdings for Silent {
            fn held(&self, _: Vec<Vec<String>>) -> BoxFuture<'_, Option<Vec<bool>>> {
                Box::pin(async { None })
            }
        }
        let mut page = json!({"hits": [{"rank": 1, "work": {"ids": ["doi:10.1/a"]}}]});
        let before = page.clone();
        assert_eq!(mark("search", &mut page, &Silent).await, 0);
        assert_eq!(page, before);

        let library = library(&["doi:10.1/a"]);
        let mut empty = json!({"hits": []});
        assert_eq!(mark("citing_works", &mut empty, &library).await, 0);
        let mut estimate = json!({"seeds": [], "estimate": {"requests": 1}});
        assert_eq!(mark("analyse", &mut estimate, &library).await, 0);
        // Not a tool whose results are marked.
        let mut lookup = json!({"results": [{"work": {"ids": ["doi:10.1/a"]}}]});
        assert_eq!(mark("lookup", &mut lookup, &library).await, 0);
        assert!(library.asked.lock().unwrap().is_empty());
    }
}
