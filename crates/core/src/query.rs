//! Query trees: all, any and none of terms, filters and relations, for
//! searches that say more than keywords plus one author, title and
//! container ([`SearchQuery::tree`](crate::SearchQuery::tree)).
//!
//! The engine normalises a tree ([`QueryNode::normalised`]) and checks its
//! leaves ([`QueryNode::validate`]) before a provider sees it. Each
//! provider then translates it into its own syntax or says why it can't
//! ([`Provider::express`](crate::Provider::express)), starting from
//! [`check_supported`], which holds the rules [`Capabilities`] alone
//! decide. Sources with Lucene-style query strings (Europe PMC, DataCite,
//! Open Library) share [`render_lucene`].

use serde::{Deserialize, Serialize};

use crate::http::escape_lucene;
use crate::id::WorkId;
use crate::provider::{Capabilities, Filter, YearRange};
use crate::work::WorkType;

/// A search as a tree. Groups hold any number of nodes; leaves are terms
/// in a field, filters and relations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum QueryNode {
    /// Every node matches.
    All(Vec<QueryNode>),
    /// At least one node matches.
    Any(Vec<QueryNode>),
    /// No node matches.
    None(Vec<QueryNode>),
    Term(Term),
    Filter(FilterValue),
    Relation(RelationTerm),
}

/// Words, or a phrase, in one field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Term {
    pub field: TermField,
    /// Searched as written: never parsed for quotes or operators.
    pub value: String,
    /// The words together, in order; otherwise each anywhere in the field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub phrase: bool,
}

/// Where a [`Term`] is searched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TermField {
    /// Wherever the source searches plain text.
    Keywords,
    Title,
    /// An author's name.
    Author,
    /// Where the work appeared: the journal, or the book for a chapter.
    Container,
    Abstract,
    /// An ORCID (bare or as a URL) or an OpenAlex author id (`A…`).
    AuthorId,
}

impl TermField {
    pub const ALL: &[TermField] = &[
        TermField::Keywords,
        TermField::Title,
        TermField::Author,
        TermField::Container,
        TermField::Abstract,
        TermField::AuthorId,
    ];

    /// The capability a source declares to search this field.
    pub fn filter(self) -> Filter {
        match self {
            TermField::Keywords => Filter::Keywords,
            TermField::Title => Filter::Title,
            TermField::Author => Filter::Author,
            TermField::Container => Filter::Container,
            TermField::Abstract => Filter::Abstract,
            TermField::AuthorId => Filter::AuthorId,
        }
    }

    /// In English, for reasons.
    pub fn name(self) -> &'static str {
        match self {
            TermField::Keywords => "keywords",
            TermField::Title => "title",
            TermField::Author => "author",
            TermField::Container => "journal",
            TermField::Abstract => "abstract",
            TermField::AuthorId => "author id",
        }
    }
}

/// A filter as a leaf of a tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum FilterValue {
    Years(YearRange),
    WorkType(WorkType),
    /// Whether a free, legal copy exists.
    OpenAccess(bool),
    /// Leaves out retracted works.
    NotRetracted,
    /// An ISO 639-1 code: `en`, `fr`, ….
    Language(String),
    /// Has an abstract.
    HasAbstract,
}

impl FilterValue {
    /// Every filter (with sample values), for front ends to test their
    /// mappings.
    pub const ALL: &[FilterValue] = &[
        FilterValue::Years(YearRange {
            from: Some(2017),
            to: None,
        }),
        FilterValue::WorkType(WorkType::Book),
        FilterValue::OpenAccess(true),
        FilterValue::NotRetracted,
        FilterValue::Language(String::new()),
        FilterValue::HasAbstract,
    ];

    /// The capability a source declares to apply this filter.
    pub fn filter(&self) -> Filter {
        match self {
            FilterValue::Years(_) => Filter::Years,
            FilterValue::WorkType(_) => Filter::WorkType,
            FilterValue::OpenAccess(_) => Filter::OpenAccess,
            FilterValue::NotRetracted => Filter::NotRetracted,
            FilterValue::Language(_) => Filter::Language,
            FilterValue::HasAbstract => Filter::HasAbstract,
        }
    }
}

/// Works related to another by citation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct RelationTerm {
    pub relation: Relation,
    /// Any identifier aspero can look up; the engine resolves it to the
    /// id the source needs (an OpenAlex work id) before searching.
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Relation {
    /// Works citing the given work.
    Cites,
    /// Works the given work cites.
    CitedBy,
}

impl Relation {
    pub const ALL: &[Relation] = &[Relation::Cites, Relation::CitedBy];

    /// As serialized: `cites` or `cited_by`.
    pub fn as_str(self) -> &'static str {
        match self {
            Relation::Cites => "cites",
            Relation::CitedBy => "cited_by",
        }
    }
}

