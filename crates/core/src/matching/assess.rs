//! Scoring one candidate record against a claim ([`assess`]): each field
//! compared, then the caps and floors of [`super::caps`] applied in turn.

use super::authors::{
    self, author_overlap, byline_strangers, claimed_strangers, complete_byline, family_name,
    name_words,
};
use super::caps::{
    CORROBORATING_AUTHORS, DERIVATIVE_CITED_CAP, DERIVATIVE_PENALTY, EDITION, EXTENDED_TITLE,
    EXTENDED_TITLE_CAP, JOURNAL_MISMATCH_CAP, JOURNAL_MISMATCH_PENALTY, LOCATION_MISMATCH_CAP,
    NO_EVIDENCE_CAP, OTHER_IN_SERIES, PLACED, PLACED_YEAR_OFF, PLACED_YEAR_WRONG, PLAUSIBLE,
    SHORT_TITLE_WORDS, STRANGERS_CAP, UNCORROBORATED_CAP, UNPLACED_CAP, VERSION, combine,
};
use super::claim::{byline, cited_venue, cites_chapter, cites_edited_book};
use super::derivative::{self, WorkKind, record_derivative_kind};
use super::preprint::{names_preprint, preprint_server, preprint_version_claimed};
use super::same_work::{id_agreement, same_work_id};
use super::text::{Normalised, content_words, words};
use super::titles::{self, title_similarity, unmarked};
use super::{Agreement, Assessment, Claim, FieldCheck};
use crate::id::{IdKind, is_digits, starts_with_ci};
use crate::publication::{self, first_page, journal_in, other_journal, same_journal};
use crate::record::SourceRecord;
use crate::work::WorkType;

/// A title that is a URL.
fn is_url(title: &str) -> bool {
    let t = title.trim();
    ["http://", "https://", "www."]
        .iter()
        .any(|p| starts_with_ci(t, p))
        || t.contains("://")
}

