//! Query trees as JSON, as the CLI's `search --query` takes them. Each node
//! is an object with one key (and `phrase`, for a term):
//!
//! ```json
//! {"all": [
//!   {"keywords": "protein folding"},
//!   {"any": [{"title": "alphafold"}, {"title": "structure prediction", "phrase": true}]},
//!   {"none": [{"author_id": "0000-0002-1825-0097"}]},
//!   {"years": {"from": 2020}}, {"not_retracted": true}, {"cites": "10.1038/nature14539"}
//! ]}
//! ```
//!
//! - groups: `all`, `any`, `none`, each an array of nodes;
//! - terms: `keywords`, `title`, `author`, `container` (or `journal`),
//!   `abstract`, `author_id`, each a string, with `"phrase": true` for the
//!   words together;
//! - filters: `years` (`{"from": 2017, "to": 2020}`, either optional, or a
//!   year), `type` (a CSL type), `open_access` (a boolean), `language` (ISO
//!   639-1), and `not_retracted` and `has_abstract` (`true`);
//! - relations: `cites` (works citing it) and `cited_by` (works it cites),
//!   any identifier.
//!
//! [`QueryTree`] is the same form as a serde type, for typed arguments.

use aspero_core::{FilterValue, QueryNode, Relation, TermField, WorkType, YearRange};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value, json};

/// A query tree in its JSON form, as a serde type: it reads with
/// [`parse_query`] (so mistakes are explained the same way) and writes with
/// [`query_json`]. With the `schemars` feature, it describes itself as a
/// JSON Schema.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryTree(pub QueryNode);

impl Serialize for QueryTree {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        query_json(&self.0).serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for QueryTree {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        parse_query(&value)
            .map(QueryTree)
            .map_err(serde::de::Error::custom)
    }
}

/// Every node's key (`journal` is `container`'s alias).
const KEYS: &[&str] = &[
    "all",
    "any",
    "none",
    "keywords",
    "title",
    "author",
    "container",
    "abstract",
    "author_id",
    "years",
    "type",
    "open_access",
    "language",
    "not_retracted",
    "has_abstract",
    "cites",
    "cited_by",
];

/// A tree from its JSON form, or why not, in English.
pub fn parse_query(value: &Value) -> Result<QueryNode, String> {
    let Value::Object(object) = value else {
        return Err(format!("a query node is an object, not {value}"));
    };
    let mut keys = object.keys().filter(|k| *k != "phrase");
    let (Some(key), None) = (keys.next(), keys.next()) else {
        let keys: Vec<&String> = object.keys().collect();
        return Err(format!(
            "a query node has one key (and \"phrase\", for a term), not {keys:?}"
        ));
    };
    let value = &object[key.as_str()];
    let phrase = match object.get("phrase") {
        None => false,
        Some(Value::Bool(b)) if field(key).is_some() => *b,
        Some(_) if field(key).is_some() => return Err("\"phrase\" is true or false".into()),
        Some(_) => return Err(format!("\"phrase\" goes with a term, not {key:?}")),
    };
    if let Some(field) = field(key) {
        let text = string(key, value)?;
        return Ok(match phrase {
            true => QueryNode::phrase(field, text),
            false => QueryNode::term(field, text),
        });
    }
    let flag = |filter: FilterValue| match value {
        Value::Bool(true) => Ok(QueryNode::filter(filter)),
        _ => Err(format!("{key:?} takes true")),
    };
    Ok(match key.as_str() {
        "all" | "any" | "none" => {
            let Value::Array(items) = value else {
                return Err(format!("{key:?} takes an array of nodes"));
            };
            let nodes = items.iter().map(parse_query).collect::<Result<_, _>>()?;
            match key.as_str() {
                "all" => QueryNode::All(nodes),
                "any" => QueryNode::Any(nodes),
                _ => QueryNode::None(nodes),
            }
        }
        "years" => QueryNode::filter(FilterValue::Years(years(value)?)),
        "type" => QueryNode::filter(FilterValue::WorkType(WorkType::from_csl(string(
            key, value,
        )?))),
        "open_access" => match value {
            Value::Bool(b) => QueryNode::filter(FilterValue::OpenAccess(*b)),
            _ => return Err("\"open_access\" takes true or false".into()),
        },
        "language" => QueryNode::filter(FilterValue::Language(string(key, value)?.to_owned())),
        "not_retracted" => flag(FilterValue::NotRetracted)?,
        "has_abstract" => flag(FilterValue::HasAbstract)?,
        "cites" => QueryNode::relation(Relation::Cites, string(key, value)?),
        "cited_by" => QueryNode::relation(Relation::CitedBy, string(key, value)?),
        other => {
            return Err(format!("unknown query key {other:?} ({})", KEYS.join(", ")));
        }
    })
}

