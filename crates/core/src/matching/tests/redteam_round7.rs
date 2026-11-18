//! Red-team round 7 (#148): wild-3's model-written references citing a
//! work's DOI beside another paper's PMID, in prose and in Markdown tables.

use super::*;
use crate::id::IdKind;

/// Burtin et al. 2009, as OpenAlex has it: its DOI and its PMID.
fn burtin(pmid: Option<&str>) -> SourceRecord {
    let mut ids = vec!["doi:10.1097/ccm.0b013e3181a38937".to_owned()];
    ids.extend(pmid.map(|p| format!("pmid:{p}")));
    let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
    let mut r = placed(
        &ids,
        "Early exercise in critically ill patients enhances short-term functional recovery",
        &["Chris Burtin", "Beatrix Clerckx", "Christophe Robbeets"],
        2009,
        ("Critical Care Medicine", "37", "2499-2505"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

/// wild-3 #2: the paper's DOI, then a PMID three off (Bilotta et al.'s).
const BURTIN: &str = "Burtin C, Clerckx B, Robbeets C, et al. Early exercise in critically \
    ill patients enhances short-term functional recovery. Crit Care Med. \
    2009;37(9):2499–2505. doi:10.1097/CCM.0b013e3181a38937. PMID: 19623055.";

#[test]
fn a_cited_pmid_another_papers_beside_the_works_doi_is_plausible() {
    let claim = Claim::from_text(BURTIN);
    assert_eq!(claim.ids.len(), 2, "{:?}", claim.ids);
    let a = assess(&claim, &burtin(Some("19623052")));
    assert_eq!(a.ids, Agreement::Partial);
    assert!(
        (PLAUSIBLE..VERIFIED).contains(&a.confidence),
        "{}",
        a.confidence
    );
    let record = burtin(Some("19623052"));
    let (cited, actual) = conflicting_id(&claim.ids, &record.ids).unwrap();
    assert_eq!(
        (cited.to_string(), actual.to_string()),
        ("pmid:19623055".into(), "pmid:19623052".into())
    );
    // Its own PMID cited: verified, as before.
    let right = Claim::from_text(BURTIN.replace("19623055", "19623052"));
    let a = assess(&right, &burtin(Some("19623052")));
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence >= VERIFIED, "{}", a.confidence);
}

#[test]
fn a_cited_pmid_contradicts_nothing_on_a_record_without_one() {
    // Crossref's record carries no PMID: nothing to compare.
    let claim = Claim::from_text(BURTIN);
    let a = assess(&claim, &burtin(None));
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence >= VERIFIED, "{}", a.confidence);
}

#[test]
fn only_a_pmid_or_pmcid_is_held_to_the_works_of_its_kind() {
    let ids = |list: &[&str]| -> Vec<WorkId> { list.iter().map(|i| i.parse().unwrap()).collect() };
    let work = ids(&["doi:10.1000/a", "pmid:100", "pmcid:PMC200"]);
    for (claimed, want) in [
        // A PMCID is compared with the work's PMCID, not its PMID.
        (&["doi:10.1000/a", "pmcid:PMC201"][..], Agreement::Partial),
        (&["doi:10.1000/a", "pmcid:PMC200"], Agreement::Match),
        (&["pmid:100", "pmcid:PMC201"], Agreement::Partial),
        // A DOI may be one of several (a preprint's, a version's): not
        // held to the work's.
        (&["pmid:100", "doi:10.1000/b"], Agreement::Match),
        // Every claimed identifier another's: a different case, as before.
        (&["doi:10.1000/b", "pmid:101"], Agreement::Mismatch),
    ] {
        assert_eq!(id_agreement(&ids(claimed), &work), want, "{claimed:?}");
    }
    // A work known by two PMIDs (a source's mistake beside another's) has
    // the one cited.
    let merged = ids(&["doi:10.1000/a", "pmid:100", "pmid:101"]);
    assert_eq!(
        id_agreement(&ids(&["doi:10.1000/a", "pmid:101"]), &merged),
        Agreement::Match
    );
}

#[test]
fn a_bare_pmid_as_a_table_cell_beside_a_doi_is_one() {
    // wild-3: references as table rows, the PMID a cell of its own.
    let pmid = |text: &str| {
        Claim::from_text(text)
            .ids
            .into_iter()
            .find(|id| id.kind() == IdKind::Pmid)
            .map(|id| id.to_string())
    };
    for (text, want) in [
        (
            "Neumann JT, et al. Diagnosis of myocardial infarction using a high-sensitivity \
             troponin I 1-hour algorithm. JAMA Cardiol. 2016;1(4):397-404. | \
             10.1001/jamacardio.2016.0695 | 27438392 (verify)",
            "pmid:27438392",
        ),
        (
            "Mafi N, Lorentzon R, Alfredson H. Eccentric vs concentric calf training in chronic \
             Achilles tendinosis. Knee Surg Sports Traumatol Arthrosc. 2001;9(1) | 42-47 | \
             11269583 | 10.1007/s001670000148",
            "pmid:11269583",
        ),
        (
            "ATOM2 (0.5%, 0.1%, 0.01%; 2-year) | Chia A, et al. Ophthalmology \
             2012;119(2):347-354 | 10.1016/j.ophtha.2011.07.031 | 22063159 |",
            "pmid:22063159",
        ),
    ] {
        assert_eq!(pmid(text).as_deref(), Some(want), "{text}");
    }
}

#[test]
fn a_bare_number_elsewhere_is_no_pmid() {
    for text in [
        // In prose, beside a DOI.
        "Smith J. Trial of things. Lancet. 2016;388:1377. doi:10.1016/S0140-6736(16)31637-3. \
         We screened 27438392 records.",
        // A cell, but in a row with no DOI.
        "Neumann JT, et al. Diagnosis of myocardial infarction. JAMA Cardiol. 2016 | 27438392",
        // Part of a cell, or with more than a note.
        "Smith J. A paper. 2016 | 10.1000/xyz | PMC 27438392",
        "Smith J. A paper. 2016 | 10.1000/xyz | 27438392 participants",
        // Two such cells: which is it?
        "Smith J. A paper. 2016 | 10.1000/xyz | 27438392 | 27438393",
        // Too short, or a locator.
        "Smith J. A paper. 2016 | 10.1000/xyz | 123456",
        "Smith J. A paper. 2016;1(4):397-404 | 10.1000/xyz | 397-404",
    ] {
        let ids = Claim::from_text(text).ids;
        assert!(
            ids.iter().all(|id| id.kind() != IdKind::Pmid),
            "{text}: {ids:?}"
        );
    }
    // A PMID labelled is read as before, and a bare cell beside it isn't
    // another.
    let ids =
        Claim::from_text("Smith J. A paper. 2016 | 10.1000/xyz | PMID: 27438392 | 12345678").ids;
    let pmids: Vec<String> = ids
        .iter()
        .filter(|id| id.kind() == IdKind::Pmid)
        .map(ToString::to_string)
        .collect();
    assert_eq!(pmids, ["pmid:27438392"]);
}
