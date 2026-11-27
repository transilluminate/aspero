//! Scoring one candidate record against a claim ([`assess`]): each field
//! compared, then the caps and floors of [`super::caps`] applied in turn.

use super::authors::{
    self, author_overlap, byline_strangers, claimed_strangers, complete_byline, family_name,
    name_words,
};
use super::caps::{
    CORROBORATING_AUTHORS, DERIVATIVE_CITED_CAP, DERIVATIVE_PENALTY, EDITION, EXTENDED_TITLE,
    EXTENDED_TITLE_CAP, FIRST_EDITION_CAP, ID_CONFLICT_CAP, JOURNAL_MISMATCH_CAP,
    JOURNAL_MISMATCH_PENALTY, LOCATION_MISMATCH_CAP, NO_EVIDENCE_CAP, OTHER_IN_SERIES, PLACED,
    PLACED_YEAR_OFF, PLACED_YEAR_WRONG, PLAUSIBLE, SHORT_TITLE_WORDS, STRANGERS_CAP,
    UNCORROBORATED_CAP, UNPLACED_CAP, VENUE_RECORD_CAP, VERSION, combine,
};
use super::claim::{byline, cites_chapter, title_region, untitled_byline};
use super::derivative::{self, WorkKind, record_derivative_kind};
use super::edition;
use super::preprint::{names_preprint, preprint_server, preprint_version_claimed};
use super::record_title::record_title;
use super::same_work::{id_agreement, same_work_id};
use super::text::{self, Normalised, content_words, is_url, words};
use super::titles::{self, title_similarity, unmarked};
use super::venue;
use super::version;
use super::written;
use super::{Agreement, Assessment, Claim, FieldCheck};
use crate::id::{IdKind, is_digits};
use crate::publication::{self, first_page, journal_in, other_journal, same_journal};
use crate::record::SourceRecord;
use crate::work::WorkType;

/// A quoted span's similarity to the record's title, at least, for it to be
/// read as the title cited.
const QUOTED_TITLE_SHARE: f64 = 2.0 / 3.0;