/// A tree in its JSON form; [`parse_query`] reads it back.
pub fn query_json(node: &QueryNode) -> Value {
    let one = |key: &str, value: Value| {
        let mut map = Map::new();
        map.insert(key.to_owned(), value);
        Value::Object(map)
    };
    let nodes = |nodes: &[QueryNode]| Value::Array(nodes.iter().map(query_json).collect());
    match node {
        QueryNode::All(n) => one("all", nodes(n)),
        QueryNode::Any(n) => one("any", nodes(n)),
        QueryNode::None(n) => one("none", nodes(n)),
        QueryNode::Term(t) => {
            let mut term = one(field_key(t.field), json!(t.value));
            if t.phrase {
                term["phrase"] = json!(true);
            }
            term
        }
        QueryNode::Filter(f) => match f {
            FilterValue::Years(y) => {
                let mut range = Map::new();
                if let Some(from) = y.from {
                    range.insert("from".into(), json!(from));
                }
                if let Some(to) = y.to {
                    range.insert("to".into(), json!(to));
                }
                one("years", Value::Object(range))
            }
            FilterValue::WorkType(t) => one("type", json!(t.as_csl())),
            FilterValue::OpenAccess(oa) => one("open_access", json!(oa)),
            FilterValue::Language(code) => one("language", json!(code)),
            FilterValue::NotRetracted => one("not_retracted", json!(true)),
            FilterValue::HasAbstract => one("has_abstract", json!(true)),
            _ => one("unknown", Value::Null),
        },
        QueryNode::Relation(r) => one(r.relation.as_str(), json!(r.id)),
        _ => one("unknown", Value::Null),
    }
}

/// One alternative per node key, each an object with that key alone (and
/// `phrase`, for a term); groups refer back to the node.
#[cfg(feature = "schemars")]
impl schemars::JsonSchema for QueryTree {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "QueryNode".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let node = generator.subschema_for::<Self>().to_value();
        let one = |key: &str, value: Value, description: &str| {
            json!({
                "type": "object",
                "description": description,
                "properties": {key: value},
                "required": [key],
                "additionalProperties": false,
            })
        };
        let term = |key: &str, description: &str| {
            json!({
                "type": "object",
                "description": description,
                "properties": {
                    key: {"type": "string"},
                    "phrase": {"type": "boolean", "description": "the words together, in order"},
                },
                "required": [key],
                "additionalProperties": false,
            })
        };
        let nodes = json!({"type": "array", "items": node});
        let text = json!({"type": "string"});
        let year = json!({"type": "integer"});
        let alternatives = [
            one("all", nodes.clone(), "every node matches"),
            one("any", nodes.clone(), "at least one node matches"),
            one("none", nodes, "no node matches"),
            term("keywords", "words anywhere in the record"),
            term("title", "words of the title"),
            term("author", "an author's name"),
            term(
                "container",
                "where the work appeared: the journal, or the book for a chapter",
            ),
            term("journal", "the same as container"),
            term("abstract", "words of the abstract"),
            term("author_id", "an author's ORCID, or OpenAlex author id"),
            one(
                "years",
                json!({"anyOf": [
                    year.clone(),
                    {
                        "type": "object",
                        "properties": {"from": year.clone(), "to": year},
                        "additionalProperties": false,
                    },
                ]}),
                "publication years, inclusive: one year, or from and to (either optional)",
            ),
            one(
                "type",
                text.clone(),
                "a CSL work type: article-journal, book, chapter, paper-conference, \
                 article (a preprint), …",
            ),
            one(
                "open_access",
                json!({"type": "boolean"}),
                "whether a free, legal copy is known",
            ),
            one("language", text.clone(), "an ISO 639-1 code: en, de, fr, …"),
            one(
                "not_retracted",
                json!({"const": true}),
                "leaves out retracted works",
            ),
            one(
                "has_abstract",
                json!({"const": true}),
                "only works with an abstract",
            ),
            one(
                "cites",
                text.clone(),
                "works citing this work, named by any identifier (DOI, PMID, arXiv, …)",
            ),
            one(
                "cited_by",
                text,
                "works this work cites (its references), named by any identifier",
            ),
        ];
        schemars::Schema::try_from(json!({
            "description": "A query tree: each node an object with one key.",
            "anyOf": alternatives,
        }))
        .expect("an object is a schema")
    }
}

fn field(key: &str) -> Option<TermField> {
    Some(match key {
        "keywords" => TermField::Keywords,
        "title" => TermField::Title,
        "author" => TermField::Author,
        "container" | "journal" => TermField::Container,
        "abstract" => TermField::Abstract,
        "author_id" => TermField::AuthorId,
        _ => return None,
    })
}

fn field_key(field: TermField) -> &'static str {
    match field {
        TermField::Keywords => "keywords",
        TermField::Title => "title",
        TermField::Author => "author",
        TermField::Container => "container",
        TermField::Abstract => "abstract",
        _ => "author_id",
    }
}

fn string<'a>(key: &str, value: &'a Value) -> Result<&'a str, String> {
    value
        .as_str()
        .ok_or_else(|| format!("{key:?} takes a string, not {value}"))
}