/// How far a source can combine terms with any or none.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BooleanSupport {
    #[default]
    No,
    /// Only among terms in one field (or values of one filter).
    WithinField,
    /// Across fields.
    Anywhere,
}

impl BooleanSupport {
    pub const ALL: &[BooleanSupport] = &[
        BooleanSupport::No,
        BooleanSupport::WithinField,
        BooleanSupport::Anywhere,
    ];

    /// As serialized: `no`, `within_field` or `anywhere`.
    pub fn as_str(self) -> &'static str {
        match self {
            BooleanSupport::No => "no",
            BooleanSupport::WithinField => "within_field",
            BooleanSupport::Anywhere => "anywhere",
        }
    }
}

impl QueryNode {
    /// Words of `value` in `field`.
    pub fn term(field: TermField, value: impl Into<String>) -> Self {
        QueryNode::Term(Term {
            field,
            value: value.into(),
            phrase: false,
        })
    }

    /// `value` as a phrase in `field`.
    pub fn phrase(field: TermField, value: impl Into<String>) -> Self {
        QueryNode::Term(Term {
            field,
            value: value.into(),
            phrase: true,
        })
    }

    pub fn filter(filter: FilterValue) -> Self {
        QueryNode::Filter(filter)
    }

    pub fn relation(relation: Relation, id: impl Into<String>) -> Self {
        QueryNode::Relation(RelationTerm {
            relation,
            id: id.into(),
        })
    }

    /// The tree with blank leaves and empty groups dropped, groups nested
    /// in a group of the same kind flattened into it, and an all or any of
    /// one node replaced by that node. `None` when nothing is left to
    /// search by.
    pub fn normalised(self) -> Option<QueryNode> {
        match self {
            QueryNode::All(nodes) => group(Group::All, nodes),
            QueryNode::Any(nodes) => group(Group::Any, nodes),
            QueryNode::None(nodes) => group(Group::None, nodes),
            QueryNode::Term(mut term) => {
                term.value = term.value.trim().to_owned();
                (!term.value.is_empty()).then_some(QueryNode::Term(term))
            }
            QueryNode::Filter(FilterValue::Years(y)) if y.from.is_none() && y.to.is_none() => None,
            QueryNode::Filter(FilterValue::Language(code)) => {
                let code = code.trim().to_ascii_lowercase();
                (!code.is_empty()).then_some(QueryNode::Filter(FilterValue::Language(code)))
            }
            QueryNode::Filter(f) => Some(QueryNode::Filter(f)),
            QueryNode::Relation(mut r) => {
                r.id = r.id.trim().to_owned();
                (!r.id.is_empty()).then_some(QueryNode::Relation(r))
            }
        }
    }

    /// Checks every leaf's value: author ids, language codes, year ranges
    /// and relations' identifiers. The reason, in English, for the first
    /// that's invalid.
    pub fn validate(&self) -> Result<(), String> {
        match self {
            QueryNode::All(nodes) | QueryNode::Any(nodes) | QueryNode::None(nodes) => {
                nodes.iter().try_for_each(QueryNode::validate)
            }
            QueryNode::Term(t) if t.field == TermField::AuthorId => {
                AuthorId::parse(&t.value).map(|_| ()).ok_or_else(|| {
                    format!(
                        "{:?} is neither an ORCID nor an OpenAlex author id (A…)",
                        t.value
                    )
                })
            }
            QueryNode::Term(_) => Ok(()),
            QueryNode::Filter(FilterValue::Language(code)) => language(code)
                .map(|_| ())
                .ok_or_else(|| format!("{code:?} is not an ISO 639-1 language code, like \"en\"")),
            QueryNode::Filter(FilterValue::Years(YearRange {
                from: Some(from),
                to: Some(to),
            })) if from > to => Err(format!("the years {from} to {to} run backwards")),
            QueryNode::Filter(_) => Ok(()),
            QueryNode::Relation(r) => {
                r.id.parse::<WorkId>()
                    .map(|_| ())
                    .map_err(|e| format!("can't search by citations of {:?}: {e}", r.id))
            }
        }
    }

