//! `verify` of a claim citing two identifiers (a DOI and a PMID) that
//! name two works (#148): wild-3's model-written references.

mod common;

use std::sync::Arc;

use aspero_core::{Claim, IdKind, SourceRecord};
use aspero_engine::{CitedField, Engine, Reason, Verdict};
use common::*;

/// One source that looks up DOIs and PMIDs.
fn engine_with(works: Vec<SourceRecord>) -> Engine {
    let mut source = Scripted::new("openalex").keyword().with(works);
    source.caps.lookup_kinds.push(IdKind::Pmid);
    engine(vec![Arc::new(source)])
}

fn in_journal(mut r: SourceRecord, journal: &str, volume: &str, page: &str) -> SourceRecord {
    r.work.container_title = Some(journal.into());
    r.work.volume = Some(volume.into());
    r.work.page = Some(page.into());
    r
}

const BURTIN_TITLE: &str =
    "Early exercise in critically ill patients enhances short-term functional recovery";

fn burtin(ids: &[&str]) -> SourceRecord {
    in_journal(
        work(
            "openalex",
            ids,
            BURTIN_TITLE,
            &["Chris Burtin", "Beatrix Clerckx", "Christophe Robbeets"],
            2009,
        ),
        "Critical Care Medicine",
        "37",
        "2499-2505",
    )
}

/// The paper PMID 19623055 is.
fn bilotta() -> SourceRecord {
    in_journal(
        work(
            "openalex",
            &["10.1097/aco.0b013e3283302339", "pmid:19623055"],
            "'Anesthesia' for awake neurosurgery",
            &["Federico Bilotta"],
            2009,
        ),
        "Current Opinion in Anaesthesiology",
        "22",
        "560-565",
    )
}

fn burtin_citing(doi: &str, pmid: &str) -> Claim {
    Claim::from_text(format!(
        "Burtin C, Clerckx B, Robbeets C, et al. Early exercise in critically ill patients \
         enhances short-term functional recovery. Crit Care Med. 2009;37(9):2499–2505. \
         doi:{doi}. PMID: {pmid}."
    ))
}

fn identifier(reason: &Reason) -> (Option<String>, Option<String>) {
    let d = reason
        .differs()
        .iter()
        .find(|d| d.field == CitedField::Identifier)
        .unwrap_or_else(|| panic!("no identifier named: {reason:?}"));
    (d.actual.clone(), d.cited.clone())
}

#[tokio::test]
async fn the_works_doi_beside_another_papers_pmid_is_plausible() {
    let engine = engine_with(vec![
        burtin(&["10.1097/ccm.0b013e3181a38937", "pmid:19623052"]),
        bilotta(),
    ]);
    let v = engine
        .verify(
            &burtin_citing("10.1097/CCM.0b013e3181a38937", "19623055"),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert!(matches!(v.reason, Reason::IdPartlyMatches { .. }));
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(best.record.work.title.as_deref(), Some(BURTIN_TITLE));
    assert_eq!(
        identifier(&v.reason),
        (Some("pmid:19623052".into()), Some("pmid:19623055".into()))
    );
    assert!(
        v.reason_text.contains("identifier (pmid:19623052)"),
        "{}",
        v.reason_text
    );

    // Its own PMID: verified.
    let v = engine
        .verify(
            &burtin_citing("10.1097/CCM.0b013e3181a38937", "19623052"),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}

#[tokio::test]
async fn the_works_pmid_beside_another_papers_doi_is_plausible() {
    // wild-3 #403: the DOI cited is Bilotta's, the PMID Burtin's; Burtin's
    // record, found by its PMID, carries no PMID conflict but another DOI.
    let engine = engine_with(vec![
        burtin(&["10.1097/ccm.0b013e3181a38937", "pmid:19623052"]),
        bilotta(),
    ]);
    let v = engine
        .verify(
            &burtin_citing("10.1097/aco.0b013e3283302339", "19623052"),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert_eq!(
        identifier(&v.reason),
        (
            Some("doi:10.1097/ccm.0b013e3181a38937".into()),
            Some("doi:10.1097/aco.0b013e3283302339".into())
        )
    );
}

#[tokio::test]
async fn two_records_of_one_work_unmerged_are_not_two_works() {
    // The DOI's record without a PMID, the PMID's without the DOI.
    let engine = engine_with(vec![
        burtin(&["10.1097/ccm.0b013e3181a38937"]),
        burtin(&["pmid:19623052"]),
    ]);
    let v = engine
        .verify(
            &burtin_citing("10.1097/CCM.0b013e3181a38937", "19623052"),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}

#[tokio::test]
async fn identifiers_all_another_works_stay_a_mismatch() {
    // Both identifiers Bilotta's beside Burtin's details: none names the
    // work cited, as before.
    let engine = engine_with(vec![
        burtin(&["10.1097/ccm.0b013e3181a38937", "pmid:19623052"]),
        bilotta(),
    ]);
    let v = engine
        .verify(
            &burtin_citing("10.1097/aco.0b013e3283302339", "19623055"),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
}

const WHO_TITLE: &str = "Guideline: protecting, promoting and supporting breastfeeding in \
                         facilities providing maternity and newborn services";

/// One source that looks up ISBNs (and knows none of these) and finds
/// the guideline by its title.
fn who_engine() -> Engine {
    let guideline = work(
        "europepmc",
        &["pmid:29565522"],
        WHO_TITLE,
        &["World Health Organization"],
        2017,
    );
    let mut source = Scripted::new("openlibrary").keyword().with(vec![guideline]);
    source.caps.lookup_kinds.push(IdKind::Isbn);
    engine(vec![Arc::new(source)])
}

#[tokio::test]
async fn an_isbn_no_catalogue_knows_beside_the_book_is_plausible() {
    // wild-6 #207, minimised: a real guideline, its ISBN missing from the
    // catalogue looked in.
    let v = who_engine()
        .verify(
            &Claim::from_text(format!(
                "World Health Organization. {WHO_TITLE}. Geneva: World Health Organization; \
                 2017. ISBN 978-92-4-155008-6."
            )),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert_eq!(
        identifier(&v.reason),
        (None, Some("isbn:9789241550086".into()))
    );
    assert!(
        !v.reason_text.contains("does not exist"),
        "{}",
        v.reason_text
    );
}

#[tokio::test]
async fn an_unknown_isbn_beside_an_invented_book_is_still_not_found() {
    let v = who_engine()
        .verify(
            &Claim::from_text(
                "Quillfeather-Ondrak P. Lanterns of the inland sea. Harbourline; 2017. \
                 ISBN 978-92-4-155008-6.",
            ),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::NotFound, "{}", v.reason_text);
}
