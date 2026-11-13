//! Query trees in OpenAlex's terms.
//!
//! OpenAlex ANDs its filters and nothing else (checked live 2026-11-13).
//! Within one text field, though, a value is a boolean query: `search=`
//! (keywords) and the `.search` filters (`title.search:`,
//! `raw_author_name.search:`, `abstract.search:`) take `AND`, `OR`, `NOT`,
//! quotes and parentheses, and a filter's `!` negates its whole value. Other
//! filters OR values with `|` (`type:article|book`) and take `!` too. So a
//! tree is said as an AND of clauses, and it can be when:
//!
//! - **all** of anything: one clause per node;
//! - **any** within one text field, however nested (`title.search:(a OR
//!   (b AND c))`), or of values of one filter (work types, languages,
//!   ORCIDs, OpenAlex author ids, works cited or citing, identifiers of
//!   one kind). Across fields it can't: there is no OR between filters;
//! - **none** of anything that can be negated, as an AND of negations: a
//!   text field or keywords, a work type, language, author id, journal,
//!   open access, retraction, abstract, relation or identifier. A range of
//!   years can't be, nor an all of several fields (that would need an OR).
//!
//! Identifiers are filters too (checked live 2026-11-14): `doi:` (arXiv
//! ids by their DataCite DOI, `10.48550/arxiv.…`), `ids.pmid:`,
//! `ids.pmcid:` (`PMC…`, though records hold the bare number) and
//! `openalex:`. There is no ISBN filter.

use aspero_core::query::{AuthorId, check_supported, render_lucene};
use aspero_core::{
    Capabilities, FilterValue, OpenAlexId, QueryNode, Relation, Term, TermField, WorkId, WorkType,
};

/// As reasons name it.
const NAME: &str = "OpenAlex";

/// A tree as OpenAlex parameters.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Plan {
    /// `search=`, keywords as a boolean query.
    pub search: Option<String>,
    /// `filter=` clauses, ANDed.
    pub filters: Vec<String>,
    /// Journals to find sources for, each then a `primary_location.source.id`
    /// filter.
    pub containers: Vec<Container>,
}

/// Journals whose sources make one filter: any of `names` (name, phrase).
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Container {
    pub names: Vec<(String, bool)>,
    /// Leave out works from these sources.
    pub negated: bool,
}

enum Clause {
    Search(String),
    Filter(String),
    Container(Container),
}

/// The plan for `tree` (normalised), or why OpenAlex can't say it.
pub(crate) fn translate(tree: &QueryNode, caps: &Capabilities) -> Result<Plan, String> {
    check_supported(tree, caps, NAME)?;
    let mut plan = Plan::default();
    let mut searches = Vec::new();
    for clause in conjunction(tree, false)? {
        match clause {
            Clause::Search(s) => searches.push(s),
            Clause::Filter(f) => plan.filters.push(f),
            Clause::Container(c) => plan.containers.push(c),
        }
    }
    plan.search = match searches.len() {
        0 => None,
        1 => searches.pop(),
        _ => Some(
            searches
                .iter()
                .map(|s| format!("({s})"))
                .collect::<Vec<_>>()
                .join(" AND "),
        ),
    };
    Ok(plan)
}

/// The clauses, ANDed, that say `node` (or, `negated`, that it doesn't
/// match).
fn conjunction(node: &QueryNode, negated: bool) -> Result<Vec<Clause>, String> {
    // One text field: one boolean query, however the tree nests. A none is
    // left to the cases below, as negations each in its own clause.
    if !matches!(node, QueryNode::None(_))
        && let Some(field) = node.single_field().filter(|f| text_key(*f).is_some())
    {
        return Ok(vec![text_clause(field, node, negated)?]);
    }
    match (node, negated) {
        (QueryNode::All(nodes), false) => concat(nodes, false),
        // Not any, and none: each left out.
        (QueryNode::Any(nodes), true) | (QueryNode::None(nodes), false) => concat(nodes, true),
        (QueryNode::None(nodes), true) => conjunction(&QueryNode::Any(nodes.clone()), false),
        (QueryNode::Any(nodes), false) => Ok(vec![any_of_values(node, nodes)?]),
        (QueryNode::All(_), true) => Err(format!(
            "{NAME} can't leave out {} together, only within one field",
            node.leaf_names()
        )),
        (leaf, negated) => leaf_clauses(leaf, negated),
    }
}

