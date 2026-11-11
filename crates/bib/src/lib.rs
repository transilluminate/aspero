//! Bibliography files: their entries read as citation claims, and found
//! works written back out.
//!
//! - BibTeX/BibLaTeX ([`parse`], via the `biblatex` crate from the Typst
//!   project), RIS ([`parse_ris`]) and CSL-JSON ([`parse_csl_json`]);
//!   [`read`] tells them apart. Each entry becomes a structured [`Claim`]:
//!   title, authors, year, journal, volume, first page, and every
//!   identifier the entry states (DOI, arXiv, ISBN, PMID, and those in its
//!   URLs). Problems with the entry itself, such as a `doi` field that
//!   isn't a DOI, are reported rather than dropped.
//! - [`bibtex`] and [`ris`] write a work as an entry.

#[cfg(feature = "styles")]
pub mod cite;
mod csl;
#[cfg(feature = "docx")]
pub mod docx;
mod ris;
mod write;

use std::fmt;

use aspero_core::matching::find_ids;
use aspero_core::{ArxivId, Claim, Doi, WorkId};
use biblatex::{Bibliography, ChunksExt, Person};

pub use csl::parse_csl_json;
pub use ris::parse_ris;
pub use write::{Keys, bibtex, cite_key, ris};

/// A bibliography file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Format {
    Bibtex,
    Ris,
    CslJson,
}

impl Format {
    pub fn as_str(self) -> &'static str {
        match self {
            Format::Bibtex => "bibtex",
            Format::Ris => "ris",
            Format::CslJson => "csl_json",
        }
    }
}

impl fmt::Display for Format {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Format::Bibtex => "BibTeX",
            Format::Ris => "RIS",
            Format::CslJson => "CSL-JSON",
        })
    }
}

/// One bibliography entry and the claim it makes.
#[derive(Debug, Clone, PartialEq)]
pub struct BibEntry {
    /// The entry's key; for an RIS record without an `ID` or a CSL-JSON
    /// item without an `id`, its 1-based position in the file.
    pub key: String,
    /// Lowercase entry type in the file's own vocabulary: `article`
    /// (BibTeX), `jour` (RIS), `article-journal` (CSL-JSON).
    pub entry_type: String,
    pub claim: Claim,
    /// Problems with the entry's own fields.
    pub notes: Vec<Note>,
}

