//! Red-team wild-4: floors and caps for model-written references whose
//! identifier names the work while the words around it don't spell its
//! title (a label, a short form, a title cut with "…").

use super::*;

/// Roller et al., "Hash Layers For Large Sparse Models", as its arXiv
/// record has it.
fn hash_layers() -> SourceRecord {
    arxiv_preprint(
        "arXiv:2106.04426",
        "Hash Layers For Large Sparse Models",
        &["Stephen Roller", "Sainbayar Sukhbaatar", "Arthur Szlam"],
        2021,
    )
}

/// wild-4 #467: a short form of the title, the arXiv id and the authors.
#[test]
fn an_identified_work_cited_by_a_short_form_of_its_title_is_plausible() {
    let claim = Claim::from_text("Hash Layers (Roller et al.) | 2106.04426 | NeurIPS 2021");
    let a = assess(&claim, &hash_layers());
    assert_eq!(a.ids, Agreement::Match);
    assert_eq!(a.title.agreement, Agreement::Mismatch, "{a:?}");
    assert!(
        (PLAUSIBLE..VERIFIED).contains(&a.confidence),
        "{}",
        a.confidence
    );
}

/// wild-4 #100: a trial's label where the title would stand, with the
/// DOI, the authors and the place.
#[test]
fn a_trial_label_beside_the_works_doi_and_authors_is_plausible() {
    let mut r = placed(
        &["10.1056/nejmoa2213836"],
        "Efficacy and Safety of a Bivalent RSV Prefusion F Vaccine in Older Adults",
        &["Edward E. Walsh", "Gonzalo Pérez Marc", "Anna M. Zareba"],
        2023,
        ("New England Journal of Medicine", "388", "1465-1477"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = Claim::from_text(
        "AReSVi-006 (GSK, RSVPreF3 OA): Walsh EE et al. N Engl J Med 2023;388:1465–1477. \
         DOI: 10.1056/NEJMoa2213836 (I am fairly confident of this one).",
    );
    let a = assess(&claim, &r);
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
    assert!(a.confidence < VERIFIED, "{a:?}");
}

/// The floor takes the authors: a claimed id another paper's, beside other
/// people and another title, stays a mismatch.
#[test]
fn an_id_beside_other_authors_and_another_title_is_not_lifted() {
    let claim = Claim::from_text(
        "Smith J, Jones K. A survey of routing in sparse models. arXiv:2106.04426 (2021)",
    );
    let a = assess(&claim, &hash_layers());
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

/// Nor does it lift a title given as a field: the title cited is another.
#[test]
fn an_id_beside_a_given_title_that_differs_is_not_lifted() {
    let claim = Claim {
        title: Some("Mixture of experts routing with learned gates".into()),
        authors: vec!["Roller, S.".into()],
        year: Some(2021),
        ids: vec!["arXiv:2106.04426".parse().unwrap()],
        ..Claim::default()
    };
    let a = assess(&claim, &hash_layers());
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

/// holdout-6 #802: another paper's DOI, its first author's surname among
/// the cited ones by chance, at another volume and page: not lifted.
#[test]
fn an_id_beside_a_namesake_at_another_place_is_not_lifted() {
    let mut r = placed(
        &["10.1371/journal.pone.0138089"],
        "The Potential for Mindfulness-Based Intervention in Workplace Mental Health \
         Promotion: Results of a Randomized Controlled Trial",
        &[
            "Shu-Ling Huang",
            "Ren-Hau Li",
            "Feng-Ying Huang",
            "Feng-Cheng Tang",
        ],
        2015,
        ("PLoS ONE", "10", "e0138089"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let claim = Claim::from_text(
        "Wang TY, Lin SM, Shie SS, Chou PC, Huang CD, Chung FT, et al. Clinical \
         characteristics and treatment outcomes of patients with low- and high-concentration \
         isoniazid-monoresistant tuberculosis. PLoS One. 2014;9(1):e86316. \
         doi: 10.1371/journal.pone.0138089",
    );
    let a = assess(&claim, &r);
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

/// CMS's tt¯H observation, as OpenAlex has it: PRL 120, 231801.
fn tth() -> SourceRecord {
    let mut r = placed(
        &["10.1103/physrevlett.120.231801", "arXiv:1804.02610"],
        "Observation of tt¯H Production",
        &["A. M. Sirunyan", "A. Tumasyan", "W. Adam"],
        2018,
        ("Physical Review Letters", "120", "231801"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    r
}

/// wild-4 #367: the DOI and the exact place agree, the title's notation
/// doesn't ("ttH" for "tt¯H"), and the table's label is no byline of it.
#[test]
fn an_identifier_and_the_exact_place_that_agree_are_plausible() {
    let claim = Claim::from_text(
        "CMS ttH observation | Observation of ttH production | PRL 120, 231801 (2018) | \
         10.1103/PhysRevLett.120.231801 | 1804.02610",
    );
    let a = assess(&claim, &tth());
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
}

/// Not when the title and the byline both differ: another paper's, cited
/// at this one's place.
#[test]
fn an_identifier_and_place_beside_another_title_and_authors_are_not_lifted() {
    let claim = Claim::from_text(
        "Smith J, Jones K. Measurement of the top quark mass in dilepton events. \
         Phys Rev Lett 120, 231801 (2018). doi:10.1103/PhysRevLett.120.231801",
    );
    let a = assess(&claim, &tth());
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

fn moe_mamba() -> SourceRecord {
    arxiv_preprint(
        "arXiv:2401.04081",
        "MoE-Mamba: Efficient Selective State Space Models with Mixture of Experts",
        &["Maciej Pióro", "Kamil Ciebiera", "Krystian Król"],
        2024,
    )
}

/// wild-4 #484 and others: the title and the work's own identifier, nothing
/// else cited. The identifier counts as the second field.
#[test]
fn a_title_beside_its_works_identifier_verifies() {
    let claim = Claim::from_text(
        "MoE-Mamba: Efficient Selective State Space Models with Mixture of Experts. \
         arXiv:2401.04081",
    );
    let a = assess(&claim, &moe_mamba());
    assert_eq!(a.ids, Agreement::Match);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

/// The title alone, without an identifier, is still one field: reviews and
/// corrections carry it too.
#[test]
fn a_title_alone_is_still_one_field() {
    let claim = Claim::from_text(
        "MoE-Mamba: Efficient Selective State Space Models with Mixture of Experts",
    );
    let a = assess(&claim, &moe_mamba());
    assert!(a.confidence <= SINGLE_FIELD_CAP, "{a:?}");
}

/// Nor does the identifier lift a title that only partly agrees.
#[test]
fn a_partly_agreeing_title_beside_the_identifier_stays_capped() {
    let claim =
        Claim::from_text("MoE-Mamba: Efficient Selective State Space Models. arXiv:2401.04081");
    let a = assess(&claim, &moe_mamba());
    assert!(a.confidence < VERIFIED, "{a:?}");
}
