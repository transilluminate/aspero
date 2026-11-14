//! Citation links: a page of a work's references, and analyses of a set.
//!
//! Works are lean (or full) `work` objects; the works of a set are named by
//! their index in `seeds`. Anything stopped early says so: `complete` is
//! false, and `incomplete` gives a kind (`capped`, `budget`,
//! `provider_failed`) and how far it got (`done` of `total`).

use aspero_engine::{
    Analysis, Estimate, Incomplete, Missing, OddOne, ReferencesPage, RelatedWork,
    RetractedReference, Section, Seed,
};
use serde_json::{Map, Value, json};

use crate::{lookup, work};

/// A page of references: the work, whether its references are known, how
/// many, this page of them (each a lookup item), and the next page's
/// cursor.
pub fn references(input: &str, page: &ReferencesPage, full: bool) -> Value {
    let mut out = Map::new();
    out.insert("input".into(), json!(input));
    out.insert("id".into(), json!(page.work));
    if let Some(record) = &page.record {
        out.insert("work".into(), work::work(record, full));
    }
    out.insert("coverage".into(), json!(page.coverage.as_str()));
    if let Some(total) = page.total {
        out.insert("total".into(), json!(total));
    }
    let items: Vec<Value> = page
        .references
        .iter()
        .map(|r| {
            let mut item = lookup::item(&r.id.to_string(), r, full);
            if let Value::Object(fields) = &mut item {
                fields.remove("input");
            }
            item
        })
        .collect();
    out.insert("references".into(), json!(items));
    if let Some(cursor) = &page.next_cursor {
        out.insert("next_cursor".into(), json!(cursor.0));
    }
    out.insert("usage".into(), work::usage(&page.usage));
    Value::Object(out)
}

/// The analyses: every seed, then each analysis asked for (related works
/// last), kinds not built yet, an estimate (with `estimate_only`),
/// `complete`, and the usage.
pub fn analysis(a: &Analysis, full: bool) -> Value {
    let mut out = Map::new();
    let seeds: Vec<Value> = a
        .seeds
        .iter()
        .enumerate()
        .map(|(i, s)| seed(i, s, full))
        .collect();
    out.insert("seeds".into(), json!(seeds));
    out.insert("with_references".into(), json!(a.with_references));
    if let Some(found) = &a.retracted_references {
        out.insert(
            "retracted_references".into(),
            section(found, |r| retracted(r, full)),
        );
    }
    if let Some(found) = &a.odd_one_out {
        out.insert("odd_one_out".into(), section(found, odd_one));
    }
    if let Some(found) = &a.probably_missing {
        out.insert(
            "probably_missing".into(),
            section(found, |m| missing(m, full)),
        );
    }
    if let Some(found) = &a.related {
        out.insert("related".into(), section(found, |r| related(r, full)));
    }
    if !a.not_yet.is_empty() {
        let kinds: Vec<&str> = a.not_yet.iter().map(|k| k.as_str()).collect();
        out.insert("not_yet".into(), json!(kinds));
    }
    if let Some(e) = &a.estimate {
        out.insert("estimate".into(), estimate(e));
    }
    out.insert("complete".into(), json!(a.complete()));
    if let Some(i) = &a.incomplete {
        out.insert("incomplete".into(), incomplete(i));
    }
    out.insert("usage".into(), work::usage(&a.usage));
    Value::Object(out)
}

/// The analyses' sections as JSON names them, and as JSON lines tag them.
const SECTIONS: [(&str, &str); 4] = [
    ("retracted_references", "retracted"),
    ("odd_one_out", "odd_one_out"),
    ("probably_missing", "probably_missing"),
    ("related", "related"),
];