    /// Every leaf, depth first.
    pub fn leaves(&self) -> Vec<&QueryNode> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves<'a>(&'a self, out: &mut Vec<&'a QueryNode>) {
        match self {
            QueryNode::All(nodes) | QueryNode::Any(nodes) | QueryNode::None(nodes) => {
                nodes.iter().for_each(|n| n.collect_leaves(out));
            }
            leaf => out.push(leaf),
        }
    }

    /// The field every leaf is a term in, if they share one.
    pub fn single_field(&self) -> Option<TermField> {
        let mut fields = self.leaves().into_iter().map(|leaf| match leaf {
            QueryNode::Term(t) => Some(t.field),
            _ => None,
        });
        let first = fields.next()??;
        fields.all(|f| f == Some(first)).then_some(first)
    }

    /// What a leaf searches by, in English, for reasons: `title`, `years`,
    /// `citations`, ….
    pub fn leaf_name(&self) -> &'static str {
        match self {
            QueryNode::Term(t) => t.field.name(),
            QueryNode::Filter(f) => match f {
                FilterValue::Years(_) => "years",
                FilterValue::WorkType(_) => "work type",
                FilterValue::OpenAccess(_) => "open access",
                FilterValue::NotRetracted => "retraction",
                FilterValue::Language(_) => "language",
                FilterValue::HasAbstract => "having an abstract",
            },
            QueryNode::Relation(r) => match r.relation {
                Relation::Cites => "citing a work",
                Relation::CitedBy => "cited by a work",
            },
            QueryNode::All(_) => "all of",
            QueryNode::Any(_) => "any of",
            QueryNode::None(_) => "none of",
        }
    }

    /// The leaves' names, each once, in order, as an English list: `title
    /// and author`.
    pub fn leaf_names(&self) -> String {
        let mut names: Vec<&str> = Vec::new();
        for leaf in self.leaves() {
            let name = leaf.leaf_name();
            if !names.contains(&name) {
                names.push(name);
            }
        }
        english_list(&names)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    All,
    Any,
    None,
}

fn group(kind: Group, nodes: Vec<QueryNode>) -> Option<QueryNode> {
    let mut flat = Vec::new();
    for node in nodes.into_iter().filter_map(QueryNode::normalised) {
        match (kind, node) {
            (Group::All, QueryNode::All(inner)) | (Group::Any, QueryNode::Any(inner)) => {
                flat.extend(inner);
            }
            (_, node) => flat.push(node),
        }
    }
    match (kind, flat.len()) {
        (_, 0) => None,
        (Group::All | Group::Any, 1) => flat.pop(),
        (Group::All, _) => Some(QueryNode::All(flat)),
        (Group::Any, _) => Some(QueryNode::Any(flat)),
        (Group::None, _) => Some(QueryNode::None(flat)),
    }
}

/// `a`, `a and b`, `a, b and c`.
fn english_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [init @ .., last] => format!("{} and {last}", init.join(", ")),
    }
}

/// An author, by identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AuthorId {
    /// Bare: `0000-0002-1825-0097`.
    Orcid(String),
    /// `A5023888391`.
    OpenAlex(String),
}

impl AuthorId {
    /// An ORCID, bare or as an `orcid.org` URL, with a valid check digit;
    /// or an OpenAlex author id, bare or as an `openalex.org` URL.
    pub fn parse(input: &str) -> Option<AuthorId> {
        let s = input.trim();
        let path = ["https://", "http://"]
            .iter()
            .find_map(|p| s.strip_prefix(p))
            .unwrap_or(s);
        let path = path.strip_prefix("www.").unwrap_or(path);
        if let Some(orcid) = path
            .strip_prefix("orcid.org/")
            .or_else(|| path.strip_prefix("orcid:"))
            .or(Some(path))
            .filter(|o| is_orcid(o))
        {
            return Some(AuthorId::Orcid(orcid.to_ascii_uppercase()));
        }
        let openalex = path.strip_prefix("openalex.org/").unwrap_or(path);
        let digits = openalex.strip_prefix(['A', 'a'])?;
        (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
            .then(|| AuthorId::OpenAlex(format!("A{digits}")))
    }
}

/// `0000-0002-1825-0097`: four groups of four, the last character an
/// ISO 7064 11,2 check digit (`X` for 10).
fn is_orcid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 19 || [4, 9, 14].iter().any(|&i| b[i] != b'-') {
        return false;
    }
    let chars: Vec<u8> = b.iter().copied().filter(|&c| c != b'-').collect();
    let (body, check) = chars.split_at(15);
    if !body.iter().all(u8::is_ascii_digit) {
        return false;
    }
    let total = body.iter().fold(0u32, |t, d| (t + u32::from(d - b'0')) * 2);
    let expected = (12 - total % 11) % 11;
    match check[0] {
        b'X' | b'x' => expected == 10,
        d @ b'0'..=b'9' => u32::from(d - b'0') == expected,
        _ => false,
    }
}

/// An ISO 639-1 code's ISO 639-2 codes: bibliographic (MEDLINE and MARC
/// use these: `fre`, `ger`) and terminology (`fra`, `deu`), the same for
/// most languages.
pub fn language(code: &str) -> Option<(&'static str, &'static str)> {
    let code = code.trim().to_ascii_lowercase();
    LANGUAGES
        .iter()
        .find(|(one, _, _)| *one == code)
        .map(|(_, b, t)| (*b, *t))
}

