//! Works written out as BibTeX entries or RIS records, for pasting into a
//! reference manager.

use std::fmt::Write as _;

use aspero_core::{Person, Work, WorkId, WorkType};

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// A conventional BibTeX key: the first author's family name, the year
/// and the title's first word, `lecun2015deep`. Accents are dropped;
/// whatever is missing is left out, and an empty key becomes `work`.
pub fn cite_key(work: &Work) -> String {
    let first = work.authors.first().map(name);
    let name = first.as_ref().and_then(|n| match n {
        Name::Person { family, .. } => Some(family.as_str()),
        Name::Whole(whole) => whole.split_whitespace().next(),
    });
    let word = work.title.as_deref().and_then(|t| {
        t.split(|c: char| !c.is_alphanumeric())
            .find(|w| !w.is_empty() && !STOP.contains(&w.to_lowercase().as_str()))
    });
    let year = work.issued.map(|d| d.year().to_string());
    let key: String = [name, year.as_deref(), word]
        .into_iter()
        .flatten()
        .flat_map(|part| part.chars().map(ascii))
        .flatten()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    if key.is_empty() { "work".into() } else { key }
}

/// Cite keys kept distinct across the entries of one file:
/// `lecun2015deep`, then `lecun2015deepa`, `lecun2015deepb`, ….
#[derive(Debug, Default)]
pub struct Keys(Vec<String>);

impl Keys {
    /// A key for `work` not given before.
    pub fn key(&mut self, work: &Work) -> String {
        let base = cite_key(work);
        let key = std::iter::once(base.clone())
            .chain(('a'..='z').map(|c| format!("{base}{c}")))
            .chain((2..).map(|n| format!("{base}-{n}")))
            .find(|k| !self.0.contains(k))
            .expect("unbounded");
        self.0.push(key.clone());
        key
    }
}

const STOP: [&str; 9] = ["a", "an", "the", "on", "of", "in", "and", "for", "to"];

/// A name as BibTeX and RIS want it: family and given names apart, or
/// whole (an organisation).
pub(crate) enum Name {
    Person {
        family: String,
        given: Option<String>,
    },
    Whole(String),
}

/// Lowercase words that join a family name: "van Gogh", "de la Cruz".
const PARTICLES: [&str; 14] = [
    "van", "von", "de", "der", "den", "da", "di", "du", "la", "le", "del", "dos", "ten", "ter",
];

/// Words that make a name an organisation's.
const BODIES: [&str; 40] = [
    "university",
    "institute",
    "institution",
    "group",
    "team",
    "organization",
    "organisation",
    "consortium",
    "committee",
    "association",
    "society",
    "collaboration",
    "network",
    "centre",
    "center",
    "foundation",
    "agency",
    "council",
    "department",
    "ministry",
    "office",
    "board",
    "federation",
    "alliance",
    "project",
    "initiative",
    "commission",
    "programme",
    "program",
    "inc",
    "ltd",
    "corporation",
    "company",
    "trust",
    "investigators",
    "study",
    "panel",
    "laboratory",
    "hospital",
    "college",
];

pub(crate) fn name(p: &Person) -> Name {
    match (&p.family, &p.literal) {
        (Some(family), _) => Name::Person {
            family: family.clone(),
            given: p.given.clone(),
        },
        (None, Some(literal)) => {
            split_literal(literal).unwrap_or_else(|| Name::Whole(literal.clone()))
        }
        (None, None) => Name::Whole(p.given.clone().unwrap_or_default()),
    }
}

/// A personal name written whole ("Yann LeCun", "Vincent van Gogh"), as
/// family and given names; `None` for anything that may be an
/// organisation's.
fn split_literal(literal: &str) -> Option<Name> {
    let words: Vec<&str> = literal.split_whitespace().collect();
    let personal = (2..=5).contains(&words.len())
        && words.iter().all(|w| {
            let lower = w.trim_end_matches(['.', ',']).to_lowercase();
            let capital = w.chars().next().is_some_and(char::is_uppercase);
            let acronym = w.chars().count() > 2 && w.chars().all(|c| !c.is_lowercase());
            (capital || PARTICLES.contains(&lower.as_str()))
                && !acronym
                && !BODIES.contains(&lower.as_str())
                && !w.chars().any(|c| c.is_ascii_digit())
        });
    if !personal {
        return None;
    }
    let mut start = words.len() - 1;
    while start > 1 && PARTICLES.contains(&words[start - 1]) {
        start -= 1;
    }
    Some(Name::Person {
        family: words[start..].join(" "),
        given: Some(words[..start].join(" ")),
    })
}