/// An [`analysis`] as JSON lines: each seed (`"section": "seed"`), then
/// each item of each analysis tagged with its section (`retracted`,
/// `odd_one_out`, `probably_missing`, `related`), then a `summary`: the
/// rest of the document (`with_references`, `complete`, `incomplete`,
/// `estimate`, `not_yet`, `usage`), with each analysis' own `complete`
/// and `incomplete` under its name.
pub fn analysis_lines(doc: &Value) -> Vec<Value> {
    let tagged = |section: &str, item: &Value| {
        let mut line = Map::new();
        line.insert("section".into(), json!(section));
        if let Value::Object(fields) = item {
            line.extend(fields.clone());
        }
        Value::Object(line)
    };
    let mut lines: Vec<Value> = doc["seeds"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|s| tagged("seed", s))
        .collect();
    let mut summary = Map::new();
    summary.insert("section".into(), json!("summary"));
    for (key, section) in SECTIONS {
        let Some(found) = doc.get(key) else {
            continue;
        };
        for item in found["items"].as_array().into_iter().flatten() {
            lines.push(tagged(section, item));
        }
        let mut status = found.clone();
        if let Value::Object(fields) = &mut status {
            fields.remove("items");
        }
        summary.insert(key.into(), status);
    }
    if let Value::Object(fields) = doc {
        for (key, value) in fields {
            if key != "seeds" && !SECTIONS.iter().any(|(k, _)| k == key) {
                summary.insert(key.clone(), value.clone());
            }
        }
    }
    lines.push(Value::Object(summary));
    lines
}

/// Names a bibliography's seeds by their entries' `keys` (one per seed, in
/// order), and adds the entries without an identifier as skipped seeds.
pub fn file_seeds(doc: &mut Value, keys: &[&str], skipped: &[&str]) {
    let Some(seeds) = doc["seeds"].as_array_mut() else {
        return;
    };
    for (seed, key) in seeds.iter_mut().zip(keys) {
        seed["key"] = json!(key);
    }
    let first = seeds.len();
    for (i, key) in skipped.iter().enumerate() {
        seeds.push(json!({
            "index": first + i,
            "key": key,
            "status": "skipped",
            "reason": "no_identifier",
        }));
    }
}

/// One seed: resolved (its OpenAlex `id`, `references` as a count or
/// `"unknown"`, the `work`, and `same_as` an earlier seed that is the
/// same work), or unresolved with a `reason` and an `error`.
pub fn seed(index: usize, s: &Seed, full: bool) -> Value {
    let mut out = Map::new();
    out.insert("index".into(), json!(index));
    out.insert("input".into(), json!(s.input));
    match (&s.record, s.problem) {
        (Some(record), _) => {
            out.insert("status".into(), json!("resolved"));
            out.insert("id".into(), json!(s.work()));
            let references = match s.references() {
                Some(cited) => json!(cited.len()),
                None => json!("unknown"),
            };
            out.insert("references".into(), references);
            if let Some(first) = s.same_as {
                out.insert("same_as".into(), json!(first));
            }
            out.insert("work".into(), work::work(record, full));
        }
        (None, problem) => {
            out.insert("status".into(), json!("unresolved"));
            out.insert("reason".into(), json!(problem.map(|p| p.as_str())));
            if let Some(message) = &s.message {
                out.insert("error".into(), json!(message));
            }
        }
    }
    Value::Object(out)
}

/// An analysis' findings: `complete`, `incomplete` when not, and `items`.
fn section<T>(s: &Section<T>, item: impl Fn(&T) -> Value) -> Value {
    let mut out = Map::new();
    out.insert("complete".into(), json!(s.complete()));
    if let Some(i) = &s.incomplete {
        out.insert("incomplete".into(), incomplete(i));
    }
    out.insert("items".into(), s.items.iter().map(item).collect());
    Value::Object(out)
}

/// `{kind, done, total}`: what stopped a step, and how far it got; with
/// `stage` (related works: `citing`, `coupling` or `lookup`, which `done`
/// and `total` then count: citing works, pages, works) and the `provider`
/// that failed, when known.
pub fn incomplete(i: &Incomplete) -> Value {
    let mut out = json!({"kind": i.kind.as_str(), "done": i.done, "total": i.total});
    if let Some(stage) = i.stage {
        out["stage"] = json!(stage.as_str());
    }
    if let Some(provider) = &i.provider {
        out["provider"] = json!(provider.as_str());
    }
    out
}

pub fn estimate(e: &Estimate) -> Value {
    json!({"requests": e.requests, "usd": e.usd, "exact": e.exact})
}

