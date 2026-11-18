//! A verification's reason as stored data: written as JSON
//! ([`Verification::reason_json`](super::Verification::reason_json)) and
//! read back typed ([`reason_from_json`]), so an app can keep why a verdict
//! was reached and word it when it's shown, in its own words and language.
//!
//! The shape is the JSON contract's `reason` with its version `v`:
//! `{"v": 1, "code", "differs": [{"field", "actual", "cited", "kind"}],
//! "cited": candidate, "why"}`. Cases are only ever added; a reader gives
//! `None` for a code or version it doesn't know, never an error.

use aspero_core::matching::WorkKind as CoreWorkKind;
use aspero_core::{Agreement as CoreAgreement, FieldSet, ProviderId, Raw, SourceRecord, WorkId};
use aspero_engine as engine;
use jiff::Timestamp;
use serde_json::{Map, Value, json};

use super::{Agreement, Candidate, Checks, CitedField, Difference, Reason, Work, WorkKind};

/// The stored reason's version. Readers take this and older, and give
/// `None` for newer.
pub const REASON_JSON_VERSION: u64 = 1;

impl Reason {
    /// The contract's name for the case: `partly_matches`, `id_elsewhere`, ….
    fn code(&self) -> &'static str {
        match self {
            Reason::Matches => "matches",
            Reason::PartlyMatches { .. } => "partly_matches",
            Reason::Unconfirmed => "unconfirmed",
            Reason::Closest { .. } => "closest",
            Reason::NoMatch => "no_match",
            Reason::IdExists => "id_exists",
            Reason::IdAgrees => "id_agrees",
            Reason::IdPartlyMatches { .. } => "id_partly_matches",
            Reason::IdElsewhere { .. } => "id_elsewhere",
            Reason::IdDisagrees => "id_disagrees",
            Reason::IdMissingElsewhere => "id_missing_elsewhere",
            Reason::IdMissing => "id_missing",
            Reason::ProvidersFailed => "providers_failed",
            Reason::InvalidClaim { .. } => "invalid_claim",
        }
    }

    /// The reason as stored ([`REASON_JSON_VERSION`]).
    pub(crate) fn to_json(&self) -> String {
        let mut out = Map::new();
        out.insert("v".into(), json!(REASON_JSON_VERSION));
        out.insert("code".into(), json!(self.code()));
        match self {
            Reason::PartlyMatches { differs }
            | Reason::Closest { differs }
            | Reason::IdPartlyMatches { differs } => {
                let differs: Vec<Value> = differs.iter().map(difference_json).collect();
                out.insert("differs".into(), json!(differs));
            }
            Reason::IdElsewhere { cited } => {
                out.insert("cited".into(), candidate_json(cited));
            }
            Reason::InvalidClaim { why } => {
                out.insert("why".into(), json!(why));
            }
            _ => {}
        }
        Value::Object(out).to_string()
    }
}

/// A stored reason ([`Verification::reason_json`](super::Verification::reason_json))
/// read back. `None` for a code or version this aspero doesn't know, or
/// JSON that isn't a reason; never an error, so a stored reason always
/// loads. A difference whose field it doesn't know is left out.
#[uniffi::export]
#[allow(clippy::needless_pass_by_value)]
pub fn reason_from_json(json: String) -> Option<Reason> {
    let v: Value = serde_json::from_str(&json).ok()?;
    let version = match v.get("v") {
        Some(n) => n.as_u64()?,
        None => 1,
    };
    if version > REASON_JSON_VERSION {
        return None;
    }
    let differs = || -> Vec<Difference> {
        v.get("differs")
            .and_then(Value::as_array)
            .map(|ds| ds.iter().filter_map(difference_from).collect())
            .unwrap_or_default()
    };
    Some(match v.get("code")?.as_str()? {
        "matches" => Reason::Matches,
        "partly_matches" => Reason::PartlyMatches { differs: differs() },
        "unconfirmed" => Reason::Unconfirmed,
        "closest" => Reason::Closest { differs: differs() },
        "no_match" => Reason::NoMatch,
        "id_exists" => Reason::IdExists,
        "id_agrees" => Reason::IdAgrees,
        "id_partly_matches" => Reason::IdPartlyMatches { differs: differs() },
        "id_elsewhere" => Reason::IdElsewhere {
            cited: candidate_from(v.get("cited")?)?,
        },
        "id_disagrees" => Reason::IdDisagrees,
        "id_missing_elsewhere" => Reason::IdMissingElsewhere,
        "id_missing" => Reason::IdMissing,
        "providers_failed" => Reason::ProvidersFailed,
        "invalid_claim" => Reason::InvalidClaim {
            why: text(&v, "why").unwrap_or_default(),
        },
        _ => return None,
    })
}