/// ISO 639-1, ISO 639-2/B, ISO 639-2/T.
const LANGUAGES: &[(&str, &str, &str)] = &[
    ("aa", "aar", "aar"),
    ("ab", "abk", "abk"),
    ("ae", "ave", "ave"),
    ("af", "afr", "afr"),
    ("ak", "aka", "aka"),
    ("am", "amh", "amh"),
    ("an", "arg", "arg"),
    ("ar", "ara", "ara"),
    ("as", "asm", "asm"),
    ("av", "ava", "ava"),
    ("ay", "aym", "aym"),
    ("az", "aze", "aze"),
    ("ba", "bak", "bak"),
    ("be", "bel", "bel"),
    ("bg", "bul", "bul"),
    ("bi", "bis", "bis"),
    ("bm", "bam", "bam"),
    ("bn", "ben", "ben"),
    ("bo", "tib", "bod"),
    ("br", "bre", "bre"),
    ("bs", "bos", "bos"),
    ("ca", "cat", "cat"),
    ("ce", "che", "che"),
    ("ch", "cha", "cha"),
    ("co", "cos", "cos"),
    ("cr", "cre", "cre"),
    ("cs", "cze", "ces"),
    ("cu", "chu", "chu"),
    ("cv", "chv", "chv"),
    ("cy", "wel", "cym"),
    ("da", "dan", "dan"),
    ("de", "ger", "deu"),
    ("dv", "div", "div"),
    ("dz", "dzo", "dzo"),
    ("ee", "ewe", "ewe"),
    ("el", "gre", "ell"),
    ("en", "eng", "eng"),
    ("eo", "epo", "epo"),
    ("es", "spa", "spa"),
    ("et", "est", "est"),
    ("eu", "baq", "eus"),
    ("fa", "per", "fas"),
    ("ff", "ful", "ful"),
    ("fi", "fin", "fin"),
    ("fj", "fij", "fij"),
    ("fo", "fao", "fao"),
    ("fr", "fre", "fra"),
    ("fy", "fry", "fry"),
    ("ga", "gle", "gle"),
    ("gd", "gla", "gla"),
    ("gl", "glg", "glg"),
    ("gn", "grn", "grn"),
    ("gu", "guj", "guj"),
    ("gv", "glv", "glv"),
    ("ha", "hau", "hau"),
    ("he", "heb", "heb"),
    ("hi", "hin", "hin"),
    ("ho", "hmo", "hmo"),
    ("hr", "hrv", "hrv"),
    ("ht", "hat", "hat"),
    ("hu", "hun", "hun"),
    ("hy", "arm", "hye"),
    ("hz", "her", "her"),
    ("ia", "ina", "ina"),
    ("id", "ind", "ind"),
    ("ie", "ile", "ile"),
    ("ig", "ibo", "ibo"),
    ("ii", "iii", "iii"),
    ("ik", "ipk", "ipk"),
    ("io", "ido", "ido"),
    ("is", "ice", "isl"),
    ("it", "ita", "ita"),
    ("iu", "iku", "iku"),
    ("ja", "jpn", "jpn"),
    ("jv", "jav", "jav"),
    ("ka", "geo", "kat"),
    ("kg", "kon", "kon"),
    ("ki", "kik", "kik"),
    ("kj", "kua", "kua"),
    ("kk", "kaz", "kaz"),
    ("kl", "kal", "kal"),
    ("km", "khm", "khm"),
    ("kn", "kan", "kan"),
    ("ko", "kor", "kor"),
    ("kr", "kau", "kau"),
    ("ks", "kas", "kas"),
    ("ku", "kur", "kur"),
    ("kv", "kom", "kom"),
    ("kw", "cor", "cor"),
    ("ky", "kir", "kir"),
    ("la", "lat", "lat"),
    ("lb", "ltz", "ltz"),
    ("lg", "lug", "lug"),
    ("li", "lim", "lim"),
    ("ln", "lin", "lin"),
    ("lo", "lao", "lao"),
    ("lt", "lit", "lit"),
    ("lu", "lub", "lub"),
    ("lv", "lav", "lav"),
    ("mg", "mlg", "mlg"),
    ("mh", "mah", "mah"),
    ("mi", "mao", "mri"),
    ("mk", "mac", "mkd"),
    ("ml", "mal", "mal"),
    ("mn", "mon", "mon"),
    ("mr", "mar", "mar"),
    ("ms", "may", "msa"),
    ("mt", "mlt", "mlt"),
    ("my", "bur", "mya"),
    ("na", "nau", "nau"),
    ("nb", "nob", "nob"),
    ("nd", "nde", "nde"),
    ("ne", "nep", "nep"),
    ("ng", "ndo", "ndo"),
    ("nl", "dut", "nld"),
    ("nn", "nno", "nno"),
    ("no", "nor", "nor"),
    ("nr", "nbl", "nbl"),
    ("nv", "nav", "nav"),
    ("ny", "nya", "nya"),
    ("oc", "oci", "oci"),
    ("oj", "oji", "oji"),
    ("om", "orm", "orm"),
    ("or", "ori", "ori"),
    ("os", "oss", "oss"),
    ("pa", "pan", "pan"),
    ("pi", "pli", "pli"),
    ("pl", "pol", "pol"),
    ("ps", "pus", "pus"),
    ("pt", "por", "por"),
    ("qu", "que", "que"),
    ("rm", "roh", "roh"),
    ("rn", "run", "run"),
    ("ro", "rum", "ron"),
    ("ru", "rus", "rus"),
    ("rw", "kin", "kin"),
    ("sa", "san", "san"),
    ("sc", "srd", "srd"),
    ("sd", "snd", "snd"),
    ("se", "sme", "sme"),
    ("sg", "sag", "sag"),
    ("si", "sin", "sin"),
    ("sk", "slo", "slk"),
    ("sl", "slv", "slv"),
    ("sm", "smo", "smo"),
    ("sn", "sna", "sna"),
    ("so", "som", "som"),
    ("sq", "alb", "sqi"),
    ("sr", "srp", "srp"),
    ("ss", "ssw", "ssw"),
    ("st", "sot", "sot"),
    ("su", "sun", "sun"),
    ("sv", "swe", "swe"),
    ("sw", "swa", "swa"),
    ("ta", "tam", "tam"),
    ("te", "tel", "tel"),
    ("tg", "tgk", "tgk"),
    ("th", "tha", "tha"),
    ("ti", "tir", "tir"),
    ("tk", "tuk", "tuk"),
    ("tl", "tgl", "tgl"),
    ("tn", "tsn", "tsn"),
    ("to", "ton", "ton"),
    ("tr", "tur", "tur"),
    ("ts", "tso", "tso"),
    ("tt", "tat", "tat"),
    ("tw", "twi", "twi"),
    ("ty", "tah", "tah"),
    ("ug", "uig", "uig"),
    ("uk", "ukr", "ukr"),
    ("ur", "urd", "urd"),
    ("uz", "uzb", "uzb"),
    ("ve", "ven", "ven"),
    ("vi", "vie", "vie"),
    ("vo", "vol", "vol"),
    ("wa", "wln", "wln"),
    ("wo", "wol", "wol"),
    ("xh", "xho", "xho"),
    ("yi", "yid", "yid"),
    ("yo", "yor", "yor"),
    ("za", "zha", "zha"),
    ("zh", "chi", "zho"),
    ("zu", "zul", "zul"),
];