/// Scores how well `record` matches `claim`.
// The caps and floors apply in order, each reading what the ones before
// left; splitting them up hides that order. Follow-up: move the cap/floor
// tail into `caps.rs` as one function over the field checks.
#[allow(clippy::too_many_lines)]
pub fn assess(claim: &Claim, record: &SourceRecord) -> Assessment {
    let work = &record.work;
    // A BibTeX entry's accents read as letters (`Veli{\v{c}}kovi{\'c}`);
    // acronyms of the record's title's words read as its words ("LLMs").
    let text = claim.text.as_deref().map(|t| {
        let t = if t.trim_start().starts_with('@') {
            publication::delatex(t)
        } else {
            t.to_owned()
        };
        let t = work
            .title
            .as_deref()
            .and_then(|title| titles::expand_acronyms(&t, title))
            .unwrap_or(t);
        Normalised::new(&t)
    });
    // A placeholder record's "title" may be a URL (a registration agency's
    // notice page): no title at all.
    let work_title = work.title.clone().filter(|t| !is_url(t));

    let families: Vec<String> = work.authors.iter().filter_map(family_name).collect();
    // A reference that gives no title ("Wittermans E, et al. ERJ Open Res.
    // 2022") doesn't give a different one.
    let untitled = claim.title.is_none()
        && text
            .as_ref()
            .is_some_and(|t| t.untitled(&families, work.container_title.as_deref()));

    // A BibTeX entry's title is the title cited, not words elsewhere in it
    // (a booktitle that is a proceedings' title).
    let claimed_title = claim.title.clone().or_else(|| {
        text.as_ref()
            .and_then(|t| publication::bibtex_field(&t.raw, "title"))
    });
    let title = match (&claimed_title, &work_title, &text) {
        (Some(claimed), Some(actual), _) => FieldCheck::scored(title_similarity(claimed, actual)),
        (None, Some(actual), Some(text)) if !untitled => {
            FieldCheck::scored(text.title_coverage(actual))
        }
        _ => FieldCheck::UNKNOWN,
    };

    // Where the record's title stands in free text: what precedes it is the
    // byline, what follows the journal and place.
    let title_span = match (&claim.title, &work_title, &text) {
        (None, Some(actual), Some(text)) => text.title_span(actual),
        _ => None,
    };
    let authors = if families.is_empty() {
        FieldCheck::UNKNOWN
    } else if !claim.authors.is_empty() {
        // Organisations named beside people, or alone, aren't compared:
        // records list a committee's members, or no one.
        let people: Vec<String> = claim
            .authors
            .iter()
            .filter(|a| !authors::is_body(a))
            .cloned()
            .collect();
        if people.is_empty() {
            FieldCheck::UNKNOWN
        } else {
            FieldCheck::scored(author_overlap(&people, &families))
        }
    } else if let Some(text) = &text {
        // A reference that names no one ("Injury 47 (2016) 797–804", a
        // pasted title) doesn't name others. One whose byline names
        // people, as any style writes them ("F. Eskdale-Mora", "Nadler,
        // Steven.", BibTeX's author), none of them the record's, does.
        // Not in the work's own title ("ARAGOG: Advanced RAG Output
        // Grading" names no one).
        let outside_title = || match &title_span {
            Some(span) if !span.is_empty() => {
                let (from, to) = (text.spans[span.start].start, text.spans[span.end - 1].end);
                format!("{} {}", &text.raw[..from], &text.raw[to..])
            }
            _ => text.raw.clone(),
        };
        let others = || {
            authors::names_given(&outside_title())
                || byline(text, work_title.as_deref(), title_span.as_ref()).is_some_and(|b| {
                    byline_strangers(&b, &work.authors, work.container_title.as_deref())
                })
        };
        match text.author_coverage(&families) {
            0.0 if !others() => FieldCheck::UNKNOWN,
            coverage => FieldCheck::scored(coverage),
        }
    } else {
        FieldCheck::UNKNOWN
    };
    // Names the claim gives that the record's byline lacks: invented
    // co-authors beside a real first author. Not a match, whoever is first.
    // Not for books, whose records often list only the first author (Open
    // Library's Sutton without Barto).
    let strangers = authors.agreement != Agreement::Unknown
        && work.work_type != Some(crate::work::WorkType::Book)
        && complete_byline(&work.authors)
        && if claim.authors.is_empty() {
            text.as_ref()
                .and_then(|text| byline(text, work_title.as_deref(), title_span.as_ref()))
                .is_some_and(|b| {
                    byline_strangers(&b, &work.authors, work.container_title.as_deref())
                })
        } else {
            claimed_strangers(&claim.authors, &work.authors)
        };
    let authors = match authors.score {
        Some(s) if strangers => FieldCheck::scored(s.min(CORROBORATING_AUTHORS)),
        _ => authors,
    };
    // The record's title with words added: another work's title that
    // contains this one's ("Deep learning for medical image analysis"
    // isn't "Deep learning"), or a subtitle the record lacks. Partly the
    // same, never the same. Books' records often leave their subtitles out
    // ("Imagined Communities", cited with "Reflections on the Origin …"):
    // there, a subtitle after the whole title is no more words.
    let subtitle_ok = work.work_type == Some(crate::work::WorkType::Book)
        && work_title
            .as_deref()
            .is_some_and(|t| titles::main_title(unmarked(t)) == unmarked(t));
    let extended = match (&claimed_title, &work_title, &text, &title_span) {
        (Some(claimed), Some(actual), _, _) => titles::extends(claimed, actual, subtitle_ok),
        (None, Some(_), Some(text), Some(span)) => {
            let journal = work
                .container_title
                .as_deref()
                .map(publication::significant);
            let names = name_words(&work.authors);
            text.extends_title(span, &names, journal.as_deref(), subtitle_ok)
        }
        _ => false,
    };
    let title = match title.score {
        Some(s) if extended => FieldCheck::scored(s.min(EXTENDED_TITLE)),
        _ => title,
    };
    // Or with a word put in place of one of its own (not misspelt):
    // "Hydroxychloroquine in hospitalized patients with Covid-19" at the
    // place of "Dexamethasone in …".
    // A year put in place of the title's own is another title of a series
    // ("Cancer statistics, 2020" isn't the 2018 report, whatever year is
    // cited). Or with words of its own inserted ("… Septic Shock in Adults
    // and Children (Sepsis-3)"); words dropped are a title shortened.
    let found = matches!(title.agreement, Agreement::Match | Agreement::Partial);
    // Or with words replaced anywhere, however many together: "Likelihood
    // Determinants Associated With …" for "Risk Factors Associated With
    // …", "A Controlled Study" for "A Randomized Trial" (synonyms, as an
    // invented reference's or a paraphrase's are).
    let reworded = match (&work_title, &text) {
        (Some(actual), Some(text)) if found && title_span.is_none() && claim.title.is_none() => {
            text.substitutes_in(actual)
                .or_else(|| text.inserts_in(actual).then_some(false))
                .or_else(|| text.replaces_in(actual).then_some(false))
        }
        (Some(actual), _) if found => claim
            .title
            .as_deref()
            .is_some_and(|claimed| titles::replaces(claimed, actual))
            .then_some(false),
        _ => None,
    };
    let substituted = reworded.is_some();
    let title = match (title.score, reworded) {
        (Some(s), Some(true)) => FieldCheck::scored(s.min(OTHER_IN_SERIES)),
        (Some(s), Some(false)) => FieldCheck::scored(s.min(EXTENDED_TITLE)),
        _ => title,
    };

    let actual_years: Vec<i16> = work
        .issued
        .map(|d| d.year())
        .into_iter()
        .chain(work.other_years.iter().copied())
        .collect();
    // A year inside the title ("Cancer statistics, 2020") is not the year
    // cited.
    let claimed_years = claim.cited_years_beside(work_title.as_deref());
    let year = if actual_years.is_empty() || claimed_years.is_empty() {
        FieldCheck::UNKNOWN
    } else {
        // Nearest pairing: any edition's year may be the one cited.
        let delta = claimed_years
            .iter()
            .flat_map(|c| actual_years.iter().map(move |a| c.abs_diff(*a)))
            .min()
            .unwrap_or(u16::MAX);
        // Halves per year off: off by one is common (online-first vs
        // print, preprint vs published) and counts as partial; two or
        // more is a mismatch, but a nearer year still ranks higher.
        FieldCheck::scored(0.5f64.powi(i32::from(delta.min(30))))
    };

    // OpenAlex types some journals' articles as preprints (Genome
    // Research's, under bioRxiv's DOI prefix): one in a journal PubMed
    // indexes is an article.
    let indexed_article = work.work_type == Some(WorkType::Preprint)
        && work
            .container_title
            .as_deref()
            .is_some_and(|c| !preprint_server(c))
        && record.ids.iter().any(|id| id.kind() == IdKind::Pmid);
    let preprint = !indexed_article
        && (work.work_type == Some(WorkType::Preprint)
            || work.container_title.as_deref().is_some_and(preprint_server));
    // Only a journal article's journal is held against a citation: books'
    // series, proceedings and repositories go by many names. A record in a
    // journal whose type isn't given (Europe PMC's editorials, letters,
    // news) is in one too.
    // So is a conference's abstract, or a notice, printed in a journal's
    // volume (a supplement's: "AB0300 …" in the Annals of the Rheumatic
    // Diseases).
    let article = work.work_type == Some(WorkType::JournalArticle)
        || indexed_article
        || (work.work_type.is_none() && work.container_title.is_some() && !preprint)
        || (matches!(work.work_type, Some(WorkType::Other(_)))
            && work.container_title.is_some()
            && work.volume.is_some()
            && !preprint);
    // A journal read from text that is the work's own title, in title case
    // ("… Hooper D. Hot Gravitons From Kerr Black Holes. 2020."), is none.
    // A BibTeX entry's `journal` is one, whatever its words ("Diabetes"
    // beside a title about diabetes).
    let title_words = work_title.as_deref().map(words).unwrap_or_default();
    let bibtex = text
        .as_ref()
        .is_some_and(|t| t.raw.trim_start().starts_with('@'));
    let journals: Vec<String> = claim
        .cited_containers()
        .into_iter()
        .filter(|cited| {
            claim.container.is_some()
                || bibtex
                || !publication::significant(cited)
                    .iter()
                    .all(|w| title_words.contains(w))
        })
        .collect();
    let cites_journal = !journals.is_empty();
    // Read with confidence, or one of a few readings.
    let sure = claim.cited_container().is_some();
    let cited_place = claim.locator();
    // A reference giving a journal, volume and page cites the published
    // article; a preprint of it (in no journal, at no volume) is another
    // version, not the one cited.
    // So is one cited in a journal by name alone ("Science Robotics.
    // 2024."), unless its arXiv identifier is the one claimed (a venue
    // named beside it, "Transformer Circuits Thread. arXiv:2209.11895", as
    // computing and physics cite): a server's DOI with a journal's name
    // names two versions.
    let by_arxiv = claim.ids.iter().any(|id| {
        record.ids.iter().any(|r| same_work_id(r, id))
            && (id.kind() == IdKind::Arxiv || id.to_string().starts_with("doi:10.48550/"))
    });
    let preprint_of_cited = preprint
        && work.volume.is_none()
        && cites_journal
        && journals.iter().all(|j| !preprint_server(j))
        && ((cited_place.volume.is_some() && cited_place.page.is_some())
            || id_agreement(&claim.ids, &record.ids) != Agreement::Match
            || !by_arxiv);
    // And the published version's record, merged with its preprint's,
    // cited in its journal by the preprint's DOI: the journal the record's,
    // the identifier the preprint's.
    let preprint_id_cited =
        article && !preprint && preprint_version_claimed(claim, record).is_some();
    // A journal's article cited as a preprint ("medRxiv preprint, 2019"),
    // without an identifier to say which version: another version.
    let article_as_preprint = article
        && !preprint
        && claim.ids.is_empty()
        && claim.container.is_none()
        && text.as_ref().is_some_and(|t| names_preprint(&t.words));
    // The journal the claim names, or reads where its reference gives one,
    // decides, not words elsewhere in a reference (an author's initials
    // "SR" for Scientific Reports). Names compare by rule, which misses
    // some pairs, so only one clearly another is a mismatch: any work's
    // when claimed, a journal article's when read from text (where a
    // book's publisher may stand); of the readings a reference allows,
    // every one.
    // A sister journal's name holds the record's ("JAMA Intern Med" for
    // JAMA) but is another journal.
    let read = claim.container.is_none() && !bibtex;
    // A journal's section numbered in its volumes ("The Astrophysical
    // Journal Letters, 195, L51" for The Astrophysical Journal's L51) is
    // the journal, at the record's own place.
    let same_text = |c: Option<&str>, a: Option<&str>| matches!((c, a), (Some(c), Some(a)) if c.eq_ignore_ascii_case(a));
    let at_its_place = same_text(cited_place.volume.as_deref(), work.volume.as_deref())
        && same_text(
            cited_place.page.as_deref(),
            work.page.as_deref().map(first_page),
        );
    let container = match &work.container_title {
        Some(actual)
            if journals.iter().any(|cited| {
                (same_journal(cited, actual) || journal_in(&words(cited), actual))
                    && (!publication::sister_journal_as(cited, actual, read)
                        || (at_its_place && publication::section_of(cited, actual)))
            }) =>
        {
            FieldCheck::exact(true)
        }
        // Of several readings, one word ("Sci." of "… Creation. Psychol.
        // Sci.") is the journal only if it names it ("Lancet." of "…
        // China. Lancet."), not if it's one of its words.
        Some(actual)
            if cites_journal
                && (article || claim.container.is_some())
                && journals.iter().all(|cited| {
                    (if read {
                        publication::other_journal_read(cited, actual)
                    } else {
                        other_journal(cited, actual)
                    }) || (journals.len() > 1
                        && publication::significant(cited).len() == 1
                        && !same_journal(cited, actual))
                }) =>
        {
            FieldCheck::exact(false)
        }
        // Not inside its links ("doi:10.1093/brain/…" names no journal),
        // nor inside the work's title ("… monitoring autophagy" names no
        // Autophagy).
        Some(actual)
            if !(cites_journal && sure)
                && text
                    .as_ref()
                    .is_some_and(|t| journal_in(&t.words_outside(title_span.as_ref()), actual)) =>
        {
            FieldCheck::exact(true)
        }
        _ => FieldCheck::UNKNOWN,
    };
    let container = if ((preprint_of_cited || article_as_preprint)
        && container.agreement != Agreement::Match)
        || preprint_id_cited
    {
        FieldCheck::exact(false)
    } else {
        container
    };
    // A book's chapter, in no volume, cited as a journal's article: a
    // volume with an issue ("Biogeochemical Cycles and Climate.
    // 2019;23(5):159-175"), or a journal and volume as fields. The
    // chapter's book, given as the journal, is the place cited, but not
    // in that shape: plausible; another journal is another venue too.
    // (A book series' volume, "Lecture Notes in Computer Science, vol.
    // 9351", has no issue; a chapter cited as one, "In: …", is none.)
    let chapter_as_article = work.work_type == Some(WorkType::Chapter)
        && work.volume.is_none()
        && cited_place.volume.is_some()
        && !claim.text.as_deref().is_some_and(cites_chapter)
        && (claim.text.as_deref().is_some_and(publication::cites_issue)
            || (claim.container.is_some() && container.agreement != Agreement::Match));
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
    let venue = cited_venue(claim, text.as_ref(), title_span.as_ref());
    // A journal's article cited as a chapter of an edited book ("In:
    // Corvaro IR, Mirkrand P, editors. Handbook of Nurse-Midwifery"): the
    // reverse, another container (a meeting's proceedings aside, and a
    // series named as its journal: "In: …, editor. Methods in Enzymology").
    let article_as_chapter = article
        && container.agreement != Agreement::Match
        && claim.text.as_deref().is_some_and(cites_edited_book)
        && !venue.as_deref().is_some_and(publication::proceedings);
    let container = if article_as_chapter {
        FieldCheck::exact(false)
    } else {
        container
    };
    let cited_conferences: Vec<usize> = venue.as_deref().map_or_else(Vec::new, |v| {
        let in_title = work_title
            .as_deref()
            .map(publication::conferences)
            .unwrap_or_default();
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
        || text
            .as_ref()
            .is_some_and(|t| t.raw.trim_start().starts_with('@'));
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
    let booktitle = text
        .as_ref()
        .is_some_and(|t| publication::bibtex_field(&t.raw, "booktitle").is_some());
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
        && id_agreement(&claim.ids, &record.ids) != Agreement::Match;
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
        && id_agreement(&claim.ids, &record.ids) != Agreement::Match;
    let preprint_at_conference = preprint_at_conference || unplaced_at_meeting;
    let container = if other_conference
        || (preprint_at_conference && container.agreement != Agreement::Match)
    {
        FieldCheck::exact(false)
    } else {
        container
    };
    // A journal article cited in a journal clearly not its own, a
    // preprint cited as a journal's article, a paper at another venue.
    let wrong_journal = container.agreement == Agreement::Mismatch
        && (preprint_of_cited
            || preprint_id_cited
            || article_as_chapter
            || article
            || other_conference
            || preprint_at_conference
            || chapter_as_article);
    let located = &cited_place;
    let same = |claimed: Option<&str>, actual: Option<&str>| match (claimed, actual) {
        (Some(c), Some(a)) if !c.is_empty() && !a.is_empty() => {
            FieldCheck::exact(c.trim().eq_ignore_ascii_case(a.trim()))
        }
        _ => FieldCheck::UNKNOWN,
    };
    let volume = same(located.volume.as_deref(), work.volume.as_deref());
    // A chapter cited at a volume is in none: the difference to name.
    let volume = if chapter_as_article && volume.agreement == Agreement::Unknown {
        FieldCheck::exact(false)
    } else {
        volume
    };
    // A record's page with a part number ("698.1", BMJ's) is the page
    // cited as "698".
    let page = same(
        located.page.as_deref(),
        work.page.as_deref().map(first_page).map(|p| {
            match (p.split_once('.'), located.page.as_deref()) {
                (Some((page, part)), Some(cited)) if is_digits(part) && !cited.contains('.') => {
                    page
                }
                _ => p,
            }
        }),
    );

    // Citing the exact place: volume and first page.
    let located = volume.agreement == Agreement::Match && page.agreement == Agreement::Match;
    // Short Vancouver references often leave the title out ("Daum RS et
    // al. N Engl J Med 2017;376:2545"); with the journal and volume
    // agreeing, a title missing from the text is not a different one (the
    // page then decides between verified and a wrong page).
    // Unless it gives one: a few words that are none of the work's authors,
    // journal, numbers or furniture ("Niwa T. Transfection of plasmid DNA
    // by nanocarriers. J Microbiol Methods. 2005;61:251" isn't titleless).
    let placed = container.agreement == Agreement::Match && volume.agreement == Agreement::Match;
    // A title quoted, or BibTeX's, is given however short ("“The essence
    // of entanglement,” Phys. Rev. A, vol. 63").
    let titled = text.as_ref().is_some_and(|t| {
        t.title_run(&families, work.container_title.as_deref()) >= SHORT_TITLE_WORDS
            || titles::given_title(&t.raw).is_some()
    });
    let title =
        if placed && claim.title.is_none() && title.agreement == Agreement::Mismatch && !titled {
            FieldCheck::UNKNOWN
        } else {
            title
        };

    let ids = id_agreement(&claim.ids, &record.ids);
    // A claim of identifiers alone is fully confirmed by a matching ID.
    let id_only = [title, authors, year].iter().all(|c| c.score.is_none());
    let mut confidence = if id_only && ids == Agreement::Match {
        1.0
    } else {
        combine(title, authors, year)
    };
    // Title and year alone can't verify: a short or generic title ("Health
    // technology assessment") turns up inside many references, and letters
    // and corrections often carry the original's title without authors.
    // Corroboration means the first author matches, or a claimed ID does.
    // So does citing the exact place.
    let corroborated = authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        || ids == Agreement::Match
        || located;
    if !id_only && !corroborated {
        confidence = confidence.min(UNCORROBORATED_CAP);
    }
    // A title of a word or two inside free text ("Aspero" in "aspero 0.6.5
    // is in") is no evidence by itself, nor with a year: it takes the
    // authors, the journal, the place or an identifier.
    let short_title_in_text = claim.title.is_none()
        && text.is_some()
        && work_title
            .as_deref()
            .is_some_and(|t| content_words(t).len() < SHORT_TITLE_WORDS);
    if short_title_in_text && !corroborated && container.agreement != Agreement::Match {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // A year, or a year and journal, identify nothing: some work matches
    // them by chance. It takes the title, authors, an identifier or the
    // place.
    let identified =
        matches!(title.agreement, Agreement::Match | Agreement::Partial) || corroborated;
    if !id_only && !identified {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // Without a title, authors, a year and a journal fit many works: it
    // takes an identifier, the place, or the journal, volume and authors.
    let pinned = ids == Agreement::Match
        || located
        || (placed && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS));
    if untitled && !pinned {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // A title that only partly agrees, with nothing else agreeing, is a
    // few shared words: "Journal of the American College of Surgeons"
    // inside a reference whose author is the College.
    let agrees = |c: &FieldCheck| matches!(c.agreement, Agreement::Match | Agreement::Partial);
    if title.agreement == Agreement::Partial
        && ![authors, year, container].iter().any(agrees)
        && ids != Agreement::Match
        && !located
    {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // Journal, volume and first page together name one work, title or no
    // title ("Injury 47 (2016) 797–804"), unless something contradicts
    // it.
    // Another title at this work's place, by its authors: a title
    // translated or paraphrased, or one work's title cited at another's
    // place. Plausible, its title to check; never verified.
    if located
        && container.agreement == Agreement::Match
        && title.agreement == Agreement::Mismatch
        && authors.agreement == Agreement::Match
        && year.agreement != Agreement::Mismatch
    {
        confidence = confidence.max(PLAUSIBLE);
    }
    if located
        && container.agreement == Agreement::Match
        && title.agreement != Agreement::Mismatch
        && authors.agreement != Agreement::Mismatch
    {
        let floor = match year.agreement {
            Agreement::Match | Agreement::Unknown => PLACED,
            Agreement::Partial => PLACED_YEAR_OFF,
            Agreement::Mismatch => 0.0,
        };
        confidence = confidence.max(floor);
    }
    // The whole title (a year in it too), the authors, volume and first
    // page name the work, a year two or more off notwithstanding: the
    // year is the citation's error ("Cancer statistics, 2020 … 2018;70:7"),
    // plausible, and ranks this work over the one of the year cited.
    if located
        && title.agreement == Agreement::Match
        && authors.agreement == Agreement::Match
        && container.agreement != Agreement::Mismatch
        && year.agreement == Agreement::Mismatch
    {
        confidence = confidence.max(PLACED_YEAR_WRONG);
    }
    // The claimed identifier with the whole title and the place name the
    // work: authors that differ are the citation's error, or a record's
    // garbled byline ("Ruff, Paul Jurica, Ronald M" as one name).
    // Plausible, the authors to check; never verified.
    if ids == Agreement::Match
        && located
        && title.agreement == Agreement::Match
        && authors.agreement == Agreement::Mismatch
        && year.agreement != Agreement::Mismatch
    {
        confidence = confidence.max(PLAUSIBLE);
    }
    // A wrong volume or page is an error in the citation, and tells an
    // original from the letters and corrections in the same journal.
    if [volume, page]
        .iter()
        .any(|c| c.agreement == Agreement::Mismatch)
    {
        confidence = confidence.min(LOCATION_MISMATCH_CAP);
    }
    // Another journal lowers the ranking; for a journal article (the wrong
    // journal, a common error in real and invented references) it is an
    // error in the citation, as a wrong volume is.
    if container.agreement == Agreement::Mismatch {
        confidence *= JOURNAL_MISMATCH_PENALTY;
    }
    if wrong_journal {
        confidence = confidence.min(JOURNAL_MISMATCH_CAP);
    }
    // A copy deposited elsewhere (Zenodo, a repository, a technical
    // report) has the work's title, authors and year but no journal,
    // volume or page: it can't confirm the journal cited, and it isn't the
    // article cited there, unless the claim's identifier says it is.
    // Preprints, books, chapters, data and software have none to give,
    // but nor do they confirm a journal, volume and page all cited
    // (PsycEXTRA's record of a paper Science printed).
    let unconfirmable = cited_place.volume.is_some()
        && cited_place.page.is_some()
        && work.volume.is_none()
        && work.page.is_none()
        && container.agreement != Agreement::Match;
    let journal_less = !matches!(
        work.work_type,
        Some(
            WorkType::Preprint
                | WorkType::Book
                | WorkType::Chapter
                | WorkType::Dataset
                | WorkType::Software
        )
    ) && work.container_title.is_none()
        && work.volume.is_none()
        && work.page.is_none();
    // A BibTeX `@article` with its journal cites a journal's article: a
    // book of that title (a working paper's monograph) isn't it.
    let book_for_article = work.work_type == Some(WorkType::Book)
        && text.as_ref().is_some_and(|t| {
            t.raw
                .trim_start()
                .get(..8)
                .is_some_and(|head| head.eq_ignore_ascii_case("@article"))
        });
    // Nor is a book cited with a volume and issue (its title where the
    // journal stands: "Methane Cycling and Climate. Biogeochemical Cycles
    // and Climate. 2019;23(5):159-175" cites a chapter of it).
    let book_as_journal = work.work_type == Some(WorkType::Book)
        && work.volume.is_none()
        && claim.text.as_deref().is_some_and(publication::cites_issue);
    if chapter_as_article || (book_as_journal && ids != Agreement::Match) {
        confidence = confidence.min(UNPLACED_CAP);
    }
    if (journal_less || unconfirmable || book_for_article)
        && cites_journal
        && ids != Agreement::Match
    {
        confidence = confidence.min(UNPLACED_CAP);
    }
    // Another edition of a book: the same title and authors, another year
    // (a 9th edition cited, a later one found). The book is right; the
    // year, or a publisher given as its container, is a difference. A
    // chapter cited in the book is not the book: its authors found may be
    // the book's editors. A book cited with its publisher, found reprinted
    // whole as a chapter of a reader (Bourdieu's "Distinction" in a 2018
    // collection), is another edition too.
    let cites_book = text.as_ref().is_some_and(|t| {
        t.words.iter().any(|w| {
            matches!(
                w.as_str(),
                "press" | "publisher" | "publishers" | "publishing" | "books" | "verlag"
            )
        })
    });
    if (work.work_type == Some(WorkType::Book)
        || (work.work_type == Some(WorkType::Chapter) && cites_book))
        && (title.agreement == Agreement::Match || extended)
        && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        && year.agreement == Agreement::Mismatch
        && !claim.text.as_deref().is_some_and(cites_chapter)
    {
        confidence = confidence.max(EDITION);
    }
    // A dataset's or software's record carries its latest version's date:
    // the claimed identifier and title name it, whatever year is cited
    // (MIMIC-III's DOI, cited as of 2016, registered in 2023).
    if matches!(work.work_type, Some(WorkType::Dataset | WorkType::Software))
        && ids == Agreement::Match
        && title.agreement == Agreement::Match
        && year.agreement == Agreement::Mismatch
    {
        confidence = confidence.max(VERSION);
    }
    // Authors the work doesn't have are an error in the citation, and a
    // tell of invented references, whatever else agrees.
    if strangers {
        confidence = confidence.min(STRANGERS_CAP);
    }
    // So is a title with words the work's lacks, or one of its own
    // replaced, whatever else agrees.
    if extended || substituted {
        confidence = confidence.min(EXTENDED_TITLE_CAP);
    }
    // And a title only partly the work's, words of the work's missing (not
    // misspelt: "Ailgnment"): reworded, as another version's title is
    // ("Preliminary adaptation of cognitive behavioral therapy …" for a
    // preprint's "Preliminary adaptation process of …"), or another
    // work's. Plausible, unless the claimed identifier or the place says
    // which work is cited.
    let misspelt = match (&claimed_title, &work_title, &text) {
        (Some(claimed), Some(actual), _) => titles::misspelt_only(&words(claimed), actual, true),
        (None, Some(actual), Some(text)) => titles::misspelt_only(&text.words, actual, false),
        _ => true,
    };
    if title.agreement == Agreement::Partial && !misspelt && ids != Agreement::Match && !located {
        confidence = confidence.min(EXTENDED_TITLE_CAP);
    }
    // A reply, correction or commentary is not the work it responds to,
    // unless the citation says it is citing one; nor is a notice its record
    // types as one, whatever its title ("… – RETRACTION").
    let cites_derivative = derivative::claim_derivative_kind(claim);
    let derivative = work_title.is_some() && record_derivative_kind(work).is_some();
    if derivative && cites_derivative.is_none() {
        confidence *= DERIVATIVE_PENALTY;
    }
    // Nor is the original a correction, reply or comment cited ("Corrigendum
    // to “…” [Biochim. Biophys. Acta 1236 (1995) 197-200]"), its title and
    // place quoted inside: plausible at most, the notice's record ranking
    // first when found. (A retraction cited isn't held to this: "retraction
    // of the gingiva" is a title's.)
    if work_title.is_some()
        && !derivative
        && matches!(
            cites_derivative,
            Some(WorkKind::Correction | WorkKind::Reply | WorkKind::Comment)
        )
    {
        confidence = confidence.min(DERIVATIVE_CITED_CAP);
    }

    Assessment {
        confidence,
        title,
        authors,
        year,
        container,
        volume,
        page,
        ids,
    }
}
