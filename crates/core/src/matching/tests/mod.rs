//! Scoring against hand-built records, each case from a real failure.

use jiff::Timestamp;

use super::caps::SINGLE_FIELD_CAP;
use super::text::Normalised;
use super::*;
use crate::id::WorkId;
use crate::provider::ProviderId;
use crate::publication;
use crate::record::{FieldSet, Raw, SourceRecord};
use crate::work::{PartialDate, Person, Work};

mod authors;
mod claim;
mod journal;
mod redteam;
mod redteam_holdouts;
mod redteam_round6;
mod redteam_round7;
mod scoring;
mod titles;
mod venue;
mod versions;

fn record(ids: &[&str], title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let work = Work {
        title: Some(title.into()),
        authors: authors.iter().map(|a| Person::literal(*a)).collect(),
        issued: Some(PartialDate::from_year(year)),
        ..Work::default()
    };
    let raw = Raw {
        media_type: "application/json".into(),
        body: "{}".into(),
    };
    SourceRecord::new(
        ProviderId::from_static("test"),
        ids.iter().map(|i| i.parse().unwrap()).collect(),
        work,
        FieldSet::ALL,
        raw,
        Timestamp::UNIX_EPOCH,
    )
}

const VASWANI: [&str; 3] = ["Ashish Vaswani", "Noam Shazeer", "Niki Parmar"];

fn attention(ids: &[&str], year: i16) -> SourceRecord {
    record(ids, "Attention Is All You Need", &VASWANI, year)
}

/// The PEITHO trial as published: NEJM 2014;370(15):1402-11.
fn peitho() -> SourceRecord {
    let mut r = record(
        &["10.1056/nejmoa1302097"],
        "Fibrinolysis for Patients with Intermediate-Risk Pulmonary Embolism",
        &["Guy Meyer", "Eric Vicaut"],
        2014,
    );
    r.work.container_title = Some("New England Journal of Medicine".into());
    r.work.volume = Some("370".into());
    r.work.page = Some("1402-1411".into());
    r
}

/// A record with where it appeared.
fn placed(
    ids: &[&str],
    title: &str,
    authors: &[&str],
    year: i16,
    at: (&str, &str, &str),
) -> SourceRecord {
    let mut r = record(ids, title, authors, year);
    r.work.container_title = Some(at.0.into());
    r.work.volume = Some(at.1.into());
    r.work.page = Some(at.2.into());
    r
}

/// #52: the ATLS manual, 9th edition (2012), cited by its organisation.
const ATLS: &str = "American College of Surgeons. Advanced trauma life support student \
                    course manual. 9th ed. Chicago, IL: American College of Surgeons; 2012.";

/// #80: Ragusa et al., Plasticenta, in Environment International.
fn plasticenta() -> SourceRecord {
    let mut r = placed(
        &["10.1016/j.envint.2020.106274"],
        "Plasticenta: First evidence of microplastics in human placenta",
        &[
            "Antonio Ragusa",
            "Alessandro Svelato",
            "Criselda Santacroce",
        ],
        2021,
        ("Environment International", "146", "106274"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

const PLASTICENTA_VANCOUVER: &str = "Ragusa A, Svelato A, Santacroce C, et al. Plasticenta: \
    first evidence of microplastics in human placenta. Environ Int. 2021;146:106274.";

const PLASTICENTA_APA: &str = "Ragusa, A., Svelato, A., & Santacroce, C. (2021). Plasticenta: \
    First evidence of microplastics in human placenta. Environment International, 146, 106274.";

/// Red team (#102): a real work cited with co-authors it doesn't have.
fn egdt() -> SourceRecord {
    let mut r = placed(
        &[],
        "Early Goal-Directed Therapy in the Treatment of Severe Sepsis and Septic Shock",
        &["Emanuel Rivers", "Bryant Nguyen", "Suzanne Havstad"],
        2001,
        ("New England Journal of Medicine", "345", "1368-1377"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

const EGDT: &str = "Early goal-directed therapy in the treatment of severe sepsis and septic shock";

fn lecun() -> SourceRecord {
    let mut r = placed(
        &["10.1038/nature14539"],
        "Deep learning",
        &["Yann LeCun", "Yoshua Bengio", "Geoffrey Hinton"],
        2015,
        ("Nature", "521", "436-444"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

fn hallmarks() -> SourceRecord {
    let mut r = placed(
        &[],
        "Hallmarks of Cancer: The Next Generation",
        &["Douglas Hanahan", "Robert A. Weinberg"],
        2011,
        ("Cell", "144", "646-674"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

/// A journal article's record for the red-team table.
fn article(title: &str, authors: &[&str], year: i16, at: (&str, &str, &str)) -> SourceRecord {
    let mut r = placed(&[], title, authors, year, at);
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

/// A conference paper's record: its proceedings as the container.
fn proceedings(title: &str, authors: &[&str], year: i16, venue: &str, pages: &str) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    r.work.container_title = Some(venue.into());
    r.work.page = Some(pages.into());
    r
}

/// An arXiv preprint's record, as OpenAlex gives it: in no venue.
fn arxiv_preprint(id: &str, title: &str, authors: &[&str], year: i16) -> SourceRecord {
    let mut r = record(&[id], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::Preprint);
    r.work.container_title = Some("arXiv (Cornell University)".into());
    r
}

/// A book chapter's record, in no volume: its book as the container.
fn chapter(title: &str, authors: &[&str], year: i16, book: &str) -> SourceRecord {
    let mut r = record(&[], title, authors, year);
    r.work.work_type = Some(crate::work::WorkType::Chapter);
    r.work.container_title = Some(book.into());
    r
}
