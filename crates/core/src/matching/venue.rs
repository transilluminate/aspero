//! The venue cited against a record's: a chapter cited as a journal's
//! article or the reverse, another conference or meeting, and a preprint or
//! unplaced copy cited in a meeting's proceedings.

use std::ops::Range;

use super::claim::{cited_venue, cites_chapter, cites_edited_book};
use super::preprint::preprint_server;
use super::text::Normalised;
use super::{Agreement, Claim, FieldCheck};
use crate::id::IdKind;
use crate::publication::{self, Locator};
use crate::record::SourceRecord;
use crate::work::WorkType;

/// What [`venue_checks`] reads: the claim, the record, and what
/// [`super::assess()`] has made of them so far.
pub(super) struct VenueInputs<'a> {
    pub(super) claim: &'a Claim,
    pub(super) record: &'a SourceRecord,
    /// The claim's free text, read.
    pub(super) text: Option<&'a Normalised>,
    /// Where the record's title stands in `text`.
    pub(super) title_span: Option<&'a Range<usize>>,
    /// The record's title, unless a URL.
    pub(super) work_title: Option<&'a str>,
    /// The volume and page cited.
    pub(super) cited_place: &'a Locator,
    /// The record is a journal's article.
    pub(super) article: bool,
    /// The record is a preprint.
    pub(super) preprint: bool,
    /// A claimed identifier names the work.
    pub(super) named: bool,
    /// The claim cites a journal.
    pub(super) cites_journal: bool,
    /// The container as compared so far.
    pub(super) container: FieldCheck,
}

/// What [`venue_checks`] decides, for the rest of [`super::assess()`].
pub(super) struct VenueChecks {
    /// The container, a mismatch where the venue cited is another.
    pub(super) container: FieldCheck,
    pub(super) chapter_as_article: bool,
    pub(super) article_as_chapter: bool,
    pub(super) other_conference: bool,
    pub(super) preprint_at_conference: bool,
}

/// Whether a container is named as a book series or a serial, whose
/// volumes are real ("Lecture Notes in Computer Science", "Advances in
/// Experimental Social Psychology", "Methods in Enzymology", "Geophysical
/// Monograph Series"), not one book. Generous: it only keeps a series'
/// chapter, cited at its volume, from reading as an article.
fn series_name(container: &str) -> bool {
    const LEADING: &[&str] = &[
        "advances in",
        "progress in",
        "methods in",
        "current topics in",
        "topics in",
        "developments in",
        "contributions to",
        "frontiers in",
        "frontiers of",
        "studies in",
        "annual review",
        "annals of",
        "handbook of experimental",
        "international review of",
        "recent results in",
        "results and problems in",
    ];
    const ANYWHERE: &[&str] = &[
        "lecture notes",
        "series",
        "monograph",
        "monographs",
        "yearbook",
        "proceedings",
        "symposium",
        "symposia",
        "reviews",
        "library",
        "studies",
    ];
    let words = format!(" {} ", super::text::normalise(container));
    let lead = words.trim_start_matches(" the ").trim_start();
    LEADING.iter().any(|s| lead.starts_with(&format!("{s} ")))
        || ANYWHERE.iter().any(|s| words.contains(&format!(" {s} ")))
}

