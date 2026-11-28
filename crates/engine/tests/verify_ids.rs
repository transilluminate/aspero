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

const AR6_WG1: &str = "Climate Change 2021: The Physical Science Basis";

/// A report's chapter as its publisher registered it: the print edition's
/// date, no authors.
fn ar6_chapter() -> SourceRecord {
    let mut r = work(
        "crossref",
        &["10.1017/9781009157896.011"],
        "Ocean, Cryosphere and Sea Level Change",
        &[],
        2023,
    );
    r.work.container_title = Some("Climate Change 2021 – The Physical Science Basis".into());
    r.work.page = Some("1211-1362".into());
    r
}

/// The whole report, under its editors, in the year it came out.
fn ar6_report() -> SourceRecord {
    work(
        "openalex",
        &["openalex:W7139306789"],
        &format!(
            "IPCC, 2021: {AR6_WG1} - Contribution of Working Group I to the Sixth \
             Assessment Report of the Intergovernmental Panel on Climate Change"
        ),
        &["Valérie Masson-Delmotte", "P. Zhai", "Anna Pirani"],
        2021,
    )
}

/// The chapter's record by its DOI; a citation matcher finding the report.
fn ar6_engine(chapter: SourceRecord) -> Engine {
    let lookup = Scripted::new("crossref").keyword().with(vec![chapter]);
    let matcher = Scripted::new("openalex").matcher(vec![ar6_report()]);
    engine(vec![Arc::new(lookup), Arc::new(matcher)])
}

#[tokio::test]
async fn a_chapter_by_its_doi_is_not_its_books_mismatch() {
    // wild-6 #597, minimised: the cited year is the report's, which the
    // report's record matches.
    let v = ar6_engine(ar6_chapter())
        .verify(
            &Claim::from_text(format!(
                "Fox-Kemper, B., H.T. Hewitt, C. Xiao, G. Aðalgeirsdóttir, S.S. Drijfhout, \
                 T.L. Edwards, N.R. Golledge, M. Hemer, R.E. Kopp, G. Krinner, A. Mix, D. Notz, \
                 S. Nowicki, I.S. Nurhati, L. Ruiz, J.-B. Sallée, A.B.A. Slangen, and Y. Yu, \
                 2021: Ocean, Cryosphere and Sea Level Change. In: {AR6_WG1}. Contribution of \
                 Working Group I to the Sixth Assessment Report of the Intergovernmental Panel \
                 on Climate Change [Masson-Delmotte, V., et al. (eds.)]. Cambridge University \
                 Press, Cambridge, UK and New York, NY, USA, pp. 1211–1362. \
                 doi:10.1017/9781009157896.011"
            )),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert_eq!(
        best.ids.first().map(ToString::to_string).as_deref(),
        Some("doi:10.1017/9781009157896.011")
    );
    assert!(
        v.reason
            .differs()
            .iter()
            .any(|d| d.field == CitedField::Year),
        "{:?}",
        v.reason
    );
    assert!(
        v.report.candidates.iter().any(|c| c
            .ids
            .iter()
            .any(|id| id.to_string() == "openalex:W7139306789")
            && c.assessment.confidence >= aspero_core::matching::PLAUSIBLE),
        "the report is a candidate: {:?}",
        v.report.candidates
    );
}

#[tokio::test]
async fn another_works_doi_beside_a_books_chapter_stays_a_mismatch() {
    // The DOI's record is not the chapter cited: the report it would be in
    // is no excuse.
    let mut other = ar6_chapter();
    other.work.title = Some("Changing State of the Climate System".into());
    let v = ar6_engine(other)
        .verify(
            &Claim::from_text(format!(
                "Fox-Kemper B, et al. Ocean, Cryosphere and Sea Level Change. In: {AR6_WG1}. \
                 Cambridge University Press; 2021. pp. 1211–1362. \
                 doi:10.1017/9781009157896.011"
            )),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason_text);
}

#[tokio::test]
async fn the_ids_work_without_authors_to_compare_and_another_year_is_plausible() {
    // wild-3 #931, minimised: an arXiv ID's work, dated by its journal
    // version, cited by its title and preprint year.
    let mut source = Scripted::new("openalex").keyword().with(vec![work(
        "openalex",
        &["arxiv:2309.07864"],
        "The rise and potential of large language model based agents: a survey",
        &["Zhiheng Xi"],
        2025,
    )]);
    source.caps.lookup_kinds.push(IdKind::Arxiv);
    let v = engine(vec![Arc::new(source)])
        .verify(
            &Claim::from_text(
                "The rise and potential of large language model based agents: a survey                  (2023). arXiv:2309.07864",
            ),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert!(
        v.reason
            .differs()
            .iter()
            .any(|d| d.field == CitedField::Year),
        "{:?}",
        v.reason
    );
}