/// `2017`, or `{"from": 2017, "to": 2020}` with either left out.
fn years(value: &Value) -> Result<YearRange, String> {
    let year = |v: &Value| {
        v.as_i64()
            .and_then(|y| i16::try_from(y).ok())
            .ok_or_else(|| format!("not a year: {v}"))
    };
    match value {
        Value::Object(range) => {
            if let Some(other) = range.keys().find(|k| !matches!(k.as_str(), "from" | "to")) {
                return Err(format!("\"years\" takes from and to, not {other:?}"));
            }
            Ok(YearRange {
                from: range.get("from").map(year).transpose()?,
                to: range.get("to").map(year).transpose()?,
            })
        }
        v => {
            let y = year(v)?;
            Ok(YearRange {
                from: Some(y),
                to: Some(y),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trees_read_and_write_their_json_form() {
        let text = r#"{"all": [
            {"keywords": "protein folding"},
            {"any": [{"title": "alphafold"}, {"journal": "structure prediction", "phrase": true}]},
            {"none": [{"author_id": "0000-0002-1825-0097"}, {"not_retracted": true}]},
            {"years": {"from": 2020}}, {"years": 2019}, {"type": "article-journal"},
            {"open_access": false}, {"language": "en"}, {"has_abstract": true},
            {"cites": "10.1038/nature14539"}, {"cited_by": "W1"}
        ]}"#;
        let tree = parse_query(&serde_json::from_str(text).unwrap()).unwrap();
        let QueryNode::All(nodes) = &tree else {
            panic!("{tree:?}")
        };
        assert_eq!(nodes.len(), 11);
        assert_eq!(
            nodes[1],
            QueryNode::Any(vec![
                QueryNode::term(TermField::Title, "alphafold"),
                QueryNode::phrase(TermField::Container, "structure prediction"),
            ])
        );
        assert_eq!(
            nodes[4],
            QueryNode::filter(FilterValue::Years(YearRange {
                from: Some(2019),
                to: Some(2019)
            }))
        );
        assert_eq!(parse_query(&query_json(&tree)).unwrap(), tree);
        assert_eq!(
            query_json(&nodes[1]).to_string(),
            r#"{"any":[{"title":"alphafold"},{"container":"structure prediction","phrase":true}]}"#
        );
    }

    #[test]
    fn query_trees_are_serde_types() {
        let text = r#"{"all":[{"title":"alphafold","phrase":true},{"cites":"W1"}]}"#;
        let tree: QueryTree = serde_json::from_str(text).unwrap();
        assert_eq!(
            tree.0,
            parse_query(&serde_json::from_str(text).unwrap()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(&tree).unwrap(),
            serde_json::from_str::<Value>(text).unwrap()
        );
        let err = serde_json::from_str::<QueryTree>(r#"{"titel": "x"}"#).unwrap_err();
        assert!(err.to_string().starts_with("unknown query key \"titel\""));
    }

    /// The schema has one alternative per key the parser reads, and every
    /// one it describes parses.
    #[cfg(feature = "schemars")]
    #[test]
    fn the_schema_describes_what_parses() {
        let schema = schemars::schema_for!(QueryTree).to_value();
        let alternatives = schema["anyOf"].as_array().unwrap();
        let keys: Vec<&str> = alternatives
            .iter()
            .map(|a| a["required"][0].as_str().unwrap())
            .collect();
        let mut expected = KEYS.to_vec();
        expected.insert(7, "journal");
        assert_eq!(keys, expected);
        let sample = |key: &str| match key {
            "all" | "any" | "none" => json!([{"title": "x"}]),
            "years" => json!({"from": 2020}),
            "open_access" | "not_retracted" | "has_abstract" => json!(true),
            "language" => json!("en"),
            _ => json!("10.1038/nature14539"),
        };
        for key in keys {
            let mut node = Map::new();
            node.insert(key.into(), sample(key));
            parse_query(&Value::Object(node)).unwrap();
        }
    }

    #[test]
    fn mistakes_are_explained() {
        let err = |text: &str| parse_query(&serde_json::from_str(text).unwrap()).unwrap_err();
        assert!(err(r#"{"titel": "x"}"#).starts_with("unknown query key \"titel\""));
        assert!(err(r#"{"title": "x", "author": "y"}"#).starts_with("a query node has one key"));
        assert_eq!(
            err(r#"{"all": {"title": "x"}}"#),
            "\"all\" takes an array of nodes"
        );
        assert_eq!(err(r#"{"title": 3}"#), "\"title\" takes a string, not 3");
        assert_eq!(
            err(r#"{"cites": "x", "phrase": true}"#),
            "\"phrase\" goes with a term, not \"cites\""
        );
        assert_eq!(
            err(r#"{"not_retracted": false}"#),
            "\"not_retracted\" takes true"
        );
        assert_eq!(
            err(r#"{"years": {"since": 2000}}"#),
            "\"years\" takes from and to, not \"since\""
        );
        assert_eq!(err(r#"["x"]"#), "a query node is an object, not [\"x\"]");
    }
}
