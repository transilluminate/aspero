//! BibTeX/BibLaTeX reading: bibliography entries as citation claims.
//!
//! Parsing uses the `biblatex` crate (from the Typst project). Each entry
//! becomes a structured [`Claim`]: title, authors, year, and every
//! identifier the entry states (`doi`, arXiv `eprint`, DOI or arXiv URLs).
//! Problems with the entry itself, such as a `doi` field that isn't a DOI,
//! are reported rather than dropped.

use aspero_core::matching::find_ids;
use aspero_core::{ArxivId, Claim, Doi, WorkId};
use biblatex::{Bibliography, ChunksExt, Person};

/// One bibliography entry and the claim it makes.
#[derive(Debug, Clone, PartialEq)]
pub struct BibEntry {
    pub key: String,
    /// Lowercase entry type, e.g. `article`, `inproceedings`, `misc`.
    pub entry_type: String,
    pub claim: Claim,
    /// Problems with the entry's own fields.
    pub notes: Vec<String>,
}

impl BibEntry {
    /// Whether there is anything to check: a title or an identifier.
    pub fn is_checkable(&self) -> bool {
        self.claim.title.is_some() || !self.claim.ids.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid BibTeX at line {line}: {message}")]
pub struct BibError {
    pub line: usize,
    pub message: String,
}

/// Parses a `.bib` file's contents into entries, in file order.
pub fn parse(src: &str) -> Result<Vec<BibEntry>, BibError> {
    let bib = Bibliography::parse(src).map_err(|e| BibError {
        line: src[..e.span.start.min(src.len())].lines().count().max(1),
        message: e.kind.to_string(),
    })?;
    Ok(bib.iter().map(entry).collect())
}

fn entry(e: &biblatex::Entry) -> BibEntry {
    let field = |name: &str| {
        e.get(name)
            .map(|chunks| chunks.format_verbatim())
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
    };
    let mut notes = Vec::new();
    let mut claim = Claim::default();
    claim.title = field("title");
    claim.authors = e
        .get_as::<Vec<Person>>("author")
        .unwrap_or_default()
        .iter()
        .filter(|p| p.name != "others")
        .map(|p| match p.given_name.as_str() {
            "" => p.name.clone(),
            given => format!("{}, {given}", p.name),
        })
        .collect();
    claim.year = field("year")
        .or_else(|| field("date"))
        .and_then(|y| first_year(&y));

    let mut ids: Vec<WorkId> = Vec::new();
    if let Some(doi) = field("doi") {
        match doi.parse::<Doi>() {
            Ok(d) => push(&mut ids, WorkId::Doi(d)),
            Err(_) => notes.push(format!("doi field {doi:?} is not a valid DOI")),
        }
    }
    let eprint_type = field("eprinttype").or_else(|| field("archiveprefix"));
    if let Some(eprint) = field("eprint")
        && eprint_type
            .as_deref()
            .is_none_or(|t| t.eq_ignore_ascii_case("arxiv"))
    {
        match eprint.parse::<ArxivId>() {
            Ok(a) => push(&mut ids, WorkId::Arxiv(a)),
            Err(_) if eprint_type.is_some() => {
                notes.push(format!("eprint {eprint:?} is not a valid arXiv ID"))
            }
            Err(_) => {}
        }
    }
    if let Some(url) = field("url") {
        for id in find_ids(&url) {
            push(&mut ids, id);
        }
    }
    claim.ids = ids;

    BibEntry {
        key: e.key.clone(),
        entry_type: e.entry_type.to_string().to_lowercase(),
        claim,
        notes,
    }
}

fn push(ids: &mut Vec<WorkId>, id: WorkId) {
    if !ids.contains(&id) {
        ids.push(id);
    }
}

/// The first four-digit year in a `year` or `date` field ("2017",
/// "2017-06-12", "2017/2018").
fn first_year(s: &str) -> Option<i16> {
    s.as_bytes()
        .windows(4)
        .find(|w| w.iter().all(u8::is_ascii_digit))
        .and_then(|w| std::str::from_utf8(w).ok()?.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIB: &str = r#"
@string{nat = "Nature"}

@article{lecun2015,
  author  = {LeCun, Yann and Bengio, Yoshua and Hinton, Geoffrey},
  title   = {Deep {Learning}},
  journal = nat,
  year    = {2015},
  doi     = {https://doi.org/10.1038/nature14539},
}

@misc{vaswani2017,
  author        = {Ashish Vaswani and Noam Shazeer and others},
  title         = {Attention Is All You Need},
  date          = {2017-06-12},
  eprint        = {1706.03762},
  archivePrefix = {arXiv},
}

@inproceedings{broken,
  title = {Something},
  doi   = {not-a-doi},
  url   = {https://arxiv.org/abs/1412.6980v9},
}

@misc{website,
  howpublished = {\url{https://example.org}},
}
"#;

    #[test]
    fn entries_become_claims() {
        let entries = parse(BIB).unwrap();
        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["lecun2015", "vaswani2017", "broken", "website"]);

        let lecun = &entries[0].claim;
        assert_eq!(lecun.title.as_deref(), Some("Deep Learning"));
        assert_eq!(lecun.authors[0], "LeCun, Yann");
        assert_eq!(lecun.year, Some(2015));
        assert_eq!(lecun.ids[0].to_string(), "doi:10.1038/nature14539");
        assert_eq!(entries[0].entry_type, "article");

        let vaswani = &entries[1].claim;
        assert_eq!(
            vaswani.authors,
            ["Vaswani, Ashish", "Shazeer, Noam"],
            "`others` dropped"
        );
        assert_eq!(vaswani.year, Some(2017));
        assert_eq!(vaswani.ids[0].to_string(), "arxiv:1706.03762");
    }

    #[test]
    fn field_problems_are_noted_and_urls_yield_ids() {
        let entries = parse(BIB).unwrap();
        let broken = &entries[2];
        assert_eq!(broken.notes, ["doi field \"not-a-doi\" is not a valid DOI"]);
        assert_eq!(broken.claim.ids[0].to_string(), "arxiv:1412.6980v9");
        assert!(!entries[3].is_checkable());
    }

    #[test]
    fn syntax_errors_name_the_line() {
        let err =
            parse("@article{ok, title={Fine}}\n\n@article{bad, title = {unclosed}\n").unwrap_err();
        assert!(err.line >= 1, "{err}");
    }
}