/// What [`Capabilities`] alone decide about a normalised tree: the fields,
/// filters and relations it uses, any and none if the source has neither,
/// and groups within groups. Providers call it first in
/// [`Provider::express`](crate::Provider::express), then check what's
/// particular to their syntax. `source` names the provider in reasons:
/// "Crossref can't search by abstract".
pub fn check_supported(tree: &QueryNode, caps: &Capabilities, source: &str) -> Result<(), String> {
    check_node(tree, caps, source, 0)
}

fn check_node(
    node: &QueryNode,
    caps: &Capabilities,
    source: &str,
    depth: usize,
) -> Result<(), String> {
    let nodes = match node {
        QueryNode::All(nodes) => nodes,
        QueryNode::Any(nodes) => {
            if caps.search_any == BooleanSupport::No {
                return Err(format!("{source} can't combine terms with any"));
            }
            nodes
        }
        QueryNode::None(nodes) => {
            if caps.search_none == BooleanSupport::No {
                return Err(format!("{source} can't leave out terms with none"));
            }
            nodes
        }
        QueryNode::Term(t) => {
            return match caps.search_filters.contains(&t.field.filter()) {
                true => Ok(()),
                false => Err(format!("{source} can't search by {}", t.field.name())),
            };
        }
        QueryNode::Filter(f) => {
            return match caps.search_filters.contains(&f.filter()) {
                true => Ok(()),
                false => Err(match f {
                    FilterValue::Years(_) => format!("{source} can't filter by year"),
                    FilterValue::WorkType(_) => format!("{source} can't filter by work type"),
                    FilterValue::OpenAccess(_) => format!("{source} can't filter by open access"),
                    FilterValue::NotRetracted => {
                        format!("{source} can't leave out retracted works")
                    }
                    FilterValue::Language(_) => format!("{source} can't filter by language"),
                    FilterValue::HasAbstract => {
                        format!("{source} can't filter by whether a work has an abstract")
                    }
                }),
            };
        }
        QueryNode::Relation(r) => {
            return match caps.relations.contains(&r.relation) {
                true => Ok(()),
                false => Err(match r.relation {
                    Relation::Cites => format!("{source} can't find the works citing a work"),
                    Relation::CitedBy => format!("{source} can't find the works a work cites"),
                }),
            };
        }
    };
    if depth > 0 && !caps.search_nesting {
        return Err(format!("{source} can't put groups inside groups"));
    }
    nodes
        .iter()
        .try_for_each(|n| check_node(n, caps, source, depth + 1))
}