impl BibEntry {
    /// Whether there is anything to check: a title or an identifier.
    pub fn is_checkable(&self) -> bool {
        self.claim.title.is_some() || !self.claim.ids.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReadError {
    #[error("not BibTeX, RIS or CSL-JSON")]
    Unrecognised,
    #[error("invalid {format} at line {line}: {message}")]
    Invalid {
        format: Format,
        line: usize,
        message: String,
    },
}

/// Which format `src` is in, judged by how it starts: `[` or `{`
/// (CSL-JSON), an RIS `TY` tag, or a BibTeX `@type{`.
pub fn detect(src: &str) -> Option<Format> {
    let src = src.trim_start_matches('\u{FEFF}').trim_start();
    if src.starts_with(['[', '{']) {
        return Some(Format::CslJson);
    }
    if src
        .lines()
        .next()
        .is_some_and(|l| ris::tag(l).is_some_and(|(t, _)| t == "TY"))
    {
        return Some(Format::Ris);
    }
    // BibTeX allows comments and other text between entries.
    let entry = src.match_indices('@').any(|(i, _)| {
        let rest = &src[i + 1..];
        let name = rest.len()
            - rest
                .trim_start_matches(|c: char| c.is_ascii_alphabetic())
                .len();
        name > 0 && rest[name..].trim_start().starts_with(['{', '('])
    });
    entry.then_some(Format::Bibtex)
}

/// Reads a bibliography file in whichever format it is.
pub fn read(src: &str) -> Result<(Format, Vec<BibEntry>), ReadError> {
    let format = detect(src).ok_or(ReadError::Unrecognised)?;
    let entries = match format {
        Format::Bibtex => parse(src),
        Format::Ris => parse_ris(src),
        Format::CslJson => parse_csl_json(src),
    }?;
    Ok((format, entries))
}

/// Parses a `.bib` file's contents into entries, in file order.
pub fn parse(src: &str) -> Result<Vec<BibEntry>, ReadError> {
    let bib = Bibliography::parse(src).map_err(|e| ReadError::Invalid {
        format: Format::Bibtex,
        line: line_at(src, e.span.start),
        message: e.kind.to_string(),
    })?;
    Ok(bib.iter().map(entry).collect())
}

/// The 1-based line of byte `at`.
fn line_at(src: &str, at: usize) -> usize {
    src[..at.min(src.len())].lines().count().max(1)
}

fn entry(e: &biblatex::Entry) -> BibEntry {
    let field = |name: &str| {
        e.get(name)
            .map(|chunks| chunks.format_verbatim())
            .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|s| !s.is_empty())
    };
    let mut ids = Ids::default();
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
    claim.container = field("journal")
        .or_else(|| field("journaltitle"))
        .or_else(|| field("booktitle"));
    claim.volume = field("volume");
    claim.page = field("pages");

    if let Some(doi) = field("doi") {
        ids.doi("doi", &doi);
    }
    let eprint_type = field("eprinttype").or_else(|| field("archiveprefix"));
    match (field("eprint"), eprint_type.as_deref()) {
        (Some(eprint), None) => ids.arxiv("eprint", &eprint, false),
        (Some(eprint), Some(t)) if t.eq_ignore_ascii_case("arxiv") => {
            ids.arxiv("eprint", &eprint, true)
        }
        (Some(eprint), Some(t)) if t.eq_ignore_ascii_case("pubmed") => ids.pmid("eprint", &eprint),
        _ => {}
    }
    if let Some(pmid) = field("pmid") {
        ids.pmid("pmid", &pmid);
    }
    if let Some(isbns) = field("isbn") {
        ids.isbns("isbn", &isbns, true);
    }
    if let Some(url) = field("url") {
        ids.urls(&url);
    }
    claim.ids = ids.ids;

    BibEntry {
        key: e.key.clone(),
        entry_type: e.entry_type.to_string().to_lowercase(),
        claim,
        notes: ids.notes,
    }
}

/// A problem with one of an entry's own fields.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Note {
    /// The field as the file names it: `doi`, `eprint`, `DO`, `ISBN`.
    pub field: String,
    pub value: String,
    pub problem: Problem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Problem {
    NotADoi,
    NotAnArxivId,
    NotAnIsbn,
    NotAPmid,
}

impl Problem {
    pub fn as_str(self) -> &'static str {
        match self {
            Problem::NotADoi => "not_a_doi",
            Problem::NotAnArxivId => "not_an_arxiv_id",
            Problem::NotAnIsbn => "not_an_isbn",
            Problem::NotAPmid => "not_a_pmid",
        }
    }
}

impl fmt::Display for Note {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = match self.problem {
            Problem::NotADoi => "DOI",
            Problem::NotAnArxivId => "arXiv ID",
            Problem::NotAnIsbn => "ISBN",
            Problem::NotAPmid => "PMID",
        };
        write!(
            f,
            "{} field {:?} is not a valid {what}",
            self.field, self.value
        )
    }
}

/// Identifiers gathered from an entry's fields, and problems with them.
#[derive(Default)]
struct Ids {
    ids: Vec<WorkId>,
    notes: Vec<Note>,
}

impl Ids {
    fn push(&mut self, id: WorkId) {
        if !self.ids.contains(&id) {
            self.ids.push(id);
        }
    }

    fn note(&mut self, field: &str, value: &str, problem: Problem) {
        self.notes.push(Note {
            field: field.into(),
            value: value.into(),
            problem,
        });
    }

    fn doi(&mut self, field: &str, raw: &str) {
        match raw.parse::<Doi>() {
            Ok(d) => self.push(WorkId::Doi(d)),
            Err(_) => self.note(field, raw, Problem::NotADoi),
        }
    }

    /// An arXiv ID; `stated` when the entry says it is one, so a bad one
    /// is worth noting.
    fn arxiv(&mut self, field: &str, raw: &str, stated: bool) {
        match raw.parse::<ArxivId>() {
            Ok(a) => self.push(WorkId::Arxiv(a)),
            Err(_) if stated => self.note(field, raw, Problem::NotAnArxivId),
            Err(_) => {}
        }
    }