fn retracted(r: &RetractedReference, full: bool) -> Value {
    json!({"seed": r.seed, "cites": work::work(&r.cites, full), "source": r.source})
}

fn odd_one(o: &OddOne) -> Value {
    json!({"seed": o.seed, "reason": o.reason.as_str()})
}

fn missing(m: &Missing, full: bool) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(m.id));
    if m.ids.len() > 1 {
        out.insert("ids".into(), json!(m.ids));
    }
    if let Some(record) = &m.record {
        out.insert("work".into(), work::work(record, full));
    }
    out.insert("cited_by".into(), json!(m.cited_by));
    out.insert("count".into(), json!(m.count()));
    Value::Object(out)
}

/// A related work: its `score` (0 to 1), and why: `co_cited_with` (citing
/// works read that cite it and the set), `shared_references` (the set's
/// references it cites too) and the `seeds` linking it.
fn related(r: &RelatedWork, full: bool) -> Value {
    let mut out = Map::new();
    out.insert("id".into(), json!(r.id));
    if r.ids.len() > 1 {
        out.insert("ids".into(), json!(r.ids));
    }
    if let Some(record) = &r.record {
        out.insert("work".into(), work::work(record, full));
    }
    out.insert("score".into(), json!(r.score));
    out.insert("co_cited_with".into(), json!(r.co_cited_with));
    out.insert("shared_references".into(), json!(r.shared_references));
    out.insert("seeds".into(), json!(r.seeds));
    Value::Object(out)
}

#[cfg(test)]
mod tests {
    use aspero_core::ProviderId;
    use aspero_engine::{IncompleteKind, Stage};

    use super::*;

    #[test]
    fn incomplete_steps_say_how_far_they_got() {
        let i = Incomplete::new(IncompleteKind::Budget, 1200, 4300);
        assert_eq!(
            incomplete(&i),
            json!({"kind": "budget", "done": 1200, "total": 4300})
        );
        let lookup = Incomplete::new(IncompleteKind::ProviderFailed, 0, 100)
            .at(Stage::Lookup)
            .from(ProviderId::from_static("openalex"));
        assert_eq!(
            incomplete(&lookup),
            json!({"kind": "provider_failed", "stage": "lookup", "provider": "openalex", "done": 0, "total": 100})
        );
    }

    #[test]
    fn analyses_as_lines_tag_each_item_and_end_with_a_summary() {
        let doc = json!({
            "seeds": [{"index": 0, "input": "10.1/a", "status": "resolved"}],
            "with_references": 1,
            "odd_one_out": {"complete": true, "items": []},
            "related": {
                "complete": false,
                "incomplete": {"kind": "capped", "stage": "citing", "done": 200, "total": 900},
                "items": [{"id": "openalex:W1", "score": 1.0}, {"id": "openalex:W2", "score": 0.5}],
            },
            "complete": false,
            "usage": {"openalex": {"cost": 0.0002}},
        });
        assert_eq!(
            analysis_lines(&doc),
            [
                json!({"section": "seed", "index": 0, "input": "10.1/a", "status": "resolved"}),
                json!({"section": "related", "id": "openalex:W1", "score": 1.0}),
                json!({"section": "related", "id": "openalex:W2", "score": 0.5}),
                json!({
                    "section": "summary",
                    "with_references": 1,
                    "odd_one_out": {"complete": true},
                    "related": {
                        "complete": false,
                        "incomplete": {"kind": "capped", "stage": "citing", "done": 200, "total": 900},
                    },
                    "complete": false,
                    "usage": {"openalex": {"cost": 0.0002}},
                }),
            ]
        );
    }

    #[test]
    fn file_seeds_are_named_and_skipped_entries_listed() {
        let mut doc = json!({"seeds": [{"index": 0, "input": "10.1/x"}]});
        file_seeds(&mut doc, &["smith2020"], &["nodoi2021"]);
        assert_eq!(
            doc["seeds"],
            json!([
                {"index": 0, "input": "10.1/x", "key": "smith2020"},
                {"index": 1, "key": "nodoi2021", "status": "skipped", "reason": "no_identifier"},
            ])
        );
    }
}
