//! Checking records against the DOI registry (Crossref): a record that
//! contradicts it is flagged, isn't trusted for dating notices, and loses
//! to the registry's record of that DOI.

mod common;

use std::sync::Arc;

use aspero_core::{
    Claim, FieldSet, IdKind, Notice, NoticeKind, PartialDate, Raw, SourceRecord, WorkId,
};
use aspero_engine::{Extras, InputLookup, Verdict};
use common::*;

fn id(s: &str) -> WorkId {
    s.parse().unwrap()
}

fn dated(mut r: SourceRecord, y: i16, m: u8, d: Option<u8>, page: &str) -> SourceRecord {
    r.work.issued = PartialDate::new(y, Some(m), d);
    r.work.page = Some(page.into());
    r
}

fn corrected_by(mut r: SourceRecord, notices: &[(&str, PartialDate)]) -> SourceRecord {
    for (notice, date) in notices {
        let mut u = Notice::new(NoticeKind::Correction);
        u.notice = Some(id(notice));
        u.date = Some(*date);
        r.work.notices.push(u);
    }
    r
}

#[tokio::test]
async fn notices_are_dated_by_when_they_appeared_unless_records_conflict() {
    let date = |y, m, d| PartialDate::new(y, Some(m), d).unwrap();
    let guideline = || work("x", &["10.1234/guideline"], "Guideline", &LECUN, 2025);
    let notice = |doi| work("x", &[doi], "Correction to Guideline", &[], 2026);

    // OpenAlex answers first: the guideline, an online-first notice, and a
    // record that merged two notices (another notice's date and page).
    let openalex = Scripted::new("openalex").with(vec![
        guideline(),
        dated(notice("10.1234/fix.a"), 2026, 10, Some(12), "111313"),
        dated(notice("10.1234/fix.b"), 2025, 6, Some(19), "1541"),
    ]);
    // Crossref registered them: notices deposited with issue dates.
    let mut crossref = Scripted::new("crossref").with(vec![
        corrected_by(
            guideline(),
            &[
                ("10.1234/fix.a", date(2026, 12, Some(1))),
                ("10.1234/fix.b", date(2026, 6, Some(30))),
            ],
        ),
        dated(notice("10.1234/fix.a"), 2026, 12, None, "111313"),
        dated(notice("10.1234/fix.b"), 2026, 6, Some(30), "1823"),
    ]);
    crossref.caps.notices = true;
    let engine = engine(vec![Arc::new(openalex), Arc::new(crossref)]);

    let mut report = engine
        .lookup(
            &[id("10.1234/guideline"), id("10.1234/fix.b")],
            FieldSet::DEFAULT,
        )
        .await;
    engine.add_notices(&mut report).await;

    let updates = &report.results[0].record.as_ref().unwrap().work.notices;
    let a = updates
        .iter()
        .find(|u| u.notice == Some(id("10.1234/fix.a")))
        .unwrap();
    assert_eq!(
        a.date,
        Some(date(2026, 10, Some(12))),
        "online, not the issue"
    );
    assert_eq!(a.deposited, Some(date(2026, 12, Some(1))));
    let b = updates
        .iter()
        .find(|u| u.notice == Some(id("10.1234/fix.b")))
        .unwrap();
    assert_eq!(
        b.date,
        Some(date(2026, 6, Some(30))),
        "the merged record's date is ignored"
    );
    assert_eq!(b.deposited, None);

    let conflicts = engine.cross_check(&mut report).await;
    assert!(conflicts[0].is_empty());
    let fields: Vec<&str> = conflicts[1].iter().map(|d| d.field).collect();
    assert_eq!(fields, ["issued", "page"]);
    assert_eq!(conflicts[1][1].registry_value, "1823");
}

// Two corrections to one guideline that OpenAlex merged into one work
// (#183): its `doi` is the 2026 correction's, its date, volume, page and
// PDF the 2025 one's, and it carries both corrections' DOIs and PMIDs.
// Crossref, the DOIs' registry, keeps them apart.

const EARLIER: &str = "10.1111/1471-0528.18258";
const LATER: &str = "10.1111/1471-0528.70298";
const CORRECTION: &str = "Correction to The Management of Nausea and Vomiting in Pregnancy and \
                          Hyperemesis Gravidarum (Green-top Guideline No. 69)";
const EARLIER_PDF: &str = "https://onlinelibrary.wiley.com/doi/pdfdirect/10.1111/1471-0528.18258";

