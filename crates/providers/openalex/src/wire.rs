//! OpenAlex JSON as received, and its mapping into aspero's model.
//!
//! Wire types are lenient: every field defaults, unknown fields are ignored
//! (the full response is kept in `Raw`), so upstream additions never break
//! decoding.

use std::collections::BTreeMap;

use aspero_core::csl::{null_default, plain_text};
use aspero_core::{
    ArxivId, AuthorId, Doi, FieldSet, Notice, NoticeKind, OpenAccess, OpenAlexId, PartialDate,
    Person, Pmcid, Pmid, Raw, SourceRecord, Work, WorkId, WorkType, orcid, tidy_abstract,
};
use jiff::Timestamp;
use serde::Deserialize;

use crate::ID;

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireWork {
    id: Option<String>,
    doi: Option<String>,
    title: Option<String>,
    display_name: Option<String>,
    publication_year: Option<i16>,
    publication_date: Option<String>,
    #[serde(rename = "type")]
    work_type: Option<String>,
    #[serde(deserialize_with = "null_default")]
    ids: WireIds,
    #[serde(deserialize_with = "null_default")]
    authorships: Vec<WireAuthorship>,
    primary_location: Option<WireLocation>,
    #[serde(deserialize_with = "null_default")]
    locations: Vec<WireLocation>,
    #[serde(deserialize_with = "null_default")]
    biblio: WireBiblio,
    abstract_inverted_index: Option<BTreeMap<String, Vec<usize>>>,
    cited_by_count: Option<u64>,
    /// From Retraction Watch; OpenAlex gives no notice details.
    #[serde(deserialize_with = "null_default")]
    is_retracted: bool,
    /// Unpaywall's data.
    open_access: Option<WireOpenAccess>,
    best_oa_location: Option<WireLocation>,
    /// The works this one cites, as OpenAlex work URLs.
    #[serde(deserialize_with = "null_default")]
    referenced_works: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireOpenAccess {
    #[serde(deserialize_with = "null_default")]
    is_oa: bool,
    oa_status: Option<String>,
    oa_url: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireIds {
    openalex: Option<String>,
    doi: Option<String>,
    pmid: Option<String>,
    pmcid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireAuthorship {
    #[serde(deserialize_with = "null_default")]
    author: WireAuthor,
    raw_author_name: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireAuthor {
    id: Option<String>,
    display_name: Option<String>,
    orcid: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireLocation {
    landing_page_url: Option<String>,
    pdf_url: Option<String>,
    license: Option<String>,
    version: Option<String>,
    source: Option<WireSource>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireSource {
    display_name: Option<String>,
    host_organization_name: Option<String>,
    #[serde(rename = "type")]
    source_type: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WireBiblio {
    volume: Option<String>,
    issue: Option<String>,
    first_page: Option<String>,
    last_page: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireList {
    #[serde(deserialize_with = "null_default")]
    pub meta: WireMeta,
    /// Kept as raw values so each work's `Raw` is its own JSON object.
    #[serde(deserialize_with = "null_default")]
    pub results: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WireMeta {
    pub count: Option<u64>,
    pub next_cursor: Option<String>,
}

/// Maps one OpenAlex work object into a [`SourceRecord`].
pub(crate) fn to_record(
    value: &serde_json::Value,
    raw: Raw,
    fetched_at: Timestamp,
) -> serde_json::Result<SourceRecord> {
    let w = WireWork::deserialize(value)?;
    Ok(
        SourceRecord::new(ID, w.ids(), w.work(), FieldSet::ALL, raw, fetched_at)
            .with_references(w.references()),
    )
}

impl WireWork {
    /// Every identifier OpenAlex asserts, OpenAlex ID first. Values that don't
    /// parse are skipped here; they remain in the raw record.
    fn ids(&self) -> Vec<WorkId> {
        let mut ids: Vec<WorkId> = Vec::new();
        let mut push = |id: WorkId| {
            if !ids.contains(&id) {
                ids.push(id);
            }
        };

        let openalex = self.ids.openalex.as_ref().or(self.id.as_ref());
        if let Some(id) = openalex.and_then(|s| s.parse::<OpenAlexId>().ok()) {
            push(id.into());
        }
        let doi = self.ids.doi.as_ref().or(self.doi.as_ref());
        if let Some(doi) = doi.and_then(|s| s.parse::<Doi>().ok()) {
            if let Some(arxiv) = doi.arxiv() {
                push(arxiv.into());
            }
            push(doi.into());
        }
        if let Some(id) = self.ids.pmid.as_ref().and_then(|s| s.parse::<Pmid>().ok()) {
            push(id.into());
        }
        if let Some(id) = self
            .ids
            .pmcid
            .as_ref()
            .and_then(|s| s.parse::<Pmcid>().ok())
        {
            push(id.into());
        }
        // arXiv versions of published works appear as arxiv.org locations.
        for url in self
            .locations
            .iter()
            .filter_map(|l| l.landing_page_url.as_ref())
        {
            if url.contains("arxiv.org/")
                && let Ok(id) = url.parse::<ArxivId>()
            {
                push(id.unversioned().into());
            }
        }
        ids
    }

    /// The works cited, or `None` when OpenAlex has none listed. OpenAlex
    /// gives the same empty `referenced_works` (and a
    /// `referenced_works_count` of 0) for works it has no reference data for
    /// (books, and works whose publisher deposited no references) as for
    /// works citing nothing, and no other field tells them apart (seen
    /// 2026-11: the DSM-5's record, indexed from Crossref, has none). Works
    /// citing nothing are rare, so an empty list reads as unknown.
    fn references(&self) -> Option<Vec<WorkId>> {
        let cited: Vec<WorkId> = self
            .referenced_works
            .iter()
            .filter_map(|url| url.parse::<OpenAlexId>().ok())
            .map(WorkId::from)
            .collect();
        (!cited.is_empty()).then_some(cited)
    }

    /// Where the work appeared: its primary location's source, unless
    /// that is a repository holding a copy of an article ("PubMed Central",
    /// "arXiv (Cornell University)"), which appeared in the journal another
    /// location names, if any.
    fn source(&self) -> Option<&WireSource> {
        let primary = self
            .primary_location
            .as_ref()
            .and_then(|l| l.source.as_ref());
        let is = |s: &WireSource, kind: &str| s.source_type.as_deref() == Some(kind);
        match primary {
            Some(s) if is(s, "repository") && self.work_type.as_deref().is_some_and(is_article) => {
                self.locations
                    .iter()
                    .filter_map(|l| l.source.as_ref())
                    .find(|s| is(s, "journal"))
            }
            _ => primary,
        }
    }

    fn work(&self) -> Work {
        let source = self.source();
        let mut work = Work::default();
        work.title = self
            .title
            .clone()
            .or_else(|| self.display_name.clone())
            .map(plain_text);
        work.authors = self
            .authorships
            .iter()
            .filter_map(WireAuthorship::person)
            .collect();
        work.issued = self
            .publication_date
            .as_deref()
            .and_then(|d| d.parse().ok())
            .or(self.publication_year.map(PartialDate::from_year));
        work.work_type = self
            .work_type
            .as_deref()
            .map(|t| work_type(t, source.and_then(|s| s.source_type.as_deref())));
        work.container_title = source.and_then(|s| s.display_name.clone()).map(plain_text);
        work.publisher = source.and_then(|s| s.host_organization_name.clone());
        work.volume.clone_from(&self.biblio.volume);
        work.issue.clone_from(&self.biblio.issue);
        work.page = match (&self.biblio.first_page, &self.biblio.last_page) {
            (Some(first), Some(last)) if first != last => Some(format!("{first}-{last}")),
            (Some(first), _) => Some(first.clone()),
            (None, last) => last.clone(),
        };
        work.abstract_text = self
            .abstract_inverted_index
            .as_ref()
            .map(|index| tidy_abstract(&rebuild_abstract(index)))
            .and_then(without_footer)
            .filter(|text| !scraped_page(text));
        work.url = self
            .primary_location
            .as_ref()
            .and_then(|l| l.landing_page_url.clone());
        work.cited_by_count = self.cited_by_count;
        work.open_access = self.open_access.as_ref().map(|oa| {
            let best = self.best_oa_location.as_ref().filter(|_| oa.is_oa);
            let mut out = OpenAccess::default();
            out.is_oa = oa.is_oa;
            out.status.clone_from(&oa.oa_status);
            out.pdf_url = best.and_then(|l| l.pdf_url.clone());
            out.url = best
                .and_then(|l| l.landing_page_url.clone())
                .or_else(|| oa.oa_url.clone().filter(|_| oa.is_oa));
            out.license = best.and_then(|l| l.license.clone());
            out.version = best.and_then(|l| l.version.clone());
            out.source = Some("openalex".into());
            out
        });
        if self.is_retracted {
            let mut retraction = Notice::new(NoticeKind::Retraction);
            retraction.source = Some("openalex".into());
            work.notices.push(retraction);
        }
        work
    }
}

impl WireAuthorship {
    /// The author's display name, or the name as printed (`raw_author_name`)
    /// when there is none or the display name is cut short of it: OpenAlex
    /// shows the Société française d'anesthésie et de réanimation as
    /// "Société française d'anesthésie et" (W894996489, seen 2026-11-14).
    /// An "et al." standing for the rest isn't an author.
    fn person(&self) -> Option<Person> {
        let raw = self.raw_author_name.as_deref().map(str::trim);
        let name = match (self.author.display_name.as_deref().map(str::trim), raw) {
            (Some(shown), Some(raw)) if cut_short(shown, raw) => raw,
            (Some(shown), _) if !shown.is_empty() => shown,
            (_, raw) => raw?,
        };
        if name.is_empty() || is_et_al(name) {
            return None;
        }
        let mut person = Person::literal(name.to_owned());
        person.orcid = self.author.orcid.as_deref().and_then(orcid);
        person.openalex_id = match self.author.id.as_deref().and_then(AuthorId::parse) {
            Some(AuthorId::OpenAlex(id)) => Some(id),
            _ => None,
        };
        Some(person)
    }
}

/// Whether `shown`, an organisation's name (three words or more, not a
/// person's), is `raw` cut short: a shorter start of it, comparing letters
/// and digits only (quotes and spacing differ). People's names are left
/// alone: "Smith" starts "Smith, John" too.
fn cut_short(shown: &str, raw: &str) -> bool {
    let key = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let organisation =
        shown.split_whitespace().count() >= 3 && Person::literal(shown).parts().is_none();
    let (shown, raw) = (key(shown), key(raw));
    organisation && raw.len() > shown.len() && raw.starts_with(&shown)
}

/// "et al.", "et al", "et. al.": a list's tail, not a name.
fn is_et_al(name: &str) -> bool {
    let words: String = name
        .chars()
        .filter(|c| c.is_alphabetic())
        .flat_map(char::to_lowercase)
        .collect();
    words == "etal"
}

/// OpenAlex types to CSL. Conference papers have their own type
/// (`conference-paper`, seen 2026-11), but older records may still be an
/// `article` in a conference source. OpenAlex's `review` means a review
/// *article*, unlike CSL's `review` (a review of another work). An
/// `article` in a book series or an ebook is a chapter of a book.
fn work_type(openalex: &str, source_type: Option<&str>) -> WorkType {
    match (openalex, source_type) {
        ("conference-paper", _) | ("article", Some("conference")) => WorkType::ConferencePaper,
        ("article", Some("book series" | "ebook platform")) => WorkType::Chapter,
        (t, _) if is_article(t) => WorkType::JournalArticle,
        ("preprint", _) => WorkType::Preprint,
        ("book", _) => WorkType::Book,
        ("book-chapter", _) => WorkType::Chapter,
        ("dissertation", _) => WorkType::Thesis,
        ("report", _) => WorkType::Report,
        ("dataset", _) => WorkType::Dataset,
        (other, _) => WorkType::from_csl(other),
    }
}

/// OpenAlex types of journal articles.
fn is_article(openalex: &str) -> bool {
    matches!(openalex, "article" | "review" | "letter" | "editorial")
}

/// Whether an "abstract" is a publisher's landing page read as one, its
/// furniture and all (OpenAlex has some): Wiley's and the ACP journals'
/// "Search for more papers by this author" beside each author, Wiley's
/// header (the journal's name run into "Volume 154, Issue S1 p. 3-31"), a journal site's "Back to table of contents".
/// None of these is in an abstract; the work is better without one.
fn scraped_page(text: &str) -> bool {
    const FURNITURE: &[&str] = &[
        "Search for more papers by this author",
        "Back to table of contents Previous article Next article",
    ];
    if FURNITURE.iter().any(|f| text.contains(f)) {
        return true;
    }
    // A section's name run into its date and the title ("Letters16 March
    // 2020COVID-19 and the Risk …", "Quality ImprovementSeptember 1,
    // 2004Review: …"): the ACP journals' pages.
    if glued_date(text) {
        return true;
    }
    // "<journal>Volume 154, Issue S1 p. 3-31": the journal's name run into
    // its issue's, near the start.
    let head: String = text.chars().take(200).collect();
    head.find("Volume ").is_some_and(|at| {
        let glued = head[..at].ends_with(|c: char| c.is_alphabetic());
        let rest = &head[at + "Volume ".len()..];
        let number = rest
            .split(',')
            .next()
            .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        glued && number && rest.contains(", Issue ") && rest.contains(" p. ")
    })
}

/// Shortest text before a page's footer that is still an abstract.
const ABSTRACT_BEFORE_FOOTER: usize = 100;

/// An abstract without the page footer OpenAlex ran into it ("… the
/// long-range order in the lattice.Received 18 September 1952DOI:https://
/// doi.org/10.1103/…©1952 American Physical Society"), `None` when too
/// little is left to be one ("A Reply to the Comment by Robert Alicki.").
fn without_footer(text: String) -> Option<String> {
    let Some(doi) = text
        .find("DOI:https://doi.org/")
        .filter(|_| text.contains('©'))
    else {
        return Some(text);
    };
    let start = text[..doi]
        .rfind("Received ")
        .filter(|at| doi - at < 30)
        .unwrap_or(doi);
    let kept = text[..start].trim();
    (kept.chars().count() >= ABSTRACT_BEFORE_FOOTER).then(|| kept.to_string())
}

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// Whether, in its first 60 characters, `text` has a date ("16 March 2020",
/// "September 1, 2004") with a letter run into it before and a capital
/// after: a page's heading, not a sentence.
fn glued_date(text: &str) -> bool {
    let head: String = text.chars().take(60).collect();
    let year_then_capital = |rest: &str| {
        let digits = rest.chars().take_while(char::is_ascii_digit).count();
        digits == 4 && rest[4..].starts_with(|c: char| c.is_uppercase())
    };
    MONTHS.iter().any(|month| {
        head.match_indices(month).any(|(at, _)| {
            let before = &head[..at];
            let after = &head[at + month.len()..];
            // "…Letters16 March 2020C…"
            let day_first = before
                .strip_suffix(' ')
                .map(|b| b.trim_end_matches(|c: char| c.is_ascii_digit()))
                .is_some_and(|b| {
                    b.len() + 1 < before.len()
                        && b.len() + 3 >= before.len()
                        && b.ends_with(|c: char| c.is_alphabetic())
                })
                && after.strip_prefix(' ').is_some_and(year_then_capital);
            // "…ImprovementSeptember 1, 2004R…"
            let month_first = before.ends_with(|c: char| c.is_lowercase())
                && after.strip_prefix(' ').is_some_and(|a| {
                    let day = a.chars().take_while(char::is_ascii_digit).count();
                    (1..=2).contains(&day)
                        && a[day..].strip_prefix(", ").is_some_and(year_then_capital)
                });
            day_first || month_first
        })
    })
}

/// OpenAlex ships abstracts as `{word: [positions]}`; put the words back in order.
fn rebuild_abstract(index: &BTreeMap<String, Vec<usize>>) -> String {
    let mut positioned: Vec<(usize, &str)> = index
        .iter()
        .flat_map(|(word, positions)| positions.iter().map(move |&p| (p, word.as_str())))
        .collect();
    positioned.sort_unstable();
    positioned
        .into_iter()
        .map(|(_, word)| word)
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authorship(display: Option<&str>, raw: Option<&str>) -> Option<Person> {
        let wire = WireAuthorship {
            author: WireAuthor {
                id: None,
                display_name: display.map(str::to_owned),
                orcid: None,
            },
            raw_author_name: raw.map(str::to_owned),
        };
        wire.person()
    }

    fn name(p: Option<Person>) -> Option<String> {
        p.and_then(|p| p.display_name())
    }

    /// As OpenAlex gives W894996489 (2015), whose first author's display
    /// name stops at "et": the name as printed is used.
    #[test]
    fn a_name_cut_short_is_read_as_printed_and_et_al_is_no_author() {
        assert_eq!(
            name(authorship(
                Some("Société française d'anesthésie et"),
                Some("Société française d’anesthésie et de réanimation"),
            ))
            .as_deref(),
            Some("Société française d’anesthésie et de réanimation")
        );
        // People's display names stand.
        for (display, raw) in [
            ("Jason Edward Smith", "J. E. Smith"),
            ("Smith", "Smith, John"),
            ("S. C. S", "S. C. S"),
        ] {
            assert_eq!(
                name(authorship(Some(display), Some(raw))).as_deref(),
                Some(display)
            );
        }
        assert_eq!(
            name(authorship(None, Some("M. Rockett"))).as_deref(),
            Some("M. Rockett")
        );
        for tail in ["et al.", "et al", "Et. Al.", " et al "] {
            assert!(authorship(Some(tail), None).is_none(), "{tail}");
        }
        assert!(authorship(Some(" "), None).is_none());
    }

    /// An article whose primary location is a repository holding a copy.
    fn in_repository(locations: &[(&str, &str)]) -> Work {
        let location = |(kind, name): &(&str, &str)| serde_json::json!({ "source": { "display_name": name, "type": kind } });
        let wire: WireWork = serde_json::from_value(serde_json::json!({
            "type": "article",
            "primary_location": location(&locations[0]),
            "locations": locations.iter().map(location).collect::<Vec<_>>(),
        }))
        .unwrap();
        wire.work()
    }

    #[test]
    fn a_repository_holding_a_copy_is_not_the_journal() {
        let w = in_repository(&[
            ("repository", "PubMed Central"),
            ("journal", "New England Journal of Medicine"),
        ]);
        assert_eq!(w.work_type, Some(WorkType::JournalArticle));
        assert_eq!(
            w.container_title.as_deref(),
            Some("New England Journal of Medicine")
        );
        let w = in_repository(&[("repository", "arXiv (Cornell University)")]);
        assert_eq!(w.work_type, Some(WorkType::JournalArticle));
        assert_eq!(w.container_title, None);
    }

    #[test]
    fn a_landing_page_read_as_an_abstract_is_none() {
        let wiley = "International Journal of Gynecology & ObstetricsVolume 154, Issue S1 \
            p. 3-31 SUPPLEMENT ARTICLEOpen Access A literature review and best practice \
            advice … Liona C. Poon, Corresponding Author Liona C. Poon";
        assert!(scraped_page(wiley));
        let acp = "Letters11 November 2020Sixty-Day Outcomes Among Patients Hospitalized \
            With COVID-19FREEVineet Chopra, MD, MScSearch for more papers by this author";
        assert!(scraped_page(acp));
        assert!(scraped_page(
            "Quality ImprovementSeptember 1, 2004Review: Individualized written action plans"
        ));
        assert_eq!(
            without_footer(
                "Received 6 December 2005DOI:https://doi.org/10.1103/PhysRevLett.96.039901\
                 ©2006 American Physical Society"
                    .into()
            ),
            None
        );
        let ising = "The spontaneous magnetization of a two-dimensional Ising model is \
            calculated exactly. The result also gives the long-range order in the lattice.";
        assert_eq!(
            without_footer(format!(
                "{ising}Received 18 September 1952DOI:https://doi.org/10.1103/PhysRev.85.808\
                 ©1952 American Physical Society"
            ))
            .as_deref(),
            Some(ising)
        );
        for real in [
            "On 16 March 2020 the WHO declared a pandemic. We studied 400 patients.",
            "Between September 1, 2004 and May 2005, we enrolled 120 adults.",
            "BACKGROUND: Coronary heart disease is the leading cause of mortality worldwide.",
            "We report outcomes (Volume 3, Issue 2 p. 4 of the registry) for 2,000 patients.",
            "Journal Article Future Directions in the Sociology of Religion Get access \
             Social Forces, Volume 86, Issue 4, June 2008",
        ] {
            assert!(!scraped_page(real), "{real}");
        }
    }

    #[test]
    fn rebuilds_inverted_abstracts() {
        let index = BTreeMap::from([
            ("is".to_owned(), vec![1]),
            ("Attention".to_owned(), vec![0]),
            ("all".to_owned(), vec![2]),
            ("you".to_owned(), vec![3, 5]),
            ("need".to_owned(), vec![4]),
        ]);
        assert_eq!(rebuild_abstract(&index), "Attention is all you need you");
    }

    #[test]
    fn maps_types_with_source_context() {
        assert_eq!(
            work_type("article", Some("conference")),
            WorkType::ConferencePaper
        );
        assert_eq!(
            work_type("conference-paper", Some("conference")),
            WorkType::ConferencePaper
        );
        assert_eq!(
            work_type("article", Some("journal")),
            WorkType::JournalArticle
        );
        assert_eq!(work_type("review", None), WorkType::JournalArticle);
        // An article in a book series or an ebook is a chapter of a book.
        for source in ["book series", "ebook platform"] {
            assert_eq!(work_type("article", Some(source)), WorkType::Chapter);
        }
        assert_eq!(
            work_type("paratext", None),
            WorkType::Other("paratext".into())
        );
    }
}