/// Renders a tree in Lucene query syntax, as Europe PMC, DataCite and Open
/// Library take it: `a AND b`, `(a OR b)`, `(NOT (a OR b))`. `leaf` renders
/// each term, filter and relation (see [`lucene_term`]), or says why it
/// can't.
pub fn render_lucene(
    tree: &QueryNode,
    leaf: &mut dyn FnMut(&QueryNode) -> Result<String, String>,
) -> Result<String, String> {
    render(tree, leaf, false)
}

fn render(
    node: &QueryNode,
    leaf: &mut dyn FnMut(&QueryNode) -> Result<String, String>,
    nested: bool,
) -> Result<String, String> {
    let join = |nodes: &[QueryNode], op: &str, leaf: &mut dyn FnMut(&QueryNode) -> _| {
        nodes
            .iter()
            .map(|n| render(n, leaf, true))
            .collect::<Result<Vec<String>, String>>()
            .map(|parts| parts.join(op))
    };
    let wrap = |s: String| if nested { format!("({s})") } else { s };
    Ok(match node {
        QueryNode::All(nodes) => wrap(join(nodes, " AND ", leaf)?),
        QueryNode::Any(nodes) => wrap(join(nodes, " OR ", leaf)?),
        // Bracketed even at the top: `x AND (NOT y)` works wherever `x AND
        // NOT y` does, and in OR groups too.
        QueryNode::None(nodes) => match nodes.as_slice() {
            [one] => format!("(NOT {})", render(one, leaf, true)?),
            _ => format!("(NOT ({}))", join(nodes, " OR ", leaf)?),
        },
        leaf_node => leaf(leaf_node)?,
    })
}

/// A term in Lucene syntax: `PREFIX:"a phrase"`, `PREFIX:(some words)`,
/// or without a prefix, `"a phrase"` or `(some words)`. The value is
/// escaped ([`lucene_words`]).
pub fn lucene_term(prefix: Option<&str>, value: &str, phrase: bool) -> String {
    let field = prefix.map(|p| format!("{p}:")).unwrap_or_default();
    match phrase {
        true => format!("{field}\"{}\"", escape_lucene(value)),
        false => format!("{field}({})", lucene_words(value)),
    }
}