fn correction(
    source: &'static str,
    ids: &[&str],
    (y, m, d): (i16, u8, u8),
    volume: &str,
    page: &str,
) -> SourceRecord {
    let mut r = work(source, ids, CORRECTION, &[], y);
    r.work.issued = PartialDate::new(y, Some(m), Some(d));
    r.work.container_title =
        Some("BJOG: An International Journal of Obstetrics & Gynaecology".into());
    r.work.volume = Some(volume.into());
    r.work.page = Some(page.into());
    r
}

/// OpenAlex's W4411450779. Found under either DOI.
fn merged() -> SourceRecord {
    let mut r = correction(
        "openalex",
        &[
            "W4411450779",
            LATER,
            "pmid:42381193",
            "PMC13418968",
            EARLIER,
        ],
        (2025, 6, 19),
        "132",
        "1541",
    );
    let mut oa = aspero_core::OpenAccess::default();
    oa.is_oa = true;
    oa.pdf_url = Some(EARLIER_PDF.into());
    r.work.open_access = Some(oa);
    r.raw = merged_raw(&[
        &format!("https://doi.org/{LATER}"),
        &format!("https://doi.org/{EARLIER}"),
        "https://pubmed.ncbi.nlm.nih.gov/40536112",
        "https://pubmed.ncbi.nlm.nih.gov/42381193",
    ]);
    r
}

/// W4411450779's response, but for its locations: these landing pages.
fn merged_raw(locations: &[&str]) -> Raw {
    let locations: Vec<_> = locations
        .iter()
        .map(|u| serde_json::json!({"landing_page_url": u}))
        .collect();
    Raw::json(&serde_json::json!({
        "id": "https://openalex.org/W4411450779",
        "doi": format!("https://doi.org/{LATER}"),
        "ids": {"doi": format!("https://doi.org/{LATER}")},
        "locations": locations,
    }))
}

fn registered_earlier() -> SourceRecord {
    correction("crossref", &[EARLIER], (2025, 6, 19), "132", "1541")
}

fn registered_later() -> SourceRecord {
    correction("crossref", &[LATER], (2026, 6, 30), "133", "1823-1823")
}

/// OpenAlex (keyword search) first, then Crossref, the registry, whose
/// matcher returns `hits`.
fn corrections(hits: Vec<SourceRecord>) -> aspero_engine::Engine {
    let openalex = Scripted::new("openalex").keyword().with(vec![merged()]);
    let mut crossref = Scripted::new("crossref")
        .matcher(hits)
        .with(vec![registered_earlier(), registered_later()]);
    crossref.caps.notices = true;
    engine(vec![Arc::new(openalex), Arc::new(crossref)])
}

fn foreign(ids: &[WorkId], other_doi: &str) -> Vec<WorkId> {
    ids.iter()
        .filter(|i| **i == id(other_doi) || matches!(i.kind(), IdKind::Pmid | IdKind::Pmcid))
        .cloned()
        .collect()
}

#[tokio::test]
async fn the_registrys_record_of_each_doi_wins_over_an_aggregators_merge() {
    let engine = corrections(vec![]);
    let inputs = [EARLIER, LATER].map(String::from);
    let report = engine.lookup_inputs(&inputs, Extras::full()).await;
    let looked = |i: usize| match &report.items[i] {
        InputLookup::Looked(l) => l,
        InputLookup::Invalid(e) => panic!("{e}"),
    };

    // The 2026 correction: Crossref's record, not OpenAlex's, with none of
    // the merged work's other identifiers or its PDF (the other's).
    let later = looked(1);
    let record = later.result.record.as_ref().unwrap();
    assert_eq!(record.source.as_str(), "crossref");
    assert_eq!(record.work.issued.unwrap().to_string(), "2026-06-30");
    assert_eq!(record.work.volume.as_deref(), Some("133"));
    assert_eq!(record.work.page.as_deref(), Some("1823-1823"));
    assert_eq!(record.ids, [id(LATER)]);
    assert!(
        record
            .work
            .open_access
            .as_ref()
            .is_none_or(|oa| oa.pdf_url.as_deref() != Some(EARLIER_PDF)),
        "{:?}",
        record.work.open_access
    );
    // The contradiction is still reported.
    let fields: Vec<&str> = later.disagreements.iter().map(|d| d.field).collect();
    assert_eq!(fields, ["issued", "volume", "page"]);
    assert_eq!(later.disagreements[0].value, "2025-06-19");
    assert_eq!(later.disagreements[0].registry_value, "2026-06-30");

    // The 2025 correction: OpenAlex's record agrees with its registry
    // record, but no longer carries the other correction's DOI, nor PMIDs
    // the registry doesn't confirm.
    let earlier = looked(0);
    let record = earlier.result.record.as_ref().unwrap();
    assert_eq!(record.source.as_str(), "openalex");
    assert_eq!(record.work.issued.unwrap().to_string(), "2025-06-19");
    assert_eq!(record.work.volume.as_deref(), Some("132"));
    assert_eq!(record.work.page.as_deref(), Some("1541"));
    assert!(record.ids.contains(&id(EARLIER)));
    assert_eq!(foreign(&record.ids, LATER), [], "{:?}", record.ids);
    assert!(
        earlier.disagreements.is_empty(),
        "{:?}",
        earlier.disagreements
    );
}