/// `c` without its accent (Latin letters), or as it is.
fn ascii(c: char) -> Option<char> {
    let base = match c {
        'à'..='å' | 'ā' | 'ą' => 'a',
        'ç' | 'ć' | 'č' => 'c',
        'è'..='ë' | 'ē' | 'ę' | 'ě' => 'e',
        'ì'..='ï' | 'ī' => 'i',
        'ñ' | 'ń' | 'ň' => 'n',
        'ò'..='ö' | 'ø' | 'ō' | 'ő' => 'o',
        'ù'..='ü' | 'ū' | 'ů' | 'ű' => 'u',
        'ý' | 'ÿ' => 'y',
        'ł' => 'l',
        'ś' | 'š' | 'ß' => 's',
        'ź' | 'ż' | 'ž' => 'z',
        'ř' => 'r',
        'À'..='Å' => 'A',
        'Ç' | 'Č' => 'C',
        'È'..='Ë' => 'E',
        'Ö' | 'Ø' => 'O',
        'Ü' => 'U',
        'Ł' => 'L',
        'Š' | 'Ś' => 'S',
        'Ž' => 'Z',
        _ => c,
    };
    base.is_ascii().then_some(base)
}

/// `work` as a BibTeX entry under `key`, with the identifiers in `ids`
/// that BibTeX has fields for.
pub fn bibtex(work: &Work, ids: &[WorkId], key: &str) -> String {
    let entry_type = match &work.work_type {
        Some(WorkType::JournalArticle | WorkType::Review) => "article",
        Some(WorkType::ConferencePaper) => "inproceedings",
        Some(WorkType::Book) => "book",
        Some(WorkType::Chapter) => "incollection",
        Some(WorkType::Thesis) => "phdthesis",
        Some(WorkType::Report) => "techreport",
        _ => "misc",
    };
    let mut fields: Vec<(&str, String)> = Vec::new();
    let names: Vec<String> = work.authors.iter().filter_map(bibtex_name).collect();
    if !names.is_empty() {
        fields.push(("author", names.join(" and ")));
    }
    if let Some(title) = &work.title {
        fields.push(("title", escape(title)));
    }
    if let Some(container) = &work.container_title {
        match entry_type {
            "article" => fields.push(("journal", escape(container))),
            "inproceedings" | "incollection" => fields.push(("booktitle", escape(container))),
            _ => {}
        }
    }
    if let Some(date) = work.issued {
        fields.push(("year", date.year().to_string()));
    }
    for (name, value) in [("volume", &work.volume), ("number", &work.issue)] {
        if let Some(v) = value {
            fields.push((name, escape(v)));
        }
    }
    if let Some(page) = &work.page {
        fields.push(("pages", escape(&page.replace(['-', '–', '—'], "--"))));
    }
    if let Some(publisher) = &work.publisher {
        let name = match entry_type {
            "phdthesis" => "school",
            "techreport" => "institution",
            _ => "publisher",
        };
        fields.push((name, escape(publisher)));
    }
    for id in ids {
        match id {
            WorkId::Doi(doi) => fields.push(("doi", doi.to_string())),
            WorkId::Isbn(isbn) => fields.push(("isbn", isbn.to_string())),
            WorkId::Pmid(pmid) => fields.push(("pmid", pmid.to_string())),
            WorkId::Arxiv(arxiv) => {
                fields.push(("eprint", arxiv.to_string()));
                fields.push(("archiveprefix", "arXiv".into()));
            }
            _ => {}
        }
    }
    if let Some(url) = &work.url {
        fields.push(("url", url.clone()));
    }
    // One of each field: the first identifier of a kind wins.
    let mut seen = Vec::new();
    fields.retain(|(name, _)| {
        let new = !seen.contains(name);
        seen.push(*name);
        new
    });

    let mut out = format!("@{entry_type}{{{key},\n");
    for (name, value) in fields {
        let _ = writeln!(out, "  {name} = {{{value}}},");
    }
    if let Some(month) = work.issued.and_then(|d| d.month()) {
        let _ = writeln!(out, "  month = {},", MONTHS[usize::from(month) - 1]);
    }
    out.push_str("}\n");
    out
}

