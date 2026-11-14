//! Works, candidates and usage.

use aspero_core::{Person, ProviderId, SourceRecord, Usage};
use aspero_engine::{Abstract, Candidate};
use serde_json::{Map, Value, json};

/// One work: lean by default, every known field with `full`.
pub fn work(record: &SourceRecord, full: bool) -> Value {
    let w = &record.work;
    let mut out = Map::new();
    out.insert("source".into(), json!(record.source));
    out.insert("ids".into(), json!(record.ids));
    // One key per work, whichever identifier found it: for deduping.
    if let Some(id) = aspero_core::canonical(&record.ids) {
        out.insert("canonical".into(), json!(id));
    }
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(v) = value {
            out.insert(key.into(), v);
        }
    };
    put("title", w.title.as_ref().map(|t| json!(t)));
    if !w.authors.is_empty() {
        let authors: Vec<Value> = w.authors.iter().filter_map(|a| author(a, full)).collect();
        put("authors", Some(json!(authors)));
    }
    put("issued", w.issued.map(|d| json!(d.to_string())));
    put("type", w.work_type.as_ref().map(|t| json!(t.as_csl())));
    put("container", w.container_title.as_ref().map(|c| json!(c)));
    // Often what tells near-identical hits apart.
    put("cited_by_count", w.cited_by_count.map(|c| json!(c)));
    put("url", w.url.as_ref().map(|u| json!(u)));
    // Never lean enough to leave out: a retracted work is not citable.
    put("updates", (!w.updates.is_empty()).then(|| json!(w.updates)));
    put(
        "update_to",
        (!w.update_to.is_empty()).then(|| json!(w.update_to)),
    );
    // Whether a free copy can be fetched instead of asking a human.
    put("open_access", w.open_access.as_ref().map(|oa| json!(oa)));
    if full {
        // Everything else, in the core `Work` shape (structured authors).
        if let Ok(Value::Object(all)) = serde_json::to_value(w) {
            for (k, v) in all {
                out.entry(k).or_insert(v);
            }
        }
        out.insert("fetched_at".into(), json!(record.fetched_at.to_string()));
    }
    Value::Object(out)
}

/// One author, the same shape lean or full: always a `name`, then the ORCID
/// iD, and with `full` the name's parts.
fn author(person: &Person, full: bool) -> Option<Value> {
    let mut out = Map::new();
    out.insert("name".into(), json!(person.display_name()?));
    if full && let Ok(Value::Object(parts)) = serde_json::to_value(person) {
        out.extend(parts);
    } else if let Some(orcid) = &person.orcid {
        out.insert("orcid".into(), json!(orcid));
    }
    Some(Value::Object(out))
}

pub fn usage(usage: &[(ProviderId, Usage)]) -> Value {
    let map: Map<String, Value> = usage
        .iter()
        .map(|(p, u)| (p.to_string(), usage_one(u)))
        .collect();
    Value::Object(map)
}

pub fn usage_one(u: &Usage) -> Value {
    let mut out = Map::new();
    if let Some(c) = u.cost {
        out.insert("cost".into(), json!(c));
    }
    if let Some(r) = u.remaining {
        out.insert("remaining".into(), json!(r));
    }
    if let Some(t) = u.resets_at {
        out.insert("resets_at".into(), json!(t.to_string()));
    }
    if let Some(l) = u.limit {
        out.insert("limit".into(), json!(l));
    }
    if let Some(p) = u.prepaid_remaining {
        out.insert("prepaid_remaining".into(), json!(p));
    }
    Value::Object(out)
}

/// Sets a work's `abstract` (null when none was found) and
/// `abstract_source`.
pub fn set_abstract(work: &mut Value, found: Option<&Abstract>) {
    match found {
        Some(a) => {
            work["abstract"] = json!(a.text);
            work["abstract_source"] = json!(a.source);
        }
        None => work["abstract"] = Value::Null,
    }
}

/// One search hit: its 1-based `rank`, the source's own `score` (left out
/// when the source gives none, as OpenAlex doesn't when sorting), and the
/// work.
pub fn hit(rank: usize, score: Option<f64>, work: Value) -> Value {
    let mut out = json!({"rank": rank});
    if let Some(score) = score {
        out["score"] = json!(score);
    }
    out["work"] = work;
    out
}