fn concat(nodes: &[QueryNode], negated: bool) -> Result<Vec<Clause>, String> {
    let mut out = Vec::new();
    for node in nodes {
        out.extend(conjunction(node, negated)?);
    }
    Ok(out)
}

/// `search=` for keywords; the `.search` filter for other text fields.
fn text_key(field: TermField) -> Option<&'static str> {
    match field {
        TermField::Keywords => Some("search"),
        TermField::Title => Some("title.search"),
        TermField::Author => Some("raw_author_name.search"),
        TermField::Abstract => Some("abstract.search"),
        _ => None,
    }
}

fn text_clause(field: TermField, node: &QueryNode, negated: bool) -> Result<Clause, String> {
    let expr = match node {
        QueryNode::Term(t) => text_value(&t.value, t.phrase, false),
        _ => render_lucene(node, &mut |leaf| match leaf {
            QueryNode::Term(t) => Ok(text_value(&t.value, t.phrase, true)),
            other => Err(format!("{NAME} can't search by {}", other.leaf_name())),
        })?,
    };
    let grouped = !matches!(node, QueryNode::Term(_));
    Ok(match (field, negated) {
        (TermField::Keywords, false) => Clause::Search(expr),
        (TermField::Keywords, true) => Clause::Search(format!("NOT ({expr})")),
        (field, negated) => {
            let key = text_key(field).expect("a text field");
            let bang = if negated { "!" } else { "" };
            match negated && grouped {
                true => Clause::Filter(format!("{key}:{bang}({expr})")),
                false => Clause::Filter(format!("{key}:{bang}{expr}")),
            }
        }
    })
}

/// A term's value for OpenAlex's boolean queries: words or a quoted
/// phrase, with the characters that would be syntax (or split a filter)
/// dropped and `AND`, `OR` and `NOT` lowercased into plain words. Several
/// words are bracketed when `grouped`, so they stay together beside an
/// operator.
fn text_value(value: &str, phrase: bool, grouped: bool) -> String {
    let words: Vec<String> = value
        .split(|c: char| c.is_whitespace() || "\"(),|!:*?~\\[]{}^".contains(c))
        .map(|w| w.trim_start_matches(['-', '+']))
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "AND" | "OR" | "NOT" => w.to_lowercase(),
            w => w.to_owned(),
        })
        .collect();
    let joined = words.join(" ");
    match (phrase, words.len()) {
        (true, _) => format!("\"{joined}\""),
        (false, 2..) if grouped => format!("({joined})"),
        _ => joined,
    }
}

/// An any of leaves that are all values of one filter: `type:a|b`, or
/// journals, whose sources are then ORed.
fn any_of_values(node: &QueryNode, nodes: &[QueryNode]) -> Result<Clause, String> {
    let refuse = || match identifier_kinds(nodes) {
        Some(kinds) => format!(
            "{NAME} can't combine {} with any, only identifiers of one kind",
            kinds
        ),
        None => format!("{NAME} can't combine {} with any", node.leaf_names()),
    };
    let journals: Option<Vec<(String, bool)>> = nodes
        .iter()
        .map(|n| match n {
            QueryNode::Term(t) if t.field == TermField::Container => {
                Some((t.value.clone(), t.phrase))
            }
            _ => None,
        })
        .collect();
    if let Some(names) = journals.filter(|names| !names.is_empty()) {
        return Ok(Clause::Container(Container {
            names,
            negated: false,
        }));
    }
    let mut key = None;
    let mut values = Vec::new();
    for n in nodes {
        let (k, v) = orable(n).ok_or_else(refuse)??;
        if key.is_some_and(|key| key != k) {
            return Err(refuse());
        }
        key = Some(k);
        values.push(v);
    }
    let key = key.ok_or_else(refuse)?;
    Ok(Clause::Filter(format!("{key}:{}", values.join("|"))))
}