fn bibtex_name(p: &Person) -> Option<String> {
    match name(p) {
        Name::Person {
            family,
            given: Some(given),
        } => Some(format!("{}, {}", escape(&family), escape(&given))),
        Name::Person { family, .. } => Some(escape(&family)),
        Name::Whole(whole) if whole.is_empty() => None,
        // Braced, so an organisation stays one name.
        Name::Whole(whole) => Some(format!("{{{}}}", escape(&whole))),
    }
}

/// Text made safe inside a braced BibTeX field.
fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// `work` as an RIS record, with the identifiers in `ids` that RIS has
/// tags for.
pub fn ris(work: &Work, ids: &[WorkId]) -> String {
    let ty = match &work.work_type {
        Some(WorkType::JournalArticle | WorkType::Review) => "JOUR",
        Some(WorkType::ConferencePaper) => "CPAPER",
        Some(WorkType::Preprint) => "UNPB",
        Some(WorkType::Book) => "BOOK",
        Some(WorkType::Chapter) => "CHAP",
        Some(WorkType::Thesis) => "THES",
        Some(WorkType::Report) => "RPRT",
        Some(WorkType::Dataset) => "DATA",
        Some(WorkType::Software) => "COMP",
        _ => "GEN",
    };
    let mut out = String::new();
    let mut line = |tag: &str, value: &str| {
        let value = value.split_whitespace().collect::<Vec<_>>().join(" ");
        let _ = writeln!(out, "{tag}  - {value}");
    };
    line("TY", ty);
    for p in &work.authors {
        match name(p) {
            Name::Person {
                family,
                given: Some(given),
            } => line("AU", &format!("{family}, {given}")),
            Name::Person { family: name, .. } | Name::Whole(name) if !name.is_empty() => {
                line("AU", &name)
            }
            _ => {}
        }
    }
    if let Some(title) = &work.title {
        line("TI", title);
    }
    if let Some(container) = &work.container_title {
        line("T2", container);
    }
    if let Some(date) = work.issued {
        line("PY", &date.year().to_string());
        if let Some(month) = date.month() {
            let day = date.day().map(|d| format!("{d:02}")).unwrap_or_default();
            line("DA", &format!("{}/{month:02}/{day}/", date.year()));
        }
    }
    if let Some(v) = &work.volume {
        line("VL", v);
    }
    if let Some(v) = &work.issue {
        line("IS", v);
    }
    if let Some(page) = &work.page {
        match page.split_once(['-', '–', '—']) {
            Some((start, end)) => {
                line("SP", start.trim());
                line("EP", end.trim_start_matches(['-', '–']).trim());
            }
            None => line("SP", page),
        }
    }
    if let Some(publisher) = &work.publisher {
        line("PB", publisher);
    }
    for id in ids {
        match id {
            WorkId::Doi(doi) => line("DO", &doi.to_string()),
            WorkId::Isbn(isbn) => line("SN", &isbn.to_string()),
            WorkId::Arxiv(arxiv) if work.url.is_none() => {
                line("UR", &format!("https://arxiv.org/abs/{arxiv}"));
            }
            _ => {}
        }
    }
    if let Some(url) = &work.url {
        line("UR", url);
    }
    line("ER", "");
    out
}

#[cfg(test)]
mod tests {
    use aspero_core::PartialDate;

    use super::*;