#[tokio::test]
async fn a_plain_lookup_checks_a_record_that_looks_merged_with_the_registry() {
    // A plain lookup asks the registry only where OpenAlex's record shows
    // signs of a merge (#183); W4411450779 shows three.
    let looked = |report: &aspero_engine::InputsReport, i: usize| match &report.items[i] {
        InputLookup::Looked(l) => l.clone(),
        InputLookup::Invalid(e) => panic!("{e}"),
    };
    let both = corrections(vec![]);
    let inputs = [EARLIER, LATER].map(String::from);
    let report = both.lookup_inputs(&inputs, Extras::default()).await;

    // The 2026 correction: Crossref's record, the contradiction reported.
    let later = looked(&report, 1);
    let record = later.result.record.as_ref().unwrap();
    assert_eq!(record.source.as_str(), "crossref");
    assert_eq!(record.work.issued.unwrap().to_string(), "2026-06-30");
    assert_eq!(record.work.volume.as_deref(), Some("133"));
    assert_eq!(record.work.page.as_deref(), Some("1823-1823"));
    assert_eq!(record.ids, [id(LATER)]);
    let fields: Vec<&str> = later.disagreements.iter().map(|d| d.field).collect();
    assert_eq!(fields, ["issued", "volume", "page"]);

    // The 2025 correction (found by a DOI not the record's own): its own
    // date, volume and page, without the other's DOI and PMIDs.
    let earlier = looked(&report, 0);
    let record = earlier.result.record.as_ref().unwrap();
    assert_eq!(record.work.issued.unwrap().to_string(), "2025-06-19");
    assert_eq!(record.work.volume.as_deref(), Some("132"));
    assert_eq!(record.work.page.as_deref(), Some("1541"));
    assert!(record.ids.contains(&id(EARLIER)));
    assert_eq!(foreign(&record.ids, LATER), [], "{:?}", record.ids);
    assert!(earlier.disagreements.is_empty());

    // Each sign alone does it: two PubMed records, or another DOI of the
    // publisher's.
    for locations in [
        [
            "https://pubmed.ncbi.nlm.nih.gov/40536112",
            "https://pubmed.ncbi.nlm.nih.gov/42381193",
        ],
        [
            &format!("https://doi.org/{LATER}"),
            &format!("https://doi.org/{EARLIER}"),
        ],
    ] {
        let mut record = merged();
        record.raw = merged_raw(&locations);
        let openalex = Scripted::new("openalex").with(vec![record]);
        let mut crossref = Scripted::new("crossref").with(vec![registered_later()]);
        crossref.caps.notices = true;
        let engine = engine(vec![Arc::new(openalex), Arc::new(crossref)]);
        let report = engine
            .lookup_inputs(&[LATER.to_owned()], Extras::default())
            .await;
        let served = looked(&report, 0).result.record.unwrap();
        assert_eq!(served.source.as_str(), "crossref", "{locations:?}");
        assert_eq!(served.work.volume.as_deref(), Some("133"), "{locations:?}");
    }
}