/// A leaf's filter key and value, if values of its filter can be ORed.
fn orable(node: &QueryNode) -> Option<Result<(&'static str, String), String>> {
    Some(match node {
        QueryNode::Filter(FilterValue::WorkType(t)) => work_type(t).map(|v| ("type", v.to_owned())),
        QueryNode::Filter(FilterValue::Language(code)) => Ok(("language", code.clone())),
        QueryNode::Term(t) if t.field == TermField::AuthorId => Ok(author_id(&t.value)),
        QueryNode::Term(t) if t.field == TermField::Identifier => identifier(t),
        QueryNode::Relation(r) => Ok((relation_key(r.relation), work_id(&r.id))),
        _ => return None,
    })
}

/// The kinds of identifier `nodes` are, as an English list, when every one
/// is an identifier: `DOI and PMID`.
fn identifier_kinds(nodes: &[QueryNode]) -> Option<String> {
    let mut names: Vec<&str> = Vec::new();
    for node in nodes {
        let QueryNode::Term(t) = node else {
            return None;
        };
        if t.field != TermField::Identifier {
            return None;
        }
        let name = t.work_id().ok()?.kind().name();
        if !names.contains(&name) {
            names.push(name);
        }
    }
    Some(match names.as_slice() {
        [init @ .., last] if !init.is_empty() => format!("{} and {last}", init.join(", ")),
        _ => names.concat(),
    })
}

/// An identifier's filter key and value: `doi:` (an arXiv id by its
/// DataCite DOI), `ids.pmid:`, `ids.pmcid:` or `openalex:`.
fn identifier(t: &Term) -> Result<(&'static str, String), String> {
    let id = t.work_id()?;
    let (key, value) = match &id {
        WorkId::Doi(doi) => ("doi", doi.as_str().to_owned()),
        WorkId::Arxiv(arxiv) => ("doi", arxiv.doi().as_str().to_owned()),
        WorkId::Pmid(pmid) => ("ids.pmid", pmid.to_string()),
        WorkId::Pmcid(pmcid) => ("ids.pmcid", pmcid.to_string()),
        WorkId::OpenAlex(w) => ("openalex", w.to_string()),
        other => return Err(format!("{NAME} has no {} filter", other.kind().name())),
    };
    // `,` and `|` would split the filter; a DOI may hold either.
    if value.contains([',', '|']) {
        return Err(format!(
            "{NAME} can't filter by a {} holding a comma or a pipe",
            id.kind().name()
        ));
    }
    Ok((key, value))
}

fn leaf_clauses(leaf: &QueryNode, negated: bool) -> Result<Vec<Clause>, String> {
    let bang = if negated { "!" } else { "" };
    let filter = |key: &str, value: &str| Ok(vec![Clause::Filter(format!("{key}:{bang}{value}"))]);
    match leaf {
        QueryNode::Term(t) if t.field == TermField::Container => {
            Ok(vec![Clause::Container(Container {
                names: vec![(t.value.clone(), t.phrase)],
                negated,
            })])
        }
        QueryNode::Term(t) if t.field == TermField::AuthorId => {
            let (key, value) = author_id(&t.value);
            filter(key, &value)
        }
        QueryNode::Term(t) if t.field == TermField::Identifier => {
            let (key, value) = identifier(t)?;
            filter(key, &value)
        }
        QueryNode::Term(t) => Err(format!("{NAME} can't search by {}", t.field.name())),
        QueryNode::Filter(f) => match f {
            FilterValue::Years(_) if negated => {
                Err(format!("{NAME} can't leave out a range of years"))
            }
            FilterValue::Years(years) => Ok([
                years
                    .from
                    .map(|y| format!("from_publication_date:{y:04}-01-01")),
                years
                    .to
                    .map(|y| format!("to_publication_date:{y:04}-12-31")),
            ]
            .into_iter()
            .flatten()
            .map(Clause::Filter)
            .collect()),
            FilterValue::WorkType(t) => filter("type", work_type(t)?),
            FilterValue::OpenAccess(oa) => {
                Ok(vec![Clause::Filter(format!("is_oa:{}", *oa != negated))])
            }
            FilterValue::NotRetracted => {
                Ok(vec![Clause::Filter(format!("is_retracted:{negated}"))])
            }
            FilterValue::Language(code) => filter("language", code),
            FilterValue::HasAbstract => {
                Ok(vec![Clause::Filter(format!("has_abstract:{}", !negated))])
            }
            _ => Err(format!("{NAME} can't filter by {}", leaf.leaf_name())),
        },
        QueryNode::Relation(r) => filter(relation_key(r.relation), &work_id(&r.id)),
        group => Err(format!("{NAME} can't say {}", group.leaf_name())),
    }
}