    fn lecun() -> (Work, Vec<WorkId>) {
        let mut w = Work::default();
        w.title = Some("Deep learning: 100% of it & more".into());
        w.authors = vec![
            Person::new("Yann", "LeCun"),
            Person::new("Yoshua", "Bengio"),
            Person::literal("Google Brain Team"),
        ];
        w.issued = PartialDate::new(2015, Some(5), Some(27));
        w.work_type = Some(WorkType::JournalArticle);
        w.container_title = Some("Nature".into());
        w.volume = Some("521".into());
        w.issue = Some("7553".into());
        w.page = Some("436-444".into());
        let ids = vec![
            "doi:10.1038/nature14539".parse().unwrap(),
            "pmid:26017442".parse().unwrap(),
        ];
        (w, ids)
    }

    #[test]
    fn bibtex_entries() {
        let (w, ids) = lecun();
        assert_eq!(cite_key(&w), "lecun2015deep");
        assert_eq!(
            bibtex(&w, &ids, &cite_key(&w)),
            "@article{lecun2015deep,
  author = {LeCun, Yann and Bengio, Yoshua and {Google Brain Team}},
  title = {Deep learning: 100\\% of it \\& more},
  journal = {Nature},
  year = {2015},
  volume = {521},
  number = {7553},
  pages = {436--444},
  doi = {10.1038/nature14539},
  pmid = {26017442},
  month = may,
}
"
        );
    }

    #[test]
    fn ris_records() {
        let (w, ids) = lecun();
        assert_eq!(
            ris(&w, &ids),
            "TY  - JOUR
AU  - LeCun, Yann
AU  - Bengio, Yoshua
AU  - Google Brain Team
TI  - Deep learning: 100% of it & more
T2  - Nature
PY  - 2015
DA  - 2015/05/27/
VL  - 521
IS  - 7553
SP  - 436
EP  - 444
DO  - 10.1038/nature14539
ER  -\x20
"
        );
    }

    #[test]
    fn written_entries_read_back() {
        let (w, ids) = lecun();
        let entries = crate::parse(&bibtex(&w, &ids, "k")).unwrap();
        assert_eq!(entries[0].claim.ids, ids);
        assert_eq!(entries[0].claim.authors[0], "LeCun, Yann");
        let entries = crate::parse_ris(&ris(&w, &ids)).unwrap();
        assert_eq!(entries[0].claim.page.as_deref(), Some("436-444"));
        assert_eq!(entries[0].claim.ids, ids[..1]);
    }

    #[test]
    fn whole_names_are_split_unless_an_organisation() {
        let split = |s: &str| match name(&Person::literal(s)) {
            Name::Person { family, given } => format!("{family}, {}", given.unwrap_or_default()),
            Name::Whole(whole) => format!("{{{whole}}}"),
        };
        assert_eq!(split("Yann LeCun"), "LeCun, Yann");
        assert_eq!(split("Geoffrey E. Hinton"), "Hinton, Geoffrey E.");
        assert_eq!(split("Vincent van Gogh"), "van Gogh, Vincent");
        assert_eq!(split("Łukasz Kaiser"), "Kaiser, Łukasz");
        assert_eq!(
            split("World Health Organization"),
            "{World Health Organization}"
        );
        assert_eq!(split("NICE"), "{NICE}");
        assert_eq!(
            split("The RECOVERY Collaborative Group"),
            "{The RECOVERY Collaborative Group}"
        );
        assert_eq!(split("Google Brain Team"), "{Google Brain Team}");
    }

    #[test]
    fn keys_stay_distinct() {
        let (w, _) = lecun();
        let mut keys = Keys::default();
        let three: Vec<String> = (0..3).map(|_| keys.key(&w)).collect();
        assert_eq!(three, ["lecun2015deep", "lecun2015deepa", "lecun2015deepb"]);
    }

    #[test]
    fn keys_drop_accents_and_stop_words() {
        let mut w = Work::default();
        w.authors = vec![Person::new("Kurt", "Gödel")];
        w.title = Some("On the theory".into());
        assert_eq!(cite_key(&w), "godeltheory");
        assert_eq!(cite_key(&Work::default()), "work");
    }
}