/// Scores how well `record` matches `claim`.
// The caps and floors apply in order, each reading what the ones before
// left; splitting them up hides that order. Follow-up: move the facts the
// tail computes into their field modules (the record's kind against the
// venue is `venue::record_kind`), then the tail into `caps.rs` as one
// function over the field checks and those facts.
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
    // Markup and a record's furniture the citation doesn't give taken out
    // ("tt¯H", ", 14th Edition", "(Verso Classics, 10)").
    let work_title = work.title.as_deref().filter(|t| !is_url(t)).map(|t| {
        let cited = claim.title.as_deref().or(claim.text.as_deref());
        record_title(
            t,
            cited.unwrap_or_default(),
            work.work_type == Some(WorkType::Book),
        )
    });
    // A claimed identifier names the work, even beside another (a PMID)
    // that is another's: identified, though not confirmed (#148).
    // An earlier version's identifier, named by a newer version's record
    // (a Cochrane review's), names that version, not this one.
    let ids = match id_agreement(&claim.ids, &record.ids) {
        Agreement::Match if version::earlier_id_claimed(claim, record) => Agreement::Partial,
        agreement => agreement,
    };
    let named = matches!(ids, Agreement::Match | Agreement::Partial);

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
    // Library's Sutton without Barto), or their editors or translators,
    // unless the reference swaps one of the record's co-authors for
    // another (`authors::book_co_author_swapped`, wild-6 #267).
    let journal = work
        .container_title
        .as_deref()
        .map(publication::significant);
    let book = work.work_type == Some(crate::work::WorkType::Book);
    let strangers = authors.agreement != Agreement::Unknown
        && complete_byline(&work.authors)
        && if claim.authors.is_empty() {
            text.as_ref()
                .and_then(|text| {
                    byline(text, work_title.as_deref(), title_span.as_ref())
                        .or_else(|| untitled_byline(text, journal.as_deref()))
                })
                .is_some_and(|b| {
                    let journal = work.container_title.as_deref();
                    if book {
                        authors::book_co_author_swapped(&b, &work.authors, journal)
                    } else {
                        byline_strangers(&b, &work.authors, journal)
                    }
                })
        } else {
            !book && claimed_strangers(&claim.authors, &work.authors)
        };
    // Or names an earlier version's first author.
    let earlier = authors.agreement != Agreement::Unknown
        && version::earlier_byline(
            claim,
            record,
            text.as_ref()
                .and_then(|t| byline(t, work_title.as_deref(), title_span.as_ref()))
                .as_deref(),
            &families,
        );
    let authors = match authors.score {
        Some(s) if strangers || earlier => FieldCheck::scored(s.min(CORROBORATING_AUTHORS)),
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
    // Found with an article given or left out, too.
    let loose_span = title_span.clone().or_else(|| {
        work_title
            .as_deref()
            .zip(text.as_ref())
            .and_then(|(t, x)| x.loose_title_span(t))
    });
    let extended = match (&claimed_title, &work_title, &text, &loose_span) {
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
    // Where only its main title was found ("\"High and Noble Adventures\":
    // Reading the Novel in \"The Male Quixote\"" for "… The Female
    // Quixote"), a word put in place in the rest.
    let main_only = title_span.as_ref().is_some_and(|span| {
        work_title
            .as_deref()
            .is_some_and(|t| span.len() < words(unmarked(t)).len())
    });
    let reworded = match (&work_title, &text) {
        (Some(actual), Some(text)) if found && title_span.is_none() && claim.title.is_none() => {
            text.substitutes_in(actual)
                .or_else(|| text.inserts_in(actual).then_some(false))
                .or_else(|| text.replaces_in(actual).then_some(false))
                // A title quoted is the title cited, all of it, as one
                // given apart is (wild-6 #653); not a reference quoted
                // whole ("Corrections to “Shamloul R, Bella AJ. …”").
                .or_else(|| {
                    titles::given_title(&text.raw)
                        .is_some_and(|quoted| {
                            title_similarity(&quoted, actual) >= QUOTED_TITLE_SHARE
                                && titles::replaces(&quoted, actual)
                        })
                        .then_some(false)
                })
        }
        (Some(actual), Some(text)) if found && main_only && claim.title.is_none() => {
            text.substitutes_in(actual)
        }
        (Some(actual), _) if found => claim
            .title
            .as_deref()
            .is_some_and(|claimed| titles::replaces(claimed, actual))
            .then_some(false),
        _ => None,
    };
    let substituted = reworded.is_some();
    // Or cut off, not at a subtitle (`Normalised::cut_off`, wild-5 #353,
    // #762): the words dropped may be what the work found.
    let cut = work_title
        .as_deref()
        .is_some_and(|actual| text::title_cut(claimed_title.as_deref(), text.as_ref(), actual));
    // Or without its opening words, the ones the rest depends on
    // (`Normalised::cut_in_front`, wild-6 #49). Not a notice's ("Correction
    // to “…”"), cited for its original: `derivative.rs` tells those.
    let cut = cut
        || (record_derivative_kind(work).is_none()
            && work_title.as_deref().is_some_and(|actual| {
                text::title_cut_in_front(claimed_title.as_deref(), text.as_ref(), actual)
            }));
    let title = match (title.score, reworded) {
        (Some(s), Some(true)) => FieldCheck::scored(s.min(OTHER_IN_SERIES)),
        (Some(s), Some(false)) => FieldCheck::scored(s.min(EXTENDED_TITLE)),
        (Some(s), None) if cut => FieldCheck::scored(s.min(EXTENDED_TITLE)),
        _ => title,
    };

    // Not a newer version's year, merged in, for this version claimed.
    let newer = version::newer_years(claim, record);
    let actual_years: Vec<i16> = work
        .issued
        .map(|d| d.year())
        .into_iter()
        .chain(
            work.other_years
                .iter()
                .copied()
                .filter(|y| !newer.contains(y)),
        )
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
    // One given a volume and page is a journal ("Physiotherapy, 40(8),
    // pp. 1753-1766"); a series' volume alone isn't.
    let volume_given = claim.place().is_some();
    let journals: Vec<String> = claim
        .cited_containers()
        .into_iter()
        .filter(|cited| {
            claim.container.is_some()
                || bibtex
                || volume_given
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
        && ((cited_place.volume.is_some() && cited_place.page.is_some()) || !named || !by_arxiv);
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
    // Not one read right after the work's title ("… trial. Hand Therapy.
    // 2014;27(4)" for the Journal of Hand Therapy): whole, not cut short.
    let read_from = |cited: &str| {
        claim.container.is_none()
            && !bibtex
            && !title_span
                .as_ref()
                .zip(text.as_ref())
                .is_some_and(|(span, t)| {
                    let name = words(cited);
                    !name.is_empty() && t.words[span.end..].starts_with(&name)
                })
    };
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
                    && (!publication::sister_journal_as(cited, actual, read_from(cited))
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
                    (if read_from(cited) {
                        publication::other_journal_read(cited, actual)
                    } else {
                        other_journal(cited, actual)
                    }) || (journals.len() > 1
                        && publication::significant(cited).len() == 1
                        && !publication::names_journal(cited, actual))
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
    // The venue cited against the record's, in `venue.rs`.
    let venue::VenueChecks {
        container,
        chapter_as_article,
        article_as_chapter,
        other_conference,
        preprint_at_conference,
    } = venue::venue_checks(&venue::VenueInputs {
        claim,
        record,
        text: text.as_ref(),
        title_span: title_span.as_ref(),
        work_title: work_title.as_deref(),
        cited_place: &cited_place,
        article,
        preprint,
        named,
        cites_journal,
        container,
    });
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
    // of entanglement,” Phys. Rev. A, vol. 63"). So is one the layout
    // reads beside other authors ("LeCun Y, Bengio Y, Hinton G. Deep
    // learning. Nature. 2015;521" cites no title of Littman's at Nature
    // 521, #172), unless all its words are the work's authors, journal or
    // furniture ("N Engl J Med"). Beside the work's own authors, a short
    // one is a label for it ("Schmidt M, et al. RESP score. Am J Respir
    // Crit Care Med 2014;189:1374").
    let others = authors.score.is_none_or(|s| s < CORROBORATING_AUTHORS);
    // Not a label it opens with, nor words after its place (a model's
    // commentary): `claim::title_region`.
    let titled = text.as_ref().is_some_and(|t| {
        let journal = work.container_title.as_deref();
        let region = Normalised::new(title_region(&t.raw));
        region.title_run(&families, journal) >= SHORT_TITLE_WORDS
            || titles::given_title(&t.raw).is_some()
            || (others
                && written::laid_out(&region.raw)
                    .title
                    .is_some_and(|read| Normalised::new(&read).title_run(&families, journal) > 0))
    });
    let title =
        if placed && claim.title.is_none() && title.agreement == Agreement::Mismatch && !titled {
            FieldCheck::UNKNOWN
        } else {
            title
        };
    // A title found in free text only as a word or two ("Iceland" of
    // "Iceland: A Peaceful Secession" inside "The History of Iceland";
    // "13th") is no full match: only partly the work's, unless its
    // authors, a claimed identifier, the place or the journal say which
    // work it is. Nor, then, is it the same title with another field
    // wrong (a closest work).
    let pinned_by_more = authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        || named
        || located
        || container.agreement == Agreement::Match;
    let title = match (&claimed_title, &work_title, &text) {
        (None, Some(actual), Some(text))
            if title.agreement == Agreement::Match
                && !pinned_by_more
                && text.found_only_short(actual) =>
        {
            FieldCheck::scored(title.score.unwrap_or(1.0).min(EXTENDED_TITLE))
        }
        _ => title,
    };

    // A claim of identifiers alone is fully confirmed by a matching ID.
    let id_only = [title, authors, year].iter().all(|c| c.score.is_none());
    // A claimed identifier that is the work's, beside a title, authors and
    // year (and journal and place) that all agree as far as they're given,
    // is a second field: "Title. arXiv:ID" is no weaker than the identifier
    // alone. (A title alone, without one, can be a review's or a
    // correction's.)
    let agree = [title, authors, year, container, volume, page]
        .iter()
        .all(|c| matches!(c.agreement, Agreement::Match | Agreement::Unknown));
    let mut confidence = if id_only && named {
        1.0
    } else {
        combine(title, authors, year, ids == Agreement::Match && agree)
    };
    // Title and year alone can't verify: a short or generic title ("Health
    // technology assessment") turns up inside many references, and letters
    // and corrections often carry the original's title without authors.
    // Corroboration means the first author matches, or a claimed ID does.
    // So does citing the exact place.
    let corroborated =
        authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS) || named || located;
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
    let pinned =
        named || located || (placed && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS));
    if untitled && !pinned {
        confidence = confidence.min(NO_EVIDENCE_CAP);
    }
    // A title that only partly agrees, with nothing else agreeing, is a
    // few shared words: "Journal of the American College of Surgeons"
    // inside a reference whose author is the College.
    let agrees = |c: &FieldCheck| matches!(c.agreement, Agreement::Match | Agreement::Partial);
    if title.agreement == Agreement::Partial
        && ![authors, year, container].iter().any(agrees)
        && !named
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
    // A claimed identifier that is the work's, beside its authors, names it
    // whatever free text stands where its title would: a trial's label
    // ("AReSVi-006 (GSK, RSVPreF3 OA):"), a description, a short form
    // ("Hash Layers (Roller et al.) | 2106.04426"). The record's title,
    // searched for in the text, isn't the title cited. Plausible, its
    // title to check; never verified. Not with the year, volume or page
    // wrong: another paper's identifier, beside a namesake's surname
    // (holdout-6 #802).
    if ids == Agreement::Match
        && claimed_title.is_none()
        && title.agreement == Agreement::Mismatch
        && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        && [year, volume, page]
            .iter()
            .all(|c| c.agreement != Agreement::Mismatch)
    {
        confidence = confidence.max(PLAUSIBLE);
    }
    // A quoted part of the work's title (its opening words or subtitle) beside
    // its authors, year and journal: the work (wild-5 #400, #488). Plausible.
    if claimed_title.is_none()
        && title.agreement == Agreement::Mismatch
        && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        && !(strangers || earlier)
        && year.agreement == Agreement::Match
        && container.agreement == Agreement::Match
        && (text.as_ref().zip(work_title.as_deref()))
            .is_some_and(|(t, w)| titles::quotes_part_of(&t.raw, w))
    {
        confidence = confidence.max(PLAUSIBLE);
    }
    // A claimed identifier and the exact place (journal, volume, first
    // page) name the same work twice: a title or a byline that differs
    // there is the citation's error ("Observation of ttH production" for
    // "… tt¯H production", a table's label read as a byline). Plausible,
    // unless both differ (another paper's title and authors at its place).
    // Not beside a claimed PMID another paper's: then an identifier
    // differs too.
    if ids == Agreement::Match
        && located
        && container.agreement == Agreement::Match
        && year.agreement != Agreement::Mismatch
        && !(title.agreement == Agreement::Mismatch && authors.agreement == Agreement::Mismatch)
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
    if named
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
    // What the record is, against the work and venue cited, in
    // `venue.rs`: each fact below caps or floors in this order.
    let kind = venue::record_kind(&venue::KindInputs {
        claim,
        record,
        text: text.as_ref(),
        title_span: title_span.as_ref(),
        work_title: work_title.as_deref(),
        claimed_title: claimed_title.as_deref(),
        cited_place: &cited_place,
        named,
        cites_journal,
        container,
        claimed_years: &claimed_years,
        actual_years: &actual_years,
    });
    if chapter_as_article || (kind.book_cited_with_issue && !named) {
        confidence = confidence.min(UNPLACED_CAP);
    }
    if kind.not_the_article && cites_journal && !named {
        confidence = confidence.min(UNPLACED_CAP);
    }
    // Another edition of a book: the same title and authors, another year
    // (a 9th edition cited, a later one found). The book is right; the
    // year, or a publisher given as its container, is a difference. A
    // chapter cited in the book is not the book: its authors found may be
    // the book's editors.
    if kind.edition
        && (title.agreement == Agreement::Match || extended)
        && authors.score.is_some_and(|s| s >= CORROBORATING_AUTHORS)
        && year.agreement == Agreement::Mismatch
        && !claim.text.as_deref().is_some_and(cites_chapter)
    {
        confidence = confidence.max(EDITION);
    }
    // A book cited as first appearing in a year ("(Original work
    // published 1969)", "(1st ed.)") of an edition it has, with one more
    // than three years earlier: a later edition's year given as the
    // original's.
    // The edition cited may exist; the year is still wrong. Plausible,
    // the year a difference (the work's is its first edition's).
    let first_cited = text
        .as_ref()
        .and_then(|t| edition::claimed_first_year(t, title_span.as_ref(), &claimed_years));
    let year = match first_cited
        .and_then(|first| Some((first, edition::earlier_edition(first, &actual_years)?)))
    {
        Some((first, earliest))
            if work.work_type == Some(WorkType::Book) && title.agreement == Agreement::Match =>
        {
            confidence = confidence.min(FIRST_EDITION_CAP);
            FieldCheck::scored(0.5f64.powi(i32::from(first.abs_diff(earliest).min(30))))
        }
        _ => year,
    };
    if kind.part_for_whole {
        confidence = confidence.min(LOCATION_MISMATCH_CAP);
    }
    if kind.volume_for_paper {
        confidence = confidence.min(VENUE_RECORD_CAP);
    }
    // A dataset's or software's record carries its latest version's date:
    // the claimed identifier and title name it, whatever year is cited
    // (MIMIC-III's DOI, cited as of 2016, registered in 2023).
    if matches!(work.work_type, Some(WorkType::Dataset | WorkType::Software))
        && named
        && title.agreement == Agreement::Match
        && year.agreement == Agreement::Mismatch
    {
        confidence = confidence.max(VERSION);
    }
    // Authors the work doesn't have are an error in the citation, and a
    // tell of invented references, whatever else agrees. An earlier
    // version's byline is another version's.
    if strangers || earlier {
        confidence = confidence.min(STRANGERS_CAP);
    }
    // Nor can a record without a byline confirm the people a citation
    // names (holdout-5: invented authors of a JAMA piece whose record
    // lists none; a correction notice's record for its original's).
    let names_people = claim.authors.iter().any(|a| !authors::is_body(a))
        || (claim.authors.is_empty()
            && text.as_ref().is_some_and(|t| {
                let raw = match &title_span {
                    Some(span) if !span.is_empty() => &t.raw[..t.spans[span.start].start],
                    _ => t.raw.as_str(),
                };
                authors::names_given(raw)
            }));
    if work.authors.is_empty() && names_people && !named {
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
    // The identifier or place doesn't make up for a title cut off, nor for one
    // given apart (structured, BibTeX or RIS) and reworded: an RIS record's TI
    // rewritten beside its DOI and place (wild-5 #194). Plausible.
    let given_apart = claimed_title.is_some();
    if cut
        || (title.agreement == Agreement::Partial
            && !misspelt
            && ((!named && !located) || given_apart))
    {
        confidence = confidence.min(EXTENDED_TITLE_CAP);
    }
    // A claimed PMID or PMCID that isn't the work's, beside an identifier
    // that is: another paper's, a few digits off (#148), or the record's
    // own mistake. Plausible, the identifier to check; not a mismatch.
    if ids == Agreement::Partial {
        confidence = confidence.min(ID_CONFLICT_CAP);
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
