//! A scripted source and fixture works for the tools' tests, and a check
//! that results match their tools' output schemas.

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use aspero_core::{
    BooleanSupport, Capabilities, FieldSet, Filter, Hit, IdKind, Lookup, Page, PartialDate, Person,
    Provider, ProviderId, QueryNode, Raw, Relation, ResiliencePolicy, Response, Result,
    SearchQuery, Sort, SourceRecord, TermField, Usage, Work, WorkId,
};
use aspero_engine::Engine;
use aspero_mcp::{Caller, Session, Tools};
use async_trait::async_trait;
use jiff::Timestamp;
use serde_json::Value;

/// What every search costs, in USD: a binary fraction, so totals are exact.
pub const SEARCH_COST: f64 = 0.25;

/// A source with a few works. Lookups find works by identifier; a search
/// finds works whose title is the keywords, the title field or any title
/// term of a tree, or that cite (or are cited by) a tree's relation's work.
pub struct Scripted {
    id: ProviderId,
    caps: Capabilities,
    works: Vec<SourceRecord>,
    /// Works citing each OpenAlex work id.
    citing: HashMap<String, Vec<SourceRecord>>,
    /// Each OpenAlex work id's references.
    references: HashMap<String, Vec<SourceRecord>>,
    pub searches: Mutex<Vec<SearchQuery>>,
}

impl Scripted {
    pub fn new(id: &'static str, works: Vec<SourceRecord>) -> Self {
        let mut caps = Capabilities::default();
        caps.lookup_kinds = vec![IdKind::Doi, IdKind::Arxiv, IdKind::OpenAlex];
        caps.max_batch = 10;
        caps.fields = FieldSet::ALL;
        caps.search = true;
        caps.search_filters = vec![Filter::Keywords, Filter::Title, Filter::Years];
        caps.search_any = BooleanSupport::WithinField;
        caps.sorts = vec![Sort::Relevance, Sort::Newest];
        let id = ProviderId::from_static(id);
        let works = works
            .into_iter()
            .map(|mut w| {
                w.source = id.clone();
                w
            })
            .collect();
        Scripted {
            id,
            caps,
            works,
            citing: HashMap::new(),
            references: HashMap::new(),
            searches: Mutex::default(),
        }
    }

    /// Finds works by citations, as OpenAlex does.
    pub fn citations(
        mut self,
        citing: &[(&str, SourceRecord)],
        cited: &[(&str, SourceRecord)],
    ) -> Self {
        self.caps.relations = vec![Relation::Cites, Relation::CitedBy];
        for (work, record) in citing {
            self.citing
                .entry(work.to_string())
                .or_default()
                .push(record.clone());
        }
        for (work, record) in cited {
            self.references
                .entry(work.to_string())
                .or_default()
                .push(record.clone());
        }
        self
    }

    pub fn sent(&self) -> Vec<SearchQuery> {
        self.searches.lock().unwrap().clone()
    }

    fn titled(&self, title: &str) -> Vec<SourceRecord> {
        self.works
            .iter()
            .filter(|w| {
                w.work.title.as_deref().map(str::to_lowercase) == Some(title.to_lowercase())
            })
            .cloned()
            .collect()
    }

    fn found(&self, node: &QueryNode, out: &mut Vec<SourceRecord>) {
        match node {
            QueryNode::All(nodes) | QueryNode::Any(nodes) => {
                nodes.iter().for_each(|n| self.found(n, out));
            }
            QueryNode::Term(t) if t.field == TermField::Title => out.extend(self.titled(&t.value)),
            QueryNode::Relation(r) => {
                let by = match r.relation {
                    Relation::Cites => &self.citing,
                    _ => &self.references,
                };
                out.extend(by.get(&r.id).cloned().unwrap_or_default());
            }
            _ => {}
        }
    }
}

#[async_trait]
impl Provider for Scripted {
    fn id(&self) -> ProviderId {
        self.id.clone()
    }

    fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    fn express(&self, tree: &QueryNode) -> std::result::Result<(), String> {
        aspero_core::query::check_supported(tree, &self.caps, "Scripted")
    }

    async fn lookup(&self, ids: &[WorkId], _: FieldSet) -> Result<Response<Vec<Lookup>>> {
        let lookups = ids
            .iter()
            .map(|id| Lookup {
                id: id.clone(),
                record: self.works.iter().find(|w| w.ids.contains(id)).cloned(),
            })
            .collect();
        Ok(costing(lookups, 0.0))
    }