#[tokio::test]
async fn a_plain_lookup_of_a_preprint_merged_record_asks_the_registry_nothing() {
    // OpenAlex's usual merge: the journal's article with its bioRxiv
    // preprint, one PubMed record. A registry record contradicting it (as
    // the preprint's date and no volume or page might) isn't asked for
    // without `full`.
    let doi = "10.3389/fpls.2023.1185616";
    let preprint = "10.1101/2022.12.11.519980";
    let record = |source, y, volume: &str, page: &str| {
        let mut r = work(source, &["W4379468661", doi, preprint], "Defence", &[], y);
        r.work.volume = Some(volume.into());
        r.work.page = Some(page.into());
        r
    };
    let mut aggregated = record("openalex", 2023, "14", "1185616");
    aggregated.raw = Raw::json(&serde_json::json!({
        "doi": format!("https://doi.org/{doi}"),
        "locations": [
            {"landing_page_url": format!("https://doi.org/{doi}")},
            {"landing_page_url": format!("https://doi.org/{preprint}")},
            {"landing_page_url": "https://www.biorxiv.org/content/10.1101/2022.12.11.519980"},
            {"landing_page_url": "https://pubmed.ncbi.nlm.nih.gov/37332715"},
        ],
    }));
    let openalex = Scripted::new("openalex").with(vec![aggregated]);
    let mut crossref = Scripted::new("crossref").with(vec![record("crossref", 2022, "1", "1")]);
    crossref.caps.notices = true;
    let engine = engine(vec![Arc::new(openalex), Arc::new(crossref)]);

    for asked in [doi, preprint] {
        let report = engine
            .lookup_inputs(&[asked.to_owned()], Extras::default())
            .await;
        let InputLookup::Looked(l) = &report.items[0] else {
            panic!()
        };
        let served = l.result.record.as_ref().unwrap();
        assert_eq!(served.source.as_str(), "openalex", "{asked}");
        assert!(l.disagreements.is_empty(), "{asked}: {:?}", l.disagreements);
    }
    // A full lookup asks, and the contradiction shows.
    let report = engine
        .lookup_inputs(&[doi.to_owned()], Extras::full())
        .await;
    let InputLookup::Looked(l) = &report.items[0] else {
        panic!()
    };
    assert_eq!(
        l.result.record.as_ref().unwrap().source.as_str(),
        "crossref"
    );
}

#[tokio::test]
async fn a_verify_of_each_correction_finds_its_own_record() {
    let later = Claim::from_text(format!("{CORRECTION}. BJOG. 2026;133:1823. doi:{LATER}"));
    let earlier = Claim::from_text(format!("{CORRECTION}. BJOG. 2025;132:1541. doi:{EARLIER}"));
    // The matcher returns both corrections (their titles are the same), in
    // either order, or only the one cited.
    let hit_sets = [
        vec![registered_later(), registered_earlier()],
        vec![registered_earlier(), registered_later()],
    ];
    for hits in hit_sets {
        let engine = corrections(hits);
        for (claim, doi, other, (year, volume, page)) in [
            (&later, LATER, EARLIER, (2026, "133", "1823")),
            (&earlier, EARLIER, LATER, (2025, "132", "1541")),
        ] {
            let v = engine.verify(claim, 5).await;
            let best = &v.report.candidates[v.best_match.unwrap()];
            let w = &best.record.work;
            assert_eq!(v.verdict, Verdict::Verified, "{doi}: {}", v.reason_text);
            assert_eq!(w.issued.unwrap().year(), year, "{doi}");
            assert_eq!(w.volume.as_deref(), Some(volume), "{doi}");
            assert_eq!(
                aspero_core::publication::first_page(w.page.as_deref().unwrap()),
                page,
                "{doi}"
            );
            assert!(best.ids.contains(&id(doi)), "{doi}: {:?}", best.ids);
            // A verify stops once the lookup alone verifies (the 2025
            // correction's, on OpenAlex's record), before the registry
            // answers; resolve gathers every candidate.
            let r = engine.resolve(claim, 5).await;
            let best = &r.candidates[0];
            assert!(best.ids.contains(&id(doi)), "{doi}: {:?}", best.ids);
            assert_eq!(best.record.work.volume.as_deref(), Some(volume), "{doi}");
            assert_eq!(foreign(&best.ids, other), [], "{doi}: {:?}", best.ids);
        }
    }

    // Only the cited one.
    let engine = corrections(vec![registered_later()]);
    let v = engine.verify(&later, 5).await;
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(best.record.work.issued.unwrap().year(), 2026);
    assert_eq!(best.record.work.volume.as_deref(), Some("133"));
}

