//! Other versions of a work: editions, preprints and their published
//! versions, reprints, and replies, corrections and notices.

use super::*;

#[test]
fn replies_and_corrections_are_not_the_original() {
    assert!(is_derivative(
        "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply"
    ));
    assert!(is_derivative("Correction to: Attention is all you need"));
    assert!(is_derivative(
        "Comment on “Fibrinolysis for patients with intermediate-risk PE”"
    ));
    assert!(!is_derivative(
        "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism"
    ));
    assert!(is_derivative("Correction: Deep learning"));
    assert!(is_derivative("In response to: Fibrinolysis for PE"));
    // The words inside a title are not markers.
    for title in [
        "Community-acquired pneumonia subgroups and differential response to corticosteroids",
        "Correction of hyponatraemia: a systematic review",
        "Rapid correction of acidosis",
    ] {
        assert!(!is_derivative(title), "{title}");
    }

    let claim = Claim {
        title: Some("Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism".into()),
        authors: vec!["Righini".into()],
        year: Some(2014),
        ..Claim::default()
    };
    let reply = assess(
        &claim,
        &record(
            &[],
            "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply",
            &["Marc Righini"],
            2014,
        ),
    );
    assert!(reply.confidence < PLAUSIBLE, "{reply:?}");
    // Citing the reply itself is fine.
    let citing_reply = Claim {
        title: Some("Age-adjusted D-dimer cutoff levels and pulmonary embolism—reply".into()),
        ..claim
    };
    let a = assess(
        &citing_reply,
        &record(
            &[],
            "Age-Adjusted D-Dimer Cutoff Levels and Pulmonary Embolism—Reply",
            &["Marc Righini"],
            2014,
        ),
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // Nor is the original a correction cited (holdout-1 #185, corpus
    // #105): the correction's record may be missing, but the original
    // isn't verified in its place, its title and place quoted inside.
    let cloning = placed(
        &[],
        "Cloning of a cGMP-gated cation channel from mouse kidney inner medullary collecting \
         duct",
        &["K H Karlson", "F Ciampolillo-Bates", "D E McCoy"],
        1995,
        ("Biochimica et Biophysica Acta", "1236", "197-200"),
    );
    let corrigendum = "Karlson KH, Ciampolillo-Bates F, McCoy DE. Corrigendum to \"Cloning of a \
                       cGMP-gated cation channel from mouse kidney inner medullary collecting \
                       duct\" [Biochim. Biophys. Acta 1236 (1995) 197-200]. Biochim Biophys Acta. \
                       1995;1238(2):197.";
    let a = assess(&Claim::from_text(corrigendum), &cloning);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    let shamloul = placed(
        &[],
        "Complementary and Alternative Medicine (CAM) for Sexual Dysfunction",
        &["Rany Shamloul", "Anthony J. Bella"],
        2014,
        ("The Journal of Sexual Medicine", "11", "1097-1098"),
    );
    let corrections = "Corrections to “Shamloul R, Bella AJ. Complementary and alternative \
                       medicine (CAM) for sexual dysfunction. J Sex Med 2014;11:1097–8”. J Sexual \
                       Med. 2014;11:2126.";
    let a = assess(&Claim::from_text(corrections), &shamloul);
    assert!(a.confidence < VERIFIED, "{a:?}");
    // The correction's own record verifies.
    let mut notice = shamloul.clone();
    notice.work.title = Some(
        "Corrections to “Complementary and Alternative Medicine (CAM) for Sexual Dysfunction”"
            .into(),
    );
    notice.work.page = None;
    let a = assess(&Claim::from_text(corrections), &notice);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn another_edition_of_a_cited_book_is_plausible() {
    let mut book = record(
        &["isbn:9781880696514"],
        "Advanced Trauma Life Support®",
        &["American College of Surgeons Committee on Trauma"],
        2025,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    let a = assess(&Claim::from_text(ATLS), &book);
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
    assert_eq!(a.year.agreement, Agreement::Mismatch);

    // Not for articles: a different year there is more often another work.
    book.work.work_type = Some(crate::work::WorkType::JournalArticle);
    assert!(assess(&Claim::from_text(ATLS), &book).confidence < PLAUSIBLE);

    // Nor lowered again after: a structured claim giving the publisher
    // as its container, for a book in a series.
    let mut book = record(
        &[],
        "Reinforcement Learning: An Introduction",
        &["Richard S. Sutton", "Andrew G. Barto"],
        1998,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    book.work.container_title = Some("Adaptive Computation and Machine Learning".into());
    let claim = Claim {
        title: Some("Reinforcement Learning: An Introduction".into()),
        authors: vec!["Sutton RS".into(), "Barto AG".into()],
        year: Some(2018),
        container: Some("MIT Press".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &book);
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
}

#[test]
fn a_chapter_is_not_another_edition_of_its_book() {
    // The book's editor is not the chapter's author.
    let mut book = record(&[], "Handbook of Sepsis", &["K. Jones"], 2018);
    book.work.work_type = Some(crate::work::WorkType::Book);
    for text in [
        "Smith J. Fluids in shock. In: Jones K, ed. Handbook of Sepsis. Springer; 2010:12-30.",
        "Smith, J. (2010). Fluids in shock. In K. Jones (Ed.), Handbook of sepsis (pp. 12-30). Springer.",
    ] {
        let a = assess(&Claim::from_text(text), &book);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
}

#[test]
fn a_preprint_and_its_published_version_are_one_work() {
    // A shared arXiv ID: the published record lists its arXiv copy.
    let preprint = attention(
        &["W1", "arXiv:1706.03762", "10.48550/arXiv.1706.03762"],
        2017,
    );
    let published = attention(&["W2", "10.5555/neurips.2017", "arXiv:1706.03762"], 2017);
    assert!(same_work(&preprint, &published));

    // No shared identifier: the same title, first author and near years.
    let biorxiv = record(
        &["W3", "10.1101/2020.01.01.1"],
        "Sepsis in mice",
        &["Jane Doe"],
        2020,
    );
    let journal = record(
        &["W4", "10.1038/x.2021"],
        "Sepsis in Mice",
        &["J. Doe", "A. N. Other"],
        2021,
    );
    assert!(same_work(&biorxiv, &journal));

    // Years two apart, another first author, or a reply: not the work.
    let later = record(&["W5"], "Sepsis in mice", &["Jane Doe"], 2022);
    assert!(!same_work(&biorxiv, &later));
    let other = record(&["W6"], "Sepsis in mice", &["John Smith"], 2020);
    assert!(!same_work(&biorxiv, &other));
    let reply = record(&["W7"], "Reply to: Sepsis in mice", &["Jane Doe"], 2020);
    assert!(!same_work(&biorxiv, &reply));
}

#[test]
fn a_preprint_and_a_journal_article_are_other_versions_of_each_other() {
    // Red team (#102): an arXiv preprint cited in a journal by name alone,
    // and a journal's article cited as a preprint, verified.
    let mut preprint = record(
        &["arxiv:2410.02479"],
        "Cross-Embodiment Dexterous Grasping with Reinforcement Learning",
        &["Haoqi Yuan", "Bohan Zhou"],
        2024,
    );
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    let text = "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. \
                Science Robotics. 2024.";
    let a = assess(&Claim::from_text(text), &preprint);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Cited as the preprint it is, or by its arXiv identifier beside a
    // venue (as computing and physics cite): verified.
    for text in [
        "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. arXiv. \
         2024.",
        "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. Science \
         Robotics. 2024. arXiv:2410.02479",
    ] {
        let a = assess(&Claim::from_text(text), &preprint);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // A preprint server's DOI with a journal's name names two versions
    // (round 5): plausible.
    let mut biorxiv = record(
        &["doi:10.1101/2024.01.02.573000"],
        "Cross-Embodiment Dexterous Grasping with Reinforcement Learning",
        &["Haoqi Yuan", "Bohan Zhou"],
        2024,
    );
    biorxiv.work.work_type = Some(crate::work::WorkType::Preprint);
    let text = "Yuan H, Zhou B. Cross-embodiment dexterous grasping with reinforcement learning. \
                Science Robotics. 2024. doi:10.1101/2024.01.02.573000";
    let a = assess(&Claim::from_text(text), &biorxiv);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    for text in [
        format!("Rivers E, Nguyen B. {EGDT}. medRxiv preprint, 2001."),
        format!("Rivers E, Nguyen B. {EGDT}. arXiv preprint. 2001."),
    ] {
        let a = assess(&Claim::from_text(&text), &egdt());
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
}

#[test]
fn reprints_and_versions_cited_with_another_year_are_plausible() {
    // Red team (#102) real misses: a book reprinted whole in a reader, and
    // a dataset whose DOI's record carries its latest version's year.
    let mut reprint = record(
        &["10.4324/9781315680347-10"],
        "Distinction: A Social Critique of the Judgement of Taste",
        &["Pierre Bourdieu"],
        2018,
    );
    reprint.work.work_type = Some(crate::work::WorkType::Chapter);
    let text = "Bourdieu P. Distinction: A Social Critique of the Judgement of Taste. Cambridge, \
                MA: Harvard University Press; 1984.";
    let a = assess(&Claim::from_text(text), &reprint);
    assert_eq!(a.year.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Nor, since round 6 (#126), a chapter cited as an article of an
    // earlier year: an article reprinted in a collection can't be told
    // from it by the records (wild-2's Crone 1980, held only as its 2017
    // reprint). Plausible, never verified.
    let text = "Bourdieu P. Distinction: a social critique of the judgement of taste. Soc Sci Inf. \
                1984;13:1-20.";
    let a = assess(&Claim::from_text(text), &reprint);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");

    let mut mimic = record(
        &["10.13026/c2xw26"],
        "MIMIC-III Clinical Database",
        &["Alistair Johnson", "Tom Pollard", "Roger Mark"],
        2023,
    );
    mimic.work.work_type = Some(crate::work::WorkType::Dataset);
    let text = "Johnson A, Pollard T, Mark R. MIMIC-III Clinical Database (version 1.4). PhysioNet. \
                2016. doi:10.13026/C2XW26";
    let a = assess(&Claim::from_text(text), &mimic);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Without the identifier, the year still tells another version.
    let text = "Johnson A, Pollard T, Mark R. MIMIC-III Clinical Database. PhysioNet. 2016.";
    assert!(assess(&Claim::from_text(text), &mimic).confidence < PLAUSIBLE);
}

#[test]
fn a_preprint_cited_as_its_journal_version_without_a_volume_is_another_version() {
    // holdout-5 #784: OpenAlex's record of the journal's article, merged
    // with the PsyArXiv preprint, names its journal but no volume; cited
    // with the journal's place and the preprint's DOI.
    let mut record = article(
        "Control of memory retrieval alters memory-based eye movements",
        &[
            "Mrinmayi Kulkarni",
            "Allison E. Nickel",
            "Greta N. Minor",
            "Deborah E. Hannula",
        ],
        2024,
        (
            "Journal of Experimental Psychology Learning Memory and Cognition",
            "50",
            "1199",
        ),
    );
    record.work.volume = None;
    record.work.page = None;
    record.ids = vec![
        "doi:10.1037/xlm0001321".parse().unwrap(),
        "doi:10.31234/osf.io/q4fvs".parse().unwrap(),
    ];
    let cited = |doi: &str| {
        format!(
            "Kulkarni, M., Nickel, A. E., Minor, G., & Hannula, D. E. (2024). Control of Memory \
             Retrieval Alters Memory-Based Eye Movements. Journal of Experimental Psychology: \
             Learning, Memory, and Cognition, 50(8), 1199–1219. https://doi.org/{doi}"
        )
    };
    let claim = Claim::from_text(cited("10.31234/osf.io/q4fvs"));
    assert_eq!(preprint_version_claimed(&claim, &record), Some("PsyArXiv"));
    let a = assess(&claim, &record);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // The journal's own DOI: the article.
    let a = assess(&Claim::from_text(cited("10.1037/xlm0001321")), &record);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // A record in no journal and at no volume is none placed.
    record.work.container_title = None;
    assert_eq!(preprint_version_claimed(&claim, &record), None);
}

#[test]
fn a_preprint_cited_as_its_journal_version_is_another_version() {
    // holdout-4's `w:preprint-as-journal`: the preprint's DOI given with
    // the journal's name and place, found on the journal article's record
    // that carries both (OpenAlex merges them).
    let merged = |preprint_doi: &str| {
        let mut r = article(
            "Effectiveness of pleurodesis for postoperative air leaks after lung resection",
            &["Norifumi Tsubokawa", "Takahiro Mimae", "Ryoichi Ito"],
            2024,
            ("Journal of Cardiothoracic Surgery", "19", "2"),
        );
        r.ids = vec![
            "doi:10.1186/s13019-023-02444-6".parse().unwrap(),
            preprint_doi.parse().unwrap(),
        ];
        r
    };
    let cited = |doi: &str| {
        format!(
            "Tsubokawa N, Mimae T, Ito R, et al. Effectiveness of Pleurodesis for Postoperative \
             Air Leaks after Lung Resection. Journal of Cardiothoracic Surgery. 2024;19(1):2. \
             doi:{doi}"
        )
    };
    let record = merged("doi:10.21203/rs.3.rs-2791419/v1");
    let claim = Claim::from_text(cited("10.21203/rs.3.rs-2791419/v1"));
    let a = assess(&claim, &record);
    assert_eq!(a.ids, Agreement::Match);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    assert_eq!(
        preprint_version_claimed(&claim, &record),
        Some("Research Square")
    );
    // The journal's own DOI: the article.
    let claim = Claim::from_text(cited("10.1186/s13019-023-02444-6"));
    let a = assess(&claim, &record);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    assert_eq!(preprint_version_claimed(&claim, &record), None);
    // A bioRxiv DOI is a preprint's; Genome Research's, under the same
    // prefix, is an article's.
    let record = merged("doi:10.1101/2023.01.02.522000");
    let claim = Claim::from_text(cited("10.1101/2023.01.02.522000"));
    assert!(assess(&claim, &record).confidence < VERIFIED);
    let record = merged("doi:10.1101/gr.277000.122");
    let claim = Claim::from_text(cited("10.1101/gr.277000.122"));
    assert!(assess(&claim, &record).confidence >= VERIFIED);

    // An arXiv identifier beside the journal: as physics cites.
    let record = merged("arxiv:2401.01234");
    let claim = Claim::from_text(
        "Tsubokawa N, Mimae T, Ito R, et al. Effectiveness of Pleurodesis for Postoperative Air \
         Leaks after Lung Resection. Journal of Cardiothoracic Surgery. 2024;19(1):2. \
         arXiv:2401.01234",
    );
    assert!(assess(&claim, &record).confidence >= VERIFIED);
}

#[test]
fn a_notice_is_not_the_original_it_corrects() {
    // holdout-4's `w:erratum-cited-as-original`: the original's title and
    // authors at the notice's place, or with its DOI. Nature's and
    // Springer's notices are titled "Author Correction: …", "Publisher
    // Correction: …"; a notice's record may say what it is only by type.
    let mut correction = article(
        "Author Correction: Enhanced energy coupling for indirect-drive fast-ignition fusion \
         targets",
        &["F. Zhang", "H. B. Cai", "W. M. Zhou"],
        2020,
        ("Nature Physics", "16", "815-815"),
    );
    correction.work.work_type = Some(crate::work::WorkType::Other("erratum".into()));
    let mut publisher = record(
        &["doi:10.1007/s15010-026-02851-6"],
        "Publisher Correction: Efficacy and safety of praziquantel combined with artemisinin \
         derivatives versus praziquantel monotherapy for schistosomiasis",
        &["Yu Yang", "Jie Meng", "Chen Li"],
        2026,
    );
    publisher.work.work_type = Some(crate::work::WorkType::JournalArticle);
    publisher.work.container_title = Some("Infection".into());
    let mut retraction = article(
        "Shear and shuffle in twinning in titanium – RETRACTION",
        &["Bin Li"],
        2016,
        ("Journal of Materials Research", "31", "2361-2361"),
    );
    retraction.work.work_type = Some(crate::work::WorkType::Other("retraction".into()));
    for (claim, record) in [
        (
            "Zhang, F. et al. (2020) 'Enhanced energy coupling for indirect-drive fast-ignition \
             fusion targets', Nature Physics, 16(7), pp. 815–815.",
            &correction,
        ),
        (
            "Yang Y, Meng J, Li C, et al. Efficacy and safety of praziquantel combined with \
             artemisinin derivatives versus praziquantel monotherapy for schistosomiasis. \
             Infection. 2026. doi:10.1007/s15010-026-02851-6",
            &publisher,
        ),
        (
            "Li B. Shear and shuffle in twinning in titanium. Journal of Materials Research. \
             2016;31(15):2361–2361.",
            &retraction,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence < VERIFIED, "{claim}: {a:?}");
        assert!(record_derivative_kind(&record.work).is_some(), "{claim}");
    }
    // Cited as the notice it is, it verifies.
    let claim = Claim::from_text(
        "Zhang, F. et al. (2020) 'Author Correction: Enhanced energy coupling for \
         indirect-drive fast-ignition fusion targets', Nature Physics, 16(7), pp. 815–815.",
    );
    let a = assess(&claim, &correction);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // A retracted article is the work cited, however marked and typed:
    // IEEE's keep their DOI under "Notice of Retraction", Elsevier's under
    // "RETRACTED:", and OpenAlex types them `retraction`.
    for title in [
        "Notice of Retraction: Laboratorial Research on the Treatment of Multi-A/O Process to \
         Municipal Wastewater",
        "RETRACTED: Laboratorial Research on the Treatment of Multi-A/O Process to Municipal \
         Wastewater",
    ] {
        let mut retracted = article(
            title,
            &["Huifang Dong", "Dapeng Deng", "Dagong Li"],
            2011,
            (
                "2011 5th International Conference on Bioinformatics",
                "1",
                "1-4",
            ),
        );
        retracted.work.work_type = Some(crate::work::WorkType::Other("retraction".into()));
        let claim = Claim::from_text(
            "Dong H, Deng D, Li D. Laboratorial Research on the Treatment of Multi-A/O Process to \
             Municipal Wastewater. 2011.",
        );
        let a = assess(&claim, &retracted);
        assert!(a.confidence >= VERIFIED, "{title}: {a:?}");
    }
}

#[test]
fn a_preprint_cited_with_its_journal_versions_longer_title_is_another_version() {
    // holdout-6 #798: the medRxiv preprint cited with its journal
    // version's title, a subtitle added and an article left out.
    let mut preprint = record(
        &["doi:10.1101/2020.12.24.20248813"],
        "Quantifying the online news media coverage of the COVID-19 pandemic",
        &["Konrad Krawczyk", "Tomasz Chelkowski", "Daniel J. Laydon"],
        2020,
    );
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    preprint.work.container_title = Some("medRxiv".into());
    let cited = "Krawczyk, K. et al. (2020) 'Quantifying Online News Media Coverage of the \
                 COVID-19 Pandemic: Text Mining Study and Resource', medRxiv [Preprint].";
    let a = assess(&Claim::from_text(cited), &preprint);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Its own title, the article left out, verifies.
    let cited = "Krawczyk, K. et al. (2020) 'Quantifying Online News Media Coverage of the \
                 COVID-19 Pandemic', medRxiv [Preprint].";
    let a = assess(&Claim::from_text(cited), &preprint);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}