/// User text as words, never syntax: special characters escaped, and the
/// operators `AND`, `OR` and `NOT` (as words) quoted, so they are searched
/// for rather than obeyed.
pub fn lucene_words(text: &str) -> String {
    escape_lucene(text)
        .split_whitespace()
        .map(|word| match word {
            "AND" | "OR" | "NOT" => format!("\"{}\"", word.to_lowercase()),
            word => word.to_owned(),
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A year range in Lucene syntax: `FIELD:[2017 TO *]`.
pub fn lucene_years(field: &str, years: YearRange) -> String {
    let end = |y: Option<i16>| y.map_or("*".to_owned(), |y| y.to_string());
    format!("{field}:[{} TO {}]", end(years.from), end(years.to))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(v: &str) -> QueryNode {
        QueryNode::term(TermField::Title, v)
    }

    #[test]
    fn normalising_flattens_drops_and_unwraps() {
        let tree = QueryNode::All(vec![
            QueryNode::All(vec![title("a"), QueryNode::Any(vec![])]),
            QueryNode::Any(vec![
                QueryNode::Any(vec![title("b"), title(" ")]),
                title("c"),
            ]),
            QueryNode::None(vec![QueryNode::All(vec![title("d")])]),
            QueryNode::filter(FilterValue::Years(YearRange::default())),
        ]);
        assert_eq!(
            tree.normalised(),
            Some(QueryNode::All(vec![
                title("a"),
                QueryNode::Any(vec![title("b"), title("c")]),
                QueryNode::None(vec![title("d")]),
            ]))
        );
        // An all of one any is the any, flattened into its parent.
        let tree = QueryNode::Any(vec![
            QueryNode::All(vec![QueryNode::Any(vec![title("a"), title("b")])]),
            title("c"),
        ]);
        assert_eq!(
            tree.normalised(),
            Some(QueryNode::Any(vec![title("a"), title("b"), title("c")]))
        );
        assert_eq!(
            QueryNode::All(vec![QueryNode::None(vec![])]).normalised(),
            None
        );
        assert_eq!(title("  ").normalised(), None);
        assert_eq!(
            title("  x ").normalised(),
            Some(title("x")),
            "terms are trimmed"
        );
    }

    #[test]
    fn author_ids_are_orcids_or_openalex_ids() {
        let orcid = |s| AuthorId::Orcid(String::from(s));
        assert_eq!(
            AuthorId::parse("0000-0002-1825-0097"),
            Some(orcid("0000-0002-1825-0097"))
        );
        assert_eq!(
            AuthorId::parse("https://orcid.org/0000-0002-1694-233x"),
            Some(orcid("0000-0002-1694-233X"))
        );
        assert_eq!(
            AuthorId::parse("A5023888391"),
            Some(AuthorId::OpenAlex("A5023888391".into()))
        );
        assert_eq!(
            AuthorId::parse("https://openalex.org/a5023888391"),
            Some(AuthorId::OpenAlex("A5023888391".into()))
        );
        // Wrong check digit; a work id; a name.
        assert_eq!(AuthorId::parse("0000-0002-1825-0098"), None);
        assert_eq!(AuthorId::parse("W2919115771"), None);
        assert_eq!(AuthorId::parse("LeCun"), None);
        assert_eq!(AuthorId::parse("A"), None);
    }

    #[test]
    fn validation_names_the_bad_leaf() {
        let bad_id = QueryNode::All(vec![
            title("x"),
            QueryNode::term(TermField::AuthorId, "Yann LeCun"),
        ]);
        assert_eq!(
            bad_id.validate().unwrap_err(),
            "\"Yann LeCun\" is neither an ORCID nor an OpenAlex author id (A…)"
        );
        let lang = QueryNode::filter(FilterValue::Language("eng".into()));
        assert!(lang.validate().unwrap_err().contains("ISO 639-1"));
        assert!(
            QueryNode::filter(FilterValue::Language("en".into()))
                .validate()
                .is_ok()
        );
        let years = QueryNode::filter(FilterValue::Years(YearRange {
            from: Some(2020),
            to: Some(2010),
        }));
        assert_eq!(
            years.validate().unwrap_err(),
            "the years 2020 to 2010 run backwards"
        );
        let relation = QueryNode::relation(Relation::Cites, "not an id");
        assert!(
            relation
                .validate()
                .unwrap_err()
                .starts_with("can't search by citations")
        );
        assert!(
            QueryNode::relation(Relation::CitedBy, "10.1038/nature14539")
                .validate()
                .is_ok()
        );
    }

    #[test]
    fn languages_map_to_both_three_letter_forms() {
        assert_eq!(language("en"), Some(("eng", "eng")));
        assert_eq!(language("FR"), Some(("fre", "fra")));
        assert_eq!(language("de"), Some(("ger", "deu")));
        assert_eq!(language("xx"), None);
        assert_eq!(LANGUAGES.len(), 183);
        assert!(LANGUAGES.windows(2).all(|w| w[0].0 < w[1].0));
    }

    fn caps(any: BooleanSupport, none: BooleanSupport, nesting: bool) -> Capabilities {
        Capabilities {
            search_filters: vec![Filter::Keywords, Filter::Title, Filter::Years],
            search_any: any,
            search_none: none,
            search_nesting: nesting,
            ..Capabilities::default()
        }
    }

    #[test]
    fn capabilities_decide_fields_filters_groups_and_relations() {
        let flat = caps(BooleanSupport::No, BooleanSupport::No, false);
        let check = |tree: &QueryNode, caps: &Capabilities| check_supported(tree, caps, "Src");
        assert_eq!(check(&title("x"), &flat), Ok(()));
        assert_eq!(
            check(&QueryNode::term(TermField::Abstract, "x"), &flat).unwrap_err(),
            "Src can't search by abstract"
        );
        assert_eq!(
            check(&QueryNode::filter(FilterValue::NotRetracted), &flat).unwrap_err(),
            "Src can't leave out retracted works"
        );
        assert_eq!(
            check(&QueryNode::relation(Relation::Cites, "W1"), &flat).unwrap_err(),
            "Src can't find the works citing a work"
        );
        assert_eq!(
            check(&QueryNode::Any(vec![title("a"), title("b")]), &flat).unwrap_err(),
            "Src can't combine terms with any"
        );
        assert_eq!(
            check(&QueryNode::None(vec![title("a")]), &flat).unwrap_err(),
            "Src can't leave out terms with none"
        );
        let shallow = caps(BooleanSupport::Anywhere, BooleanSupport::Anywhere, false);
        let nested = QueryNode::All(vec![
            title("a"),
            QueryNode::Any(vec![title("b"), title("c")]),
        ]);
        assert_eq!(
            check(&nested, &shallow).unwrap_err(),
            "Src can't put groups inside groups"
        );
        let deep = caps(BooleanSupport::Anywhere, BooleanSupport::Anywhere, true);
        assert_eq!(check(&nested, &deep), Ok(()));
    }

    #[test]
    fn lucene_rendering_groups_negates_and_escapes() {
        let tree = QueryNode::All(vec![
            QueryNode::term(TermField::Keywords, "protein: folding"),
            QueryNode::Any(vec![
                QueryNode::phrase(TermField::Title, "deep \"learning\""),
                title("cats AND dogs"),
            ]),
            QueryNode::None(vec![title("a"), title("b")]),
            QueryNode::None(vec![title("c")]),
        ]);
        let mut leaf = |n: &QueryNode| match n {
            QueryNode::Term(t) => Ok(lucene_term(
                (t.field == TermField::Title).then_some("TITLE"),
                &t.value,
                t.phrase,
            )),
            _ => Err("no".to_owned()),
        };
        assert_eq!(
            render_lucene(&tree, &mut leaf).unwrap(),
            r#"(protein\: folding) AND (TITLE:"deep \"learning\"" OR TITLE:(cats "and" dogs)) AND (NOT (TITLE:(a) OR TITLE:(b))) AND (NOT TITLE:(c))"#
        );
        let top = QueryNode::None(vec![title("a")]);
        assert_eq!(render_lucene(&top, &mut leaf).unwrap(), "(NOT TITLE:(a))");
        let bad = QueryNode::All(vec![
            title("a"),
            QueryNode::filter(FilterValue::HasAbstract),
        ]);
        assert_eq!(render_lucene(&bad, &mut leaf).unwrap_err(), "no");
        assert_eq!(
            lucene_years(
                "PUB_YEAR",
                YearRange {
                    from: Some(2017),
                    to: None
                }
            ),
            "PUB_YEAR:[2017 TO *]"
        );
    }

    #[test]
    fn single_fields_and_leaf_names() {
        let same = QueryNode::Any(vec![title("a"), QueryNode::None(vec![title("b")])]);
        assert_eq!(same.single_field(), Some(TermField::Title));
        let mixed = QueryNode::Any(vec![
            title("a"),
            QueryNode::term(TermField::Author, "b"),
            title("c"),
            QueryNode::filter(FilterValue::NotRetracted),
        ]);
        assert_eq!(mixed.single_field(), None);
        assert_eq!(mixed.leaf_names(), "title, author and retraction");
    }

    #[test]
    fn every_value_is_listed() {
        let position = |f: &FilterValue| match f {
            FilterValue::Years(_) => 0,
            FilterValue::WorkType(_) => 1,
            FilterValue::OpenAccess(_) => 2,
            FilterValue::NotRetracted => 3,
            FilterValue::Language(_) => 4,
            FilterValue::HasAbstract => 5,
        };
        for (i, f) in FilterValue::ALL.iter().enumerate() {
            assert_eq!(position(f), i);
        }
        assert_eq!(FilterValue::ALL.len(), 6);
        let position = |f: TermField| match f {
            TermField::Keywords => 0,
            TermField::Title => 1,
            TermField::Author => 2,
            TermField::Container => 3,
            TermField::Abstract => 4,
            TermField::AuthorId => 5,
        };
        for (i, f) in TermField::ALL.iter().enumerate() {
            assert_eq!(position(*f), i);
            assert!(f.filter().is_field());
        }
        assert_eq!(TermField::ALL.len(), 6);
        let position = |r: Relation| match r {
            Relation::Cites => 0,
            Relation::CitedBy => 1,
        };
        for (i, r) in Relation::ALL.iter().enumerate() {
            assert_eq!(position(*r), i);
            assert_eq!(serde_json::to_value(r).unwrap(), r.as_str());
        }
        assert_eq!(Relation::ALL.len(), 2);
        let position = |b: BooleanSupport| match b {
            BooleanSupport::No => 0,
            BooleanSupport::WithinField => 1,
            BooleanSupport::Anywhere => 2,
        };
        for (i, b) in BooleanSupport::ALL.iter().enumerate() {
            assert_eq!(position(*b), i);
            assert_eq!(serde_json::to_value(b).unwrap(), b.as_str());
        }
        assert_eq!(BooleanSupport::ALL.len(), 3);
    }

    #[test]
    fn trees_serialize_compactly() {
        let tree = QueryNode::All(vec![
            title("a"),
            QueryNode::phrase(TermField::Author, "b"),
            QueryNode::filter(FilterValue::NotRetracted),
            QueryNode::relation(Relation::CitedBy, "W1"),
        ]);
        let json = serde_json::to_string(&tree).unwrap();
        assert_eq!(
            json,
            r#"{"all":[{"term":{"field":"title","value":"a"}},{"term":{"field":"author","value":"b","phrase":true}},{"filter":"not_retracted"},{"relation":{"relation":"cited_by","id":"W1"}}]}"#
        );
        assert_eq!(serde_json::from_str::<QueryNode>(&json).unwrap(), tree);
    }
}