#[tokio::test]
async fn an_aggregator_record_agreeing_with_the_registry_merges_as_before() {
    // Online first in 2020 (OpenAlex's date), in the 2021 issue (Crossref's,
    // which knows both years, or only the issue's: one field differing
    // isn't a contradiction).
    let doi = "10.1016/j.envint.2020.106274";
    let record = |source, (y, m)| {
        let mut r = work(source, &[doi], "Plasticenta", &["Antonio Ragusa"], y);
        r.work.issued = PartialDate::new(y, Some(m), None);
        r.work.volume = Some("146".into());
        r.work.page = Some("106274".into());
        r
    };
    for other_years in [vec![2020], vec![]] {
        let mut registered = record("crossref", (2021, 1));
        registered.work.other_years = other_years.clone();
        let openalex = Scripted::new("openalex")
            .keyword()
            .with(vec![record("openalex", (2020, 12))]);
        let mut crossref = Scripted::new("crossref")
            .matcher(vec![registered.clone()])
            .with(vec![registered]);
        crossref.caps.notices = true;
        let engine = engine(vec![Arc::new(openalex), Arc::new(crossref)]);

        let claim = Claim::from_text(format!(
            "Ragusa A. Plasticenta. Environ Int. 2021;146:106274. doi:{doi}"
        ));
        let v = engine.resolve(&claim, 5).await;
        let [c] = &v.candidates[..] else {
            panic!("one work: {:?} ({other_years:?})", v.candidates.len());
        };
        let mut sources: Vec<&str> = c.sightings.iter().map(|s| s.provider.as_str()).collect();
        sources.dedup();
        assert_eq!(sources, ["openalex", "crossref", "openalex"]);
        assert_eq!(c.record.work.issued.unwrap().to_string(), "2020-12");

        let report = engine
            .lookup_inputs(&[doi.to_owned()], Extras::full())
            .await;
        let InputLookup::Looked(l) = &report.items[0] else {
            panic!()
        };
        let served = l.result.record.as_ref().unwrap();
        assert_eq!(served.source.as_str(), "openalex", "{other_years:?}");
        let fields: Vec<&str> = l.disagreements.iter().map(|d| d.field).collect();
        let expected: &[&str] = if other_years.is_empty() {
            &["issued"]
        } else {
            &[]
        };
        assert_eq!(fields, expected);
    }
}

#[tokio::test]
async fn another_providers_record_contradicting_the_registry_merges_as_before() {
    // wild-4 #131: the registry has a reprint's place (BJSM 2015;49:1348)
    // under the original's DOI, as does OpenAlex; Europe PMC has the
    // original's (BMJ 2013;347:f6234), as cited. Either may be wrong, so
    // the citation still verifies against Europe PMC's.
    let doi = "10.1136/bmj.f6234";
    let title = "The effect of fall prevention exercise programmes on fall induced injuries \
                 in community dwelling older adults";
    let authors = [
        "Fabienne El-Khoury",
        "Bernard Cassou",
        "Patricia Dargent-Molina",
    ];
    let record = |source, ids: &[&str], (y, m), journal: &str, volume: &str, page: &str| {
        let mut r = work(source, ids, title, &authors, y);
        r.work.issued = PartialDate::new(y, Some(m), None);
        r.work.container_title = Some(journal.into());
        r.work.volume = Some(volume.into());
        r.work.page = Some(page.into());
        r
    };
    let reprint = |source| record(source, &[doi], (2015, 10), "Br J Sports Med", "49", "1348");
    let original = record(
        "europepmc",
        &[doi, "pmid:24169944"],
        (2013, 10),
        "BMJ",
        "347",
        "f6234",
    );
    let openalex = Scripted::new("openalex").with(vec![reprint("openalex")]);
    let mut crossref = Scripted::new("crossref")
        .matcher(vec![reprint("crossref")])
        .with(vec![reprint("crossref")]);
    crossref.caps.notices = true;
    let europepmc = Scripted::new("europepmc").keyword().with(vec![original]);
    let engine = engine(vec![
        Arc::new(openalex),
        Arc::new(crossref),
        Arc::new(europepmc),
    ]);

    let claim = Claim::from_text(format!(
        "El-Khoury F, Cassou B, Dargent-Molina P. {title}. BMJ. 2013;347:f6234. doi:{doi}"
    ));
    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(best.record.source.as_str(), "europepmc");
    assert!(best.ids.contains(&id(doi)) && best.ids.contains(&id("pmid:24169944")));
}