fn relation_key(relation: Relation) -> &'static str {
    match relation {
        Relation::Cites => "cites",
        _ => "cited_by",
    }
}

/// A relation's work as OpenAlex names it. The engine resolves other
/// identifiers to OpenAlex work ids before searching; until then (asking
/// whether a tree can be said) the value doesn't matter.
fn work_id(id: &str) -> String {
    id.parse::<OpenAlexId>()
        .map_or_else(|_| id.replace([',', '|'], ""), |w| w.to_string())
}

/// `authorships.author.orcid` or `authorships.author.id`, and the value.
/// Validated before translation.
fn author_id(value: &str) -> (&'static str, String) {
    match AuthorId::parse(value) {
        Some(AuthorId::OpenAlex(a)) => ("authorships.author.id", a),
        Some(AuthorId::Orcid(o)) => ("authorships.author.orcid", o),
        _ => ("authorships.author.orcid", value.replace([',', '|'], "")),
    }
}

/// OpenAlex's name for a work type.
pub(crate) fn work_type(t: &WorkType) -> Result<&str, String> {
    Ok(match t {
        WorkType::JournalArticle => "article",
        WorkType::ConferencePaper => "conference-paper",
        WorkType::Preprint => "preprint",
        WorkType::Book => "book",
        WorkType::Chapter => "book-chapter",
        WorkType::Thesis => "dissertation",
        WorkType::Report => "report",
        WorkType::Dataset => "dataset",
        WorkType::Review => "review",
        WorkType::Other(other) => other.as_str(),
        _ => {
            return Err(format!("{NAME} has no work type matching {:?}", t.as_csl()));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use aspero_core::{BooleanSupport, Filter, YearRange};

    fn caps() -> Capabilities {
        let mut caps = Capabilities::default();
        caps.search_filters = Filter::ALL.to_vec();
        caps.search_any = BooleanSupport::WithinField;
        caps.search_none = BooleanSupport::Anywhere;
        caps.search_nesting = true;
        caps.relations = Relation::ALL.to_vec();
        caps
    }

    fn term(field: TermField, v: &str) -> QueryNode {
        QueryNode::term(field, v)
    }

    fn plan(tree: QueryNode) -> Result<Plan, String> {
        translate(&tree.normalised().unwrap(), &caps())
    }

    #[test]
    fn keywords_and_fields_nest_within_one_field() {
        let tree = QueryNode::All(vec![
            term(TermField::Keywords, "protein folding"),
            QueryNode::Any(vec![
                QueryNode::phrase(TermField::Title, "deep learning"),
                QueryNode::All(vec![
                    term(TermField::Title, "cats AND dogs"),
                    QueryNode::None(vec![term(TermField::Title, "(vision)")]),
                ]),
            ]),
            QueryNode::None(vec![
                term(TermField::Author, "lecun"),
                QueryNode::phrase(TermField::Keywords, "survey"),
            ]),
        ]);
        let p = plan(tree).unwrap();
        assert_eq!(
            p.search.as_deref(),
            Some(r#"(protein folding) AND (NOT ("survey"))"#)
        );
        assert_eq!(
            p.filters,
            [
                r#"title.search:"deep learning" OR ((cats and dogs) AND (NOT vision))"#,
                "raw_author_name.search:!lecun",
            ]
        );
    }

    #[test]
    fn any_of_one_filters_values_is_piped() {
        let tree = QueryNode::All(vec![
            QueryNode::Any(vec![
                QueryNode::filter(FilterValue::WorkType(WorkType::Book)),
                QueryNode::filter(FilterValue::WorkType(WorkType::Chapter)),
            ]),
            QueryNode::Any(vec![
                term(TermField::AuthorId, "https://orcid.org/0000-0002-1825-0097"),
                term(TermField::AuthorId, "0000-0002-1694-233X"),
            ]),
            QueryNode::None(vec![
                QueryNode::filter(FilterValue::Language("en".into())),
                QueryNode::relation(Relation::Cites, "https://openalex.org/W2919115771"),
            ]),
            QueryNode::filter(FilterValue::NotRetracted),
            QueryNode::filter(FilterValue::HasAbstract),
            QueryNode::filter(FilterValue::OpenAccess(true)),
            QueryNode::filter(FilterValue::Years(YearRange {
                from: Some(2017),
                to: Some(2020),
            })),
            QueryNode::relation(Relation::CitedBy, "W1"),
        ]);
        assert_eq!(
            plan(tree).unwrap().filters,
            [
                "type:book|book-chapter",
                "authorships.author.orcid:0000-0002-1825-0097|0000-0002-1694-233X",
                "language:!en",
                "cites:!W2919115771",
                "is_retracted:false",
                "has_abstract:true",
                "is_oa:true",
                "from_publication_date:2017-01-01",
                "to_publication_date:2020-12-31",
                "cited_by:W1",
            ]
        );
        // None of the filters that are booleans flips them.
        let none = QueryNode::None(vec![
            QueryNode::filter(FilterValue::NotRetracted),
            QueryNode::filter(FilterValue::HasAbstract),
            QueryNode::filter(FilterValue::OpenAccess(true)),
            term(TermField::AuthorId, "A5023888391"),
        ]);
        assert_eq!(
            plan(none).unwrap().filters,
            [
                "is_retracted:true",
                "has_abstract:false",
                "is_oa:false",
                "authorships.author.id:!A5023888391",
            ]
        );
    }

    #[test]
    fn negated_groups_in_one_field_are_bracketed() {
        // None of an any: each left out. Not all (none of an all) in one
        // field: one negated query.
        let tree = QueryNode::None(vec![QueryNode::All(vec![
            term(TermField::Title, "a"),
            term(TermField::Title, "b c"),
        ])]);
        assert_eq!(plan(tree).unwrap().filters, ["title.search:!(a AND (b c))"]);
        let tree = QueryNode::None(vec![term(TermField::Abstract, "x, y|z")]);
        assert_eq!(plan(tree).unwrap().filters, ["abstract.search:!x y z"]);
    }

    #[test]
    fn journals_become_source_lookups() {
        let tree = QueryNode::All(vec![
            QueryNode::phrase(TermField::Container, "Nature"),
            QueryNode::None(vec![term(TermField::Container, "Science")]),
        ]);
        assert_eq!(
            plan(tree).unwrap().containers,
            [
                Container {
                    names: vec![("Nature".into(), true)],
                    negated: false
                },
                Container {
                    names: vec![("Science".into(), false)],
                    negated: true
                },
            ]
        );
    }

    #[test]
    fn any_of_journals_ors_their_sources() {
        let tree = QueryNode::Any(vec![
            term(TermField::Container, "Nature"),
            QueryNode::phrase(TermField::Container, "Science"),
        ]);
        assert_eq!(
            plan(tree).unwrap().containers,
            [Container {
                names: vec![("Nature".into(), false), ("Science".into(), true)],
                negated: false
            }]
        );
    }

    #[test]
    fn what_openalex_cant_say_and_why() {
        let reason = |tree| plan(tree).unwrap_err();
        assert_eq!(
            reason(QueryNode::Any(vec![
                term(TermField::Title, "a"),
                term(TermField::Author, "b"),
            ])),
            "OpenAlex can't combine title and author with any"
        );
        assert_eq!(
            reason(QueryNode::Any(vec![
                term(TermField::Keywords, "a"),
                QueryNode::filter(FilterValue::NotRetracted),
            ])),
            "OpenAlex can't combine keywords and retraction with any"
        );
        assert_eq!(
            reason(QueryNode::Any(vec![
                QueryNode::filter(FilterValue::Language("en".into())),
                QueryNode::filter(FilterValue::WorkType(WorkType::Book)),
            ])),
            "OpenAlex can't combine language and work type with any"
        );
        assert_eq!(
            reason(QueryNode::None(vec![QueryNode::All(vec![
                term(TermField::Title, "a"),
                term(TermField::Author, "b"),
            ])])),
            "OpenAlex can't leave out title and author together, only within one field"
        );
        assert_eq!(
            reason(QueryNode::None(vec![QueryNode::filter(
                FilterValue::Years(YearRange {
                    from: Some(2000),
                    to: None
                })
            )])),
            "OpenAlex can't leave out a range of years"
        );
        assert_eq!(
            reason(QueryNode::filter(FilterValue::WorkType(WorkType::Software))),
            "OpenAlex has no work type matching \"software\""
        );
    }

    #[test]
    fn reviews_are_a_type_openalex_filters_on() {
        assert_eq!(work_type(&WorkType::Review), Ok("review"));
    }

    fn id(v: &str) -> QueryNode {
        term(TermField::Identifier, v)
    }

    #[test]
    fn identifiers_are_filters_ored_within_a_kind() {
        let tree = QueryNode::All(vec![
            QueryNode::phrase(TermField::Identifier, "https://doi.org/10.1038/NATURE14539"),
            QueryNode::filter(FilterValue::NotRetracted),
            QueryNode::Any(vec![id("pmid:26017442"), id("pmid:31452104")]),
            QueryNode::Any(vec![id("10.1145/3065386"), id("arXiv:1706.03762v5")]),
            id("PMC7254481"),
            id("https://openalex.org/W2919115771"),
            QueryNode::None(vec![id("openalex:W1"), id("pmcid:2")]),
        ]);
        assert_eq!(
            plan(tree).unwrap().filters,
            [
                "doi:10.1038/nature14539",
                "is_retracted:false",
                "ids.pmid:26017442|31452104",
                "doi:10.1145/3065386|10.48550/arxiv.1706.03762",
                "ids.pmcid:PMC7254481",
                "openalex:W2919115771",
                "openalex:!W1",
                "ids.pmcid:!PMC2",
            ]
        );
    }

    #[test]
    fn identifiers_openalex_cant_filter_by() {
        let reason = |tree| plan(tree).unwrap_err();
        assert_eq!(
            reason(id("978-0-262-03561-3")),
            "OpenAlex has no ISBN filter"
        );
        assert_eq!(
            reason(id("NBK526056")),
            "OpenAlex has no NCBI Bookshelf id filter"
        );
        assert_eq!(
            reason(QueryNode::Any(vec![
                id("10.1038/nature14539"),
                id("arxiv:1706.03762"),
                id("pmid:26017442"),
            ])),
            "OpenAlex can't combine DOI, arXiv id and PMID with any, only identifiers of one kind"
        );
        assert_eq!(
            reason(QueryNode::Any(vec![
                id("10.1038/nature14539"),
                term(TermField::Title, "deep learning"),
            ])),
            "OpenAlex can't combine identifier and title with any"
        );
        assert_eq!(
            reason(id("10.1000/a,b")),
            "OpenAlex can't filter by a DOI holding a comma or a pipe"
        );
    }
}