    async fn search(&self, query: &SearchQuery) -> Result<Response<Page>> {
        self.searches.lock().unwrap().push(query.clone());
        let mut records = Vec::new();
        match &query.tree {
            Some(tree) => self.found(tree, &mut records),
            None => {
                records.extend(self.titled(&query.text));
                if let Some(title) = &query.title {
                    records.extend(self.titled(title));
                }
            }
        }
        let hits = records
            .into_iter()
            .enumerate()
            .map(|(i, r)| Hit::new(r, Some(100.0 - i as f64)))
            .collect();
        Ok(costing(Page::new(hits, Some(42), None), SEARCH_COST))
    }
}

fn costing<T>(value: T, cost: f64) -> Response<T> {
    let mut usage = Usage::default();
    usage.cost = Some(cost);
    let mut response = Response::new(value);
    response.usage = Some(usage);
    response
}

pub fn work(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut w = Work::default();
    w.title = Some(title.into());
    w.authors = authors.iter().map(|a| Person::literal(*a)).collect();
    w.issued = Some(PartialDate::from_year(year));
    w.abstract_text = Some("An abstract.".into());
    SourceRecord::new(
        ProviderId::from_static("openalex"),
        ids.iter().map(|i| i.parse().unwrap()).collect(),
        w,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        },
        Timestamp::UNIX_EPOCH,
    )
}

pub const LECUN: [&str; 3] = ["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"];

pub fn deep_learning() -> SourceRecord {
    work(
        &["10.1038/nature14539", "W1"],
        "Deep learning",
        &LECUN,
        2015,
    )
}

pub fn attention() -> SourceRecord {
    work(
        &["W2", "arXiv:1706.03762"],
        "Attention is all you need",
        &["Ashish Vaswani", "Noam Shazeer"],
        2017,
    )
}

pub fn backprop() -> SourceRecord {
    work(
        &["10.1038/323533a0", "W3"],
        "Learning representations by back-propagating errors",
        &[
            "David E. Rumelhart",
            "Geoffrey E. Hinton",
            "Ronald J. Williams",
        ],
        1986,
    )
}

/// The setup most tests use: one source knowing three works, where
/// "Attention" cites "Deep learning", which cites "back-propagation".
pub fn source() -> Arc<Scripted> {
    Arc::new(
        Scripted::new("openalex", vec![deep_learning(), attention(), backprop()])
            .citations(&[("W1", attention())], &[("W1", backprop())]),
    )
}

pub fn engine(providers: Vec<Arc<Scripted>>) -> Engine {
    providers
        .into_iter()
        .enumerate()
        .fold(Engine::builder(), |b, (i, p)| {
            b.provider(p, ResiliencePolicy::default(), i as i32)
        })
        .build()
}

pub fn tools(providers: Vec<Arc<Scripted>>) -> Tools {
    Tools::new(engine(providers))
}

pub fn session(budget: f64) -> Session {
    Session::new(Caller::default(), budget)
}

/// Whether `value` matches `schema`: the keywords the output schemas use
/// (type, properties, required, items, additionalProperties, enum and
/// const). Panics with the path of the first mismatch.
pub fn conforms(value: &Value, schema: &Value, path: &str) {
    let Some(schema) = schema.as_object() else {
        return;
    };
    if let Some(kind) = schema.get("type") {
        let kinds: Vec<&str> = match kind {
            Value::String(k) => vec![k.as_str()],
            Value::Array(ks) => ks.iter().filter_map(Value::as_str).collect(),
            _ => vec![],
        };
        let actual = match value {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(n) if n.is_i64() || n.is_u64() => "integer",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        };
        let ok = kinds.contains(&actual) || (actual == "integer" && kinds.contains(&"number"));
        assert!(ok, "{path}: {actual} where {kinds:?} expected: {value}");
    }
    if let Some(Value::Array(options)) = schema.get("enum") {
        assert!(
            options.contains(value),
            "{path}: {value} not in {options:?}"
        );
    }
    if let Some(expected) = schema.get("const") {
        assert_eq!(value, expected, "{path}");
    }
    if let Some(Value::Array(required)) = schema.get("required") {
        for key in required.iter().filter_map(Value::as_str) {
            assert!(value.get(key).is_some(), "{path}: missing {key}: {value}");
        }
    }
    if let Value::Object(fields) = value {
        let properties = schema.get("properties").and_then(Value::as_object);
        for (key, v) in fields {
            match properties.and_then(|p| p.get(key)) {
                Some(s) => conforms(v, s, &format!("{path}.{key}")),
                None => {
                    if let Some(extra) = schema.get("additionalProperties") {
                        conforms(v, extra, &format!("{path}.{key}"));
                    }
                }
            }
        }
    }
    if let (Value::Array(items), Some(item)) = (value, schema.get("items")) {
        for (i, v) in items.iter().enumerate() {
            conforms(v, item, &format!("{path}[{i}]"));
        }
    }
}