/// Holds the venue cited against the record's, in order, each check
/// reading the container the ones before left.
#[allow(clippy::too_many_lines)]
pub(super) fn venue_checks(input: &VenueInputs<'_>) -> VenueChecks {
    let VenueInputs {
        claim,
        record,
        text,
        title_span,
        work_title,
        cited_place,
        article,
        preprint,
        named,
        cites_journal,
        container,
    } = *input;
    let work = &record.work;
    // A book's chapter, in no volume, cited as a journal's article: a
    // volume with an issue ("Biogeochemical Cycles and Climate.
    // 2019;23(5):159-175"), or a journal and volume as fields. The
    // chapter's book, given as the journal, is the place cited, but not
    // in that shape: plausible; another journal is another venue too.
    // (A book series' volume, "Lecture Notes in Computer Science, vol.
    // 9351", has no issue; a chapter cited as one, "In: …", is none.)
    // So is a conference paper in a series ("Lecture Notes in Computer
    // Science"), no more a journal's.
    // So is the book named as the journal, its volume given as a
    // journal's, without an issue ("Nagatomo J. (2014). Life after
    // Migration. Migration as Transnational Leisure, 33, 128–178."; "Crop
    // Physiology. 2015;35:1-14."): a book has no volume of that shape. Not
    // a series' volume ("Advances in Experimental Social Psychology, 35,
    // 53–152"), which its record may lack: a container named as a series
    // is one, and so is a chapter its identifier names.
    let issue_cited = claim.text.as_deref().is_some_and(publication::cites_issue);
    let book_as_journal = container.agreement == Agreement::Match
        && !named
        && claim
            .text
            .as_deref()
            .is_some_and(|t| publication::cites_journal_place(t) && !cites_edited_book(t))
        && !work.container_title.as_deref().is_some_and(series_name);
    let chapter_as_article = work.volume.is_none()
        && cited_place.volume.is_some()
        && !claim.text.as_deref().is_some_and(cites_chapter)
        && match work.work_type {
            Some(WorkType::Chapter) => {
                issue_cited
                    || book_as_journal
                    || (claim.container.is_some() && container.agreement != Agreement::Match)
            }
            Some(WorkType::ConferencePaper) => issue_cited,
            _ => false,
        };
    let container =
        if chapter_as_article && cites_journal && container.agreement != Agreement::Match {
            FieldCheck::exact(false)
        } else {
            container
        };
    // A conference paper cited at another conference ("… In IEEE
    // International Conference on Computer Vision (ICCV)" for CVPR's), or
    // a preprint, at none, cited in a conference's proceedings: the venue
    // cited isn't confirmed, unless the claimed identifier says which
    // version is meant.
    let venue = cited_venue(claim, text, title_span);
    // A journal's article cited as a chapter of an edited book ("In:
    // Corvaro IR, Mirkrand P, editors. Handbook of Nurse-Midwifery"): the
    // reverse, another container (a meeting's proceedings aside, and a
    // series named as its journal: "In: …, editor. Methods in Enzymology").
    // So too when the book named is its journal, but in no volume ("In:
    // …, editor. Journal of Applied Probability. Wiley-Blackwell; 1994.
    // p. 185-197."): a series' volume is cited with its number ("Methods
    // in Enzymology. Vol 182").
    let volume_cited = work.volume.as_deref().is_some_and(|v| {
        cited_place.volume.as_deref() == Some(v)
            || text.is_some_and(|t| t.words.iter().any(|w| w == v))
    });
    let article_as_chapter = article
        && (container.agreement != Agreement::Match || !volume_cited)
        && claim.text.as_deref().is_some_and(cites_edited_book)
        && !venue.as_deref().is_some_and(publication::proceedings);
    let container = if article_as_chapter {
        FieldCheck::exact(false)
    } else {
        container
    };
    let cited_conferences: Vec<usize> = venue.as_deref().map_or_else(Vec::new, |v| {
        let in_title = work_title.map(publication::conferences).unwrap_or_default();
        publication::conferences(v)
            .into_iter()
            .filter(|c| !in_title.contains(c))
            .collect()
    });
    // The conference a record is at: its proceedings' name, else what an
    // IEEE DOI names ("10.1109/cvpr.2017.632"), for a record of the paper
    // merged with its preprint's, without a venue.
    let own_conferences: Vec<usize> = match work.container_title.as_deref() {
        Some(venue) if !preprint_server(venue) => publication::conferences(venue),
        _ => record
            .ids
            .iter()
            .filter(|id| id.kind() == IdKind::Doi)
            .map(ToString::to_string)
            .filter_map(|doi| {
                doi.strip_prefix("doi:10.1109/")
                    .map(|s| publication::conferences(s.split('.').next().unwrap_or("")))
            })
            .flatten()
            .collect(),
    };
    let other_conference = !cited_conferences.is_empty()
        && !own_conferences.is_empty()
        && !cited_conferences
            .iter()
            .any(|c| own_conferences.contains(c));
    // A preprint server's copy, cited in a conference's proceedings it
    // can't confirm (a place pooled from its published version's record
    // names no venue).
    let posted = work.container_title.as_deref().is_some_and(preprint_server)
        || record.ids.iter().any(|id| id.kind() == IdKind::Arxiv);
    let confirmed = cited_conferences
        .iter()
        .any(|c| own_conferences.contains(c));
    // Any other meeting by what it's about, its name's furniture aside
    // ("Proceedings of the 2nd International Workshop on User Interface
    // Test Automation" for "… 16th World Conference on Mobile and
    // Contextual Learning"): a venue the claim gives, or its text gives
    // after the title, as a meeting's ("In …", "Proceedings of …"),
    // against a record's proceedings (not a journal called "Proceedings
    // of …").
    let given_venue = claim.container.is_some()
        || title_span.is_some()
        || text.is_some_and(|t| t.raw.trim_start().starts_with('@'));
    let meeting_cited = venue.as_deref().is_some_and(|v| {
        let v = v.trim_start_matches(|c: char| !c.is_alphanumeric());
        claim.container.is_some()
            || publication::proceedings(v)
            || v.starts_with("In ")
            || v.starts_with("in ")
            || v.starts_with("In:")
    });
    // A journal's article cited in a meeting's proceedings ("In
    // Proceedings of the 8th International Symposium on …", BibTeX's
    // `booktitle`) is another version, if any, of the paper cited.
    let booktitle = text.is_some_and(|t| publication::bibtex_field(&t.raw, "booktitle").is_some());
    let in_proceedings = |v: &str| {
        let v = v.trim_start_matches(|c: char| !c.is_alphanumeric());
        (booktitle || v.starts_with("In ") || v.starts_with("In:")) && publication::proceedings(v)
    };
    let other_meeting = !other_conference
        && !confirmed
        && given_venue
        && meeting_cited
        && container.agreement != Agreement::Match
        && !preprint
        && match (venue.as_deref(), work.container_title.as_deref()) {
            // Journals called "Proceedings of …" (the Royal Society's) go
            // by abbreviations this can't tell.
            (Some(cited), Some(actual))
                if work.work_type == Some(WorkType::JournalArticle)
                    && !publication::meeting_name(actual, true) =>
            {
                claim.container.is_none() && in_proceedings(cited)
            }
            // A conference paper's container is its meeting's, whatever
            // its name ("Interaction Design and Children"), when a
            // meeting's proceedings are cited.
            (Some(cited), Some(actual)) => {
                ((work.work_type == Some(WorkType::ConferencePaper)
                    && publication::proceedings(cited))
                    || publication::meeting_name(
                        actual,
                        work.work_type != Some(WorkType::JournalArticle),
                    ))
                    && publication::proceedings_agree(cited, actual) == Some(false)
            }
            _ => false,
        };
    let other_conference = other_conference || other_meeting;
    let preprint_at_conference = preprint
        && posted
        && !confirmed
        && venue.as_deref().is_some_and(publication::proceedings)
        && !named;
    // Nor can a copy in no venue at all (a repository's typed an article,
    // a talk's video record at DataCite), cited as a meeting's: "in Proc.
    // 2010 IEEE International Conference on Robotics and Automation (ICRA)"
    // for an IROS paper's copy. A conference paper's record without its
    // proceedings' name is in one, if unnamed.
    let unplaced_at_meeting = !confirmed
        && work.container_title.is_none()
        && work.volume.is_none()
        && work.page.is_none()
        && matches!(
            work.work_type,
            Some(WorkType::JournalArticle | WorkType::Preprint | WorkType::Other(_))
        )
        && given_venue
        && meeting_cited
        && venue.as_deref().is_some_and(publication::proceedings)
        && !named;
    let preprint_at_conference = preprint_at_conference || unplaced_at_meeting;
    let container = if other_conference
        || (preprint_at_conference && container.agreement != Agreement::Match)
    {
        FieldCheck::exact(false)
    } else {
        container
    };
    VenueChecks {
        container,
        chapter_as_article,
        article_as_chapter,
        other_conference,
        preprint_at_conference,
    }
}