/// A work read back from its full JSON ([`Work::json`]). `None` for JSON
/// that isn't a work, or lacks the full form's `source` and `fetched_at`.
#[uniffi::export]
#[allow(clippy::needless_pass_by_value)]
pub fn work_from_json(json: String) -> Option<Work> {
    work_from(&serde_json::from_str(&json).ok()?)
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn field_name(field: CitedField) -> &'static str {
    engine::CitedField::ALL
        .iter()
        .find(|f| CitedField::from(**f) == field)
        .map_or("unknown", |f| f.as_str())
}

/// A work kind's name: `reply`, `expression_of_concern`, ….
fn kind_name(kind: CoreWorkKind) -> String {
    kind.as_str().replace(' ', "_")
}

fn difference_json(d: &Difference) -> Value {
    let mut out = json!({"field": field_name(d.field)});
    if let Some(actual) = &d.actual {
        out["actual"] = json!(actual);
    }
    if let Some(cited) = &d.cited {
        out["cited"] = json!(cited);
    }
    if let Some(kind) = d.kind {
        let core = CoreWorkKind::ALL
            .iter()
            .find(|k| WorkKind::from(**k) == kind);
        if let Some(core) = core {
            out["kind"] = json!(kind_name(*core));
        }
    }
    out
}

fn difference_from(d: &Value) -> Option<Difference> {
    let name = d.get("field")?.as_str()?;
    let field = engine::CitedField::ALL
        .iter()
        .find(|f| f.as_str() == name)
        .map(|f| CitedField::from(*f))?;
    let kind = d.get("kind").and_then(Value::as_str).and_then(|name| {
        CoreWorkKind::ALL
            .iter()
            .find(|k| kind_name(**k) == name)
            .map(|k| WorkKind::from(*k))
    });
    Some(Difference {
        field,
        actual: text(d, "actual"),
        cited: text(d, "cited"),
        kind,
    })
}

fn agreement_name(a: Agreement) -> &'static str {
    match a {
        Agreement::Match => "match",
        Agreement::Partial => "partial",
        Agreement::Mismatch => "mismatch",
        Agreement::Unknown => "unknown",
    }
}

/// The contract's candidate as the app has it: confidence unrounded, each
/// check's agreement, the providers that found it, and the full work.
fn candidate_json(c: &Candidate) -> Value {
    let k = &c.checks;
    let found_by: Vec<Value> = c.found_by.iter().map(|p| json!({"provider": p})).collect();
    json!({
        "confidence": c.confidence,
        "checks": {
            "title": agreement_name(k.title),
            "authors": agreement_name(k.authors),
            "year": agreement_name(k.year),
            "container": agreement_name(k.container),
            "volume": agreement_name(k.volume),
            "page": agreement_name(k.page),
            "ids": agreement_name(k.ids),
        },
        "found_by": found_by,
        "work": serde_json::from_str::<Value>(&c.work.json).unwrap_or(Value::Null),
    })
}

/// A check lean (`"match"`) or full (`{"agreement": "match", "score"}`);
/// unknown when missing.
fn agreement_from(checks: Option<&Value>, key: &str) -> Agreement {
    let check = checks.and_then(|c| c.get(key));
    let value = check.and_then(|c| c.get("agreement")).or(check);
    value
        .and_then(|v| serde_json::from_value::<CoreAgreement>(v.clone()).ok())
        .map_or(Agreement::Unknown, Agreement::from)
}

fn candidate_from(c: &Value) -> Option<Candidate> {
    let checks = c.get("checks");
    let mut found_by: Vec<String> = Vec::new();
    for s in c
        .get("found_by")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let provider = s.get("provider").unwrap_or(s).as_str();
        if let Some(p) = provider
            && !found_by.iter().any(|f| f == p)
        {
            found_by.push(p.to_owned());
        }
    }
    Some(Candidate {
        confidence: c.get("confidence")?.as_f64()?,
        checks: Checks {
            title: agreement_from(checks, "title"),
            authors: agreement_from(checks, "authors"),
            year: agreement_from(checks, "year"),
            container: agreement_from(checks, "container"),
            volume: agreement_from(checks, "volume"),
            page: agreement_from(checks, "page"),
            ids: agreement_from(checks, "ids"),
        },
        found_by,
        work: work_from(c.get("work")?)?,
    })
}

/// The full work JSON is the core `Work`'s fields with a few said
/// differently: `notices` and `notice_for` (stored as `updates` and
/// `update_to`), and the record's `source`, `ids` and `fetched_at`.
fn work_from(w: &Value) -> Option<Work> {
    let mut fields = w.as_object()?.clone();
    for (said, stored) in [("notices", "updates"), ("notice_for", "update_to")] {
        if let Some(v) = fields.remove(said) {
            fields.insert(stored.into(), v);
        }
    }
    let source = ProviderId::parse(w.get("source")?.as_str()?)?;
    let ids: Vec<WorkId> = match w.get("ids") {
        Some(ids) => serde_json::from_value(ids.clone()).ok()?,
        None => Vec::new(),
    };
    let fetched_at: Timestamp = w.get("fetched_at")?.as_str()?.parse().ok()?;
    let work = serde_json::from_value(Value::Object(fields)).ok()?;
    let raw = Raw {
        media_type: String::new(),
        body: String::new(),
    };
    let record = SourceRecord::new(source, ids, work, FieldSet::ALL, raw, fetched_at);
    Some(Work::from_record(&record))
}