    fn pmid(&mut self, field: &str, raw: &str) {
        match format!("pmid:{}", raw.trim()).parse::<WorkId>() {
            Ok(id) => self.push(id),
            Err(_) => self.note(field, raw, Problem::NotAPmid),
        }
    }

    /// ISBNs, separated by commas or semicolons. `noted`: whether a bad
    /// one is worth a note (RIS uses the same field for ISSNs).
    fn isbns(&mut self, field: &str, raw: &str, noted: bool) {
        for raw in raw
            .split([',', ';'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            match raw.parse::<aspero_core::Isbn>() {
                Ok(isbn) => self.push(WorkId::Isbn(isbn)),
                Err(_) if noted => self.note(field, raw, Problem::NotAnIsbn),
                Err(_) => {}
            }
        }
    }

    fn urls(&mut self, text: &str) {
        for id in find_ids(text) {
            self.push(id);
        }
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

@book{goodfellow2016,
  author    = {Goodfellow, Ian and Bengio, Yoshua and Courville, Aaron},
  title     = {Deep Learning}, publisher = {MIT Press}, year = {2016},
  isbn      = {978-0-262-03561-3, 0262035619}
}

@misc{website,
  howpublished = {\url{https://example.org}},
}
"#;

    #[test]
    fn entries_become_claims() {
        let entries = parse(BIB).unwrap();
        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "lecun2015",
                "vaswani2017",
                "broken",
                "goodfellow2016",
                "website"
            ]
        );
        let book = &entries[3];
        assert_eq!(book.claim.ids[0].to_string(), "isbn:9780262035613");
        assert_eq!(
            book.notes
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            ["isbn field \"0262035619\" is not a valid ISBN"],
            "bad checksum noted"
        );

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
        let note = &broken.notes[0];
        assert_eq!(
            (note.field.as_str(), note.value.as_str()),
            ("doi", "not-a-doi")
        );
        assert_eq!(note.problem, Problem::NotADoi);
        assert_eq!(
            note.to_string(),
            "doi field \"not-a-doi\" is not a valid DOI"
        );
        assert_eq!(broken.claim.ids[0].to_string(), "arxiv:1412.6980v9");
        assert!(!entries[4].is_checkable());
    }

    #[test]
    fn syntax_errors_name_the_line() {
        let err =
            parse("@article{ok, title={Fine}}\n\n@article{bad, title = {unclosed}\n").unwrap_err();
        assert!(
            matches!(
                err,
                ReadError::Invalid {
                    format: Format::Bibtex,
                    line: 3..,
                    ..
                }
            ),
            "{err}"
        );
    }

    #[test]
    fn formats_are_told_apart() {
        assert_eq!(detect(BIB), Some(Format::Bibtex));
        assert_eq!(
            detect("% refs\n@Article (x, title = {T})"),
            Some(Format::Bibtex)
        );
        assert_eq!(detect("\u{FEFF}TY  - JOUR\nER  - \n"), Some(Format::Ris));
        assert_eq!(detect("  [{\"title\": \"T\"}]"), Some(Format::CslJson));
        assert_eq!(detect("Smith J. A study. 2020. me@example.org"), None);
        assert_eq!(read("1. Smith J."), Err(ReadError::Unrecognised));
        let (format, entries) = read(BIB).unwrap();
        assert_eq!((format, entries.len()), (Format::Bibtex, 5));
    }

    #[test]
    fn bibtex_entries_carry_journal_volume_and_pages() {
        let entries = parse(
            "@article{x, title={T}, journaltitle={Nature}, volume={521}, pages={436--444},
              eprinttype={pubmed}, eprint={26017442}}",
        )
        .unwrap();
        let claim = &entries[0].claim;
        assert_eq!(claim.container.as_deref(), Some("Nature"));
        assert_eq!(claim.volume.as_deref(), Some("521"));
        assert_eq!(claim.page.as_deref(), Some("436–444"), "-- read as a dash");
        assert_eq!(claim.ids[0].to_string(), "pmid:26017442");
    }
}