/// A scored candidate: confidence, per-field checks, who found it, and the
/// work (with every identifier merged records asserted).
pub fn candidate(c: &Candidate, full: bool) -> Value {
    let a = &c.assessment;
    let checks = if full {
        json!({
            "title": a.title,
            "authors": a.authors,
            "year": a.year,
            "container": a.container,
            "volume": a.volume,
            "page": a.page,
            "ids": a.ids,
        })
    } else {
        json!({
            "title": a.title.agreement,
            "authors": a.authors.agreement,
            "year": a.year.agreement,
            "container": a.container.agreement,
            "volume": a.volume.agreement,
            "page": a.page.agreement,
            "ids": a.ids,
        })
    };
    let found_by: Vec<Value> = c
        .sightings
        .iter()
        .map(|s| {
            let mut out = json!({"provider": s.provider, "via": s.via.as_str(), "rank": s.rank});
            if let Some(score) = s.score {
                out["score"] = json!(score);
            }
            out
        })
        .collect();
    let mut work = work(&c.merged_record(), full);
    // Always there, null without identifiers.
    work["canonical"] = json!(aspero_core::canonical(&c.ids));
    json!({
        "confidence": (a.confidence * 100.0).round() / 100.0,
        "checks": checks,
        "found_by": found_by,
        "work": work,
    })
}

#[cfg(test)]
mod tests {
    use aspero_core::{FieldSet, PartialDate, Raw, Work, WorkType};
    use jiff::Timestamp;

    use super::*;

    fn record() -> SourceRecord {
        let mut work = Work::default();
        work.title = Some("Deep learning".into());
        work.authors = [
            "Yann LeCun",
            "Yoshua Bengio",
            "Geoffrey Hinton",
            "A. N. Other",
        ]
        .map(Person::literal)
        .to_vec();
        work.issued = PartialDate::new(2015, Some(5), Some(27));
        work.work_type = Some(WorkType::JournalArticle);
        work.container_title = Some("Nature".into());
        work.abstract_text = Some("Deep learning allows…".into());
        let raw = Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        };
        SourceRecord::new(
            ProviderId::from_static("crossref"),
            vec!["10.1038/nature14539".parse().unwrap()],
            work,
            FieldSet::ALL,
            raw,
            Timestamp::UNIX_EPOCH,
        )
    }

    #[test]
    fn lean_json_identifies_and_cites_without_extras() {
        assert_eq!(
            work(&record(), false),
            json!({
                "source": "crossref",
                "ids": ["doi:10.1038/nature14539"],
                "canonical": "doi:10.1038/nature14539",
                "title": "Deep learning",
                "authors": [
                    {"name": "Yann LeCun"},
                    {"name": "Yoshua Bengio"},
                    {"name": "Geoffrey Hinton"},
                    {"name": "A. N. Other"},
                ],
                "issued": "2015-05-27",
                "type": "article-journal",
                "container": "Nature",
            })
        );
    }

    #[test]
    fn a_hit_without_a_score_leaves_it_out() {
        let w = json!({"title": "Deep learning"});
        assert_eq!(
            hit(1, None, w.clone()),
            json!({"rank": 1, "work": {"title": "Deep learning"}})
        );
        assert_eq!(
            hit(2, Some(12.5), w),
            json!({"rank": 2, "score": 12.5, "work": {"title": "Deep learning"}})
        );
    }

    #[test]
    fn full_json_adds_everything() {
        let full = work(&record(), true);
        assert_eq!(full["abstract"], "Deep learning allows…");
        assert_eq!(full["authors"][0]["literal"], "Yann LeCun");
        assert!(full.get("fetched_at").is_some());
    }

    #[test]
    fn authors_have_one_shape_lean_or_full() {
        let mut r = record();
        let mut lecun = Person::new("Yann", "LeCun");
        lecun.orcid = Some("0000-0000-0000-0001".into());
        r.work.authors = vec![lecun];
        assert_eq!(
            work(&r, false)["authors"],
            json!([{"name": "Yann LeCun", "orcid": "0000-0000-0000-0001"}])
        );
        assert_eq!(
            work(&r, true)["authors"],
            json!([{
                "name": "Yann LeCun",
                "given": "Yann",
                "family": "LeCun",
                "orcid": "0000-0000-0000-0001",
            }])
        );
    }
}