#[cfg(test)]
mod tests {
    use aspero_core::{
        Notice, NoticeFor, NoticeKind, OpenAccess, Person, Work as CoreWork, WorkType,
    };

    use super::*;

    fn id(s: &str) -> WorkId {
        s.parse().unwrap()
    }

    /// A work with every field the full JSON carries.
    fn work() -> Work {
        let mut w = CoreWork::default();
        w.title = Some("Deep learning".into());
        let mut lecun = Person::new("Yann", "LeCun");
        lecun.orcid = Some("0000-0002-1825-0097".into());
        w.authors = vec![lecun, Person::literal("The Deep Learning Group")];
        w.issued = Some("2015-05-27".parse().unwrap());
        w.work_type = Some(WorkType::JournalArticle);
        w.container_title = Some("Nature".into());
        w.volume = Some("521".into());
        w.issue = Some("7553".into());
        w.page = Some("436-444".into());
        w.publisher = Some("Springer Nature".into());
        w.abstract_text = Some("Deep learning allows…".into());
        w.url = Some("https://www.nature.com/articles/nature14539".into());
        w.cited_by_count = Some(70_000);
        let mut notice = Notice::new(NoticeKind::Correction);
        notice.notice = Some(id("doi:10.1038/nature14540"));
        notice.date = Some("2016-01".parse().unwrap());
        notice.source = Some("publisher".into());
        w.notices = vec![notice, Notice::new(NoticeKind::Other("comment".into()))];
        w.notice_for = vec![NoticeFor::new(
            NoticeKind::Retraction,
            id("doi:10.1000/retracted"),
        )];
        let mut oa = OpenAccess::default();
        oa.is_oa = true;
        oa.status = Some("bronze".into());
        w.open_access = Some(oa);
        w.other_years = vec![2016];
        w.accessed = Some("2026-11-18".parse().unwrap());
        w.language = Some("en".into());
        let record = SourceRecord::new(
            ProviderId::from_static("openalex"),
            vec![
                id("openalex:W1"),
                id("doi:10.1038/nature14539"),
                id("pmid:26017442"),
            ],
            w,
            FieldSet::ALL,
            Raw::json(&json!({})),
            "2026-11-18T09:30:01.123456789Z".parse().unwrap(),
        );
        Work::from_record(&record)
    }

    fn candidate() -> Candidate {
        Candidate {
            confidence: 0.912_345_678_9,
            checks: Checks {
                title: Agreement::Match,
                authors: Agreement::Partial,
                year: Agreement::Mismatch,
                container: Agreement::Unknown,
                volume: Agreement::Match,
                page: Agreement::Match,
                ids: Agreement::Mismatch,
            },
            found_by: vec!["openalex".into(), "crossref".into()],
            work: work(),
        }
    }

    /// One difference per field, and one per work kind.
    fn differs() -> Vec<Difference> {
        let fields = engine::CitedField::ALL.iter().map(|f| Difference {
            field: (*f).into(),
            actual: Some(format!("actual {}", f.as_str())),
            cited: (f.as_str() != "title").then(|| "cited".to_owned()),
            kind: None,
        });
        let kinds = CoreWorkKind::ALL.iter().map(|k| Difference {
            field: CitedField::Kind,
            actual: Some(k.as_str().to_owned()),
            cited: None,
            kind: Some((*k).into()),
        });
        fields.chain(kinds).collect()
    }

    fn every_case() -> Vec<Reason> {
        vec![
            Reason::Matches,
            Reason::PartlyMatches { differs: differs() },
            Reason::Unconfirmed,
            Reason::Closest { differs: differs() },
            Reason::NoMatch,
            Reason::IdExists,
            Reason::IdAgrees,
            Reason::IdPartlyMatches { differs: differs() },
            Reason::IdElsewhere { cited: candidate() },
            Reason::IdDisagrees,
            Reason::IdMissingElsewhere,
            Reason::IdMissing,
            Reason::ProvidersFailed,
            Reason::InvalidClaim {
                why: "no identifier, title or text".into(),
            },
        ]
    }

    #[test]
    fn every_case_reads_back_as_written() {
        for reason in every_case() {
            let json = reason.to_json();
            let back = reason_from_json(json.clone()).unwrap_or_else(|| panic!("{json}"));
            assert_eq!(format!("{back:?}"), format!("{reason:?}"), "{json}");
            assert_eq!(back.to_json(), json);
        }
    }

    #[test]
    fn codes_are_the_contracts() {
        let record = SourceRecord::new(
            ProviderId::from_static("crossref"),
            Vec::new(),
            CoreWork::default(),
            FieldSet::ALL,
            Raw::json(&json!({})),
            Timestamp::UNIX_EPOCH,
        );
        let candidates = [engine::Candidate {
            assessment: aspero_core::matching::assess(&aspero_core::Claim::default(), &record),
            record,
            ids: Vec::new(),
            sightings: Vec::new(),
            notices: Vec::new(),
        }];
        for r in engine::Reason::ALL {
            assert_eq!(Reason::new(r, &candidates).code(), r.as_str());
        }
        let cases: Vec<&str> = every_case().iter().map(Reason::code).collect();
        assert_eq!(cases.len(), engine::Reason::ALL.len() + 1, "{cases:?}");
    }

    #[test]
    fn the_shape_is_the_contracts() {
        let reason = Reason::PartlyMatches {
            differs: vec![Difference {
                field: CitedField::Kind,
                actual: Some("expression of concern".into()),
                cited: None,
                kind: Some(WorkKind::ExpressionOfConcern),
            }],
        };
        let v: Value = serde_json::from_str(&reason.to_json()).unwrap();
        assert_eq!(
            v,
            json!({"v": 1, "code": "partly_matches", "differs": [{
                "field": "kind",
                "actual": "expression of concern",
                "kind": "expression_of_concern",
            }]})
        );
        let v: Value =
            serde_json::from_str(&Reason::IdElsewhere { cited: candidate() }.to_json()).unwrap();
        assert_eq!(v["cited"]["work"]["notices"][0]["kind"], "correction");
        assert_eq!(v["cited"]["checks"]["year"], "mismatch");
        assert_eq!(v["cited"]["found_by"][1]["provider"], "crossref");
    }

    #[test]
    fn what_it_cant_read_is_none_never_an_error() {
        for json in [
            "",
            "not json",
            "[]",
            r#"{"v": 1}"#,
            r#"{"v": 1, "code": "a_case_added_later"}"#,
            r#"{"v": 2, "code": "matches"}"#,
            r#"{"v": "1", "code": "matches"}"#,
            r#"{"v": 1, "code": "id_elsewhere"}"#,
        ] {
            assert!(reason_from_json(json.into()).is_none(), "{json}");
        }
    }

    #[test]
    fn what_it_can_read_it_does() {
        // No version is the first.
        assert!(matches!(
            reason_from_json(r#"{"code": "matches"}"#.into()),
            Some(Reason::Matches)
        ));
        // A field added later is left out; the rest stays.
        let json = r#"{"v": 1, "code": "closest", "differs": [
            {"field": "edition", "actual": "2nd"},
            {"field": "year", "actual": "2015", "cited": "2016"}
        ]}"#;
        let Some(Reason::Closest { differs }) = reason_from_json(json.into()) else {
            panic!("{json}");
        };
        assert_eq!(differs.len(), 1);
        assert_eq!(differs[0].field, CitedField::Year);
        assert_eq!(differs[0].cited.as_deref(), Some("2016"));
        // The CLI's candidate, lean or full, reads too.
        let cli = json!({"code": "id_elsewhere", "cited": {
            "confidence": 0.91,
            "checks": {"title": {"agreement": "match", "score": 0.98}, "year": "partial"},
            "found_by": [{"provider": "crossref", "via": "search", "rank": 1},
                         {"provider": "crossref", "via": "lookup", "rank": 1}],
            "work": serde_json::from_str::<Value>(&work().json).unwrap(),
        }});
        let Some(Reason::IdElsewhere { cited }) = reason_from_json(cli.to_string()) else {
            panic!("{cli}");
        };
        assert_eq!(cited.checks.title, Agreement::Match);
        assert_eq!(cited.checks.year, Agreement::Partial);
        assert_eq!(cited.checks.ids, Agreement::Unknown);
        assert_eq!(cited.found_by, ["crossref"]);
    }

    #[test]
    fn works_read_back_from_their_json() {
        let w = work();
        let back = work_from_json(w.json.clone()).unwrap();
        assert_eq!(format!("{back:?}"), format!("{w:?}"));
        // Stored before 0.35, notices were `updates`.
        let mut old: Value = serde_json::from_str(&w.json).unwrap();
        let notices = old.as_object_mut().unwrap().remove("notices").unwrap();
        old["updates"] = notices;
        assert_eq!(work_from_json(old.to_string()).unwrap().notices.len(), 2);
        // The lean form has no `fetched_at`.
        assert!(work_from_json(r#"{"source": "crossref", "ids": []}"#.into()).is_none());
    }
}
