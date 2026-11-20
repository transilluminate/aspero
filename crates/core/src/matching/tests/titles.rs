//! Titles: found inside a reference, shortened, or with words added,
//! inserted, replaced or reworded; a year or URL in a title.

use super::*;

#[test]
fn shared_title_finds_the_cited_title_inside_a_derivative_one() {
    let reference = "Jumper J, et al. Highly accurate protein structure prediction with AlphaFold. Nature. 2021.";
    assert_eq!(
        shared_title(
            "Faculty Opinions recommendation of Highly accurate protein structure prediction with AlphaFold.",
            reference
        )
        .as_deref(),
        Some("highly accurate protein structure prediction with alphafold")
    );
    assert_eq!(
        shared_title("Deep learning", "LeCun Y. Deep learning. Nature 2015").as_deref(),
        Some("deep learning"),
        "a short title counts when whole"
    );
    assert_eq!(
        shared_title("Deep learning for AI", "Smith J. On learning. 2020"),
        None
    );
}

#[test]
fn retraction_markers_do_not_hide_a_title_match() {
    assert_eq!(
        unmarked("RETRACTED: Ileal-lymphoid hyperplasia"),
        "Ileal-lymphoid hyperplasia"
    );
    assert_eq!(unmarked("Retracted Article - Something"), "Something");
    assert_eq!(unmarked("Retraction: Something"), "Retraction: Something");
    assert_eq!(
        unmarked("Retracted proteins in yeast"),
        "Retracted proteins in yeast"
    );
    assert_eq!(
        title_similarity(
            "RETRACTED: Ileal-lymphoid hyperplasia",
            "Ileal-lymphoid hyperplasia"
        ),
        1.0
    );
}

#[test]
fn a_url_is_not_a_title() {
    // Dogfood: a Crossref placeholder titled with a notice page's URL,
    // against a citation whose DOI link Preview spaced out.
    let claim =
        Claim::from_text("Eur Respir J. 2021. https:// doi. org/ 10. 1183/ 23120 541. 00489- 2021");
    let placeholder = record(
        &[],
        "https://www.ejhbe.com/notification-about-the-doi",
        &[],
        2021,
    );
    let a = assess(&claim, &placeholder);
    assert_eq!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
    // Nor are a link's words a title's.
    let notice = record(&[], "DOI notice", &[], 2021);
    let a = assess(&claim, &notice);
    assert_ne!(a.title.agreement, Agreement::Match, "{a:?}");
}

#[test]
fn a_few_shared_topic_words_are_not_a_title_match() {
    // Dogfood control: a fabricated citation sharing "septic shock".
    let claim = Claim::from_text("Smith 2019 Ketamine reduces mortality in septic shock");
    let a = assess(
        &claim,
        &record(
            &[],
            "Leukotrienes in Endotoxic and Septic Shock",
            &["J Smith"],
            2019,
        ),
    );
    assert_eq!(a.title.agreement, Agreement::Mismatch);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn dropped_subtitles_still_match() {
    let structured = title_similarity(
        "Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism",
        "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism: The ADJUST-PE Study",
    );
    assert_eq!(structured, 1.0);
    let claim = Claim::from_text(
        "Righini M et al. Age-adjusted D-dimer cutoff levels to rule out pulmonary embolism. JAMA 2014",
    );
    let a = assess(
        &claim,
        &record(
            &[],
            "Age-Adjusted D-Dimer Cutoff Levels to Rule Out Pulmonary Embolism: The ADJUST-PE Study",
            &["Marc Righini"],
            2014,
        ),
    );
    assert_eq!(a.title.agreement, Agreement::Match);
}

#[test]
fn a_title_with_words_added_is_not_the_records() {
    // Red team (#102): another work's title containing this one's, or a
    // subtitle it lacks or doesn't have.
    for (text, record) in [
        (
            "LeCun Y, Bengio Y, Hinton G. Deep learning for medical image analysis. Nature. \
             2015;521:436-444.",
            lecun(),
        ),
        (
            "LeCun Y, Bengio Y, Hinton G. Deep learning: a primer for clinicians. Nature. 2015.",
            lecun(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation of targeted \
             therapies. Cell. 2011;144(5):646-674.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: implications for therapy. Cell. 2011.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. Colorectal hallmarks of cancer: the next generation. Cell. \
             2011.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation enables therapy. \
             Cell. 2011.",
            hallmarks(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.title.agreement, Agreement::Partial, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    let claim = Claim {
        title: Some("Deep learning: a primer for clinicians".into()),
        authors: vec!["LeCun, Y.".into(), "Bengio, Y.".into()],
        year: Some(2015),
        container: Some("Nature".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &lecun());
    assert_eq!(a.title.agreement, Agreement::Partial);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn a_title_cited_as_another_ones_subtitle_is_not_the_records() {
    // #168: Seymour's title has Singer's as its subtitle. Free text grades
    // it as the fields do: partly the same, never the same.
    let singer = article(
        "The Third International Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)",
        &[
            "Mervyn Singer",
            "Clifford S. Deutschman",
            "Christopher Warren Seymour",
        ],
        2016,
        ("JAMA", "315", "801-810"),
    );
    let title = "Assessment of Clinical Criteria for Sepsis: For the Third International \
                 Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)";
    let text =
        format!("Seymour CW, Liu VX, Iwashyna TJ, et al. {title}. JAMA. 2016;315(8):762-774.");
    let as_text = assess(&Claim::from_text(text), &singer);
    let as_fields = assess(
        &Claim {
            title: Some(title.into()),
            ..Claim::default()
        },
        &singer,
    );
    assert_eq!(as_text.title.agreement, Agreement::Partial, "{as_text:?}");
    assert_eq!(
        as_fields.title.agreement,
        Agreement::Partial,
        "{as_fields:?}"
    );
    assert_eq!(as_text.title.score, as_fields.title.score);
    // A label before the title isn't a main title: an acronym's, a
    // number's, a work's short name, a notice's.
    for text in [
        "Singer M, Deutschman CS, Seymour CW. Consensus3: The Third International Consensus \
         Definitions for Sepsis and Septic Shock (Sepsis-3). JAMA. 2016;315(8):801-810.",
        "Singer M, Deutschman CS, Seymour CW. Author Correction: The Third International \
         Consensus Definitions for Sepsis and Septic Shock (Sepsis-3). JAMA. \
         2016;315(8):801-810.",
        "Singer M, Deutschman CS, Seymour CW. SEPSIS: The Third International Consensus \
         Definitions for Sepsis and Septic Shock (Sepsis-3). JAMA. 2016;315(8):801-810.",
        "Singer M, Deutschman CS, Seymour CW. Sepsis-3: The Third International Consensus \
         Definitions for Sepsis and Septic Shock (Sepsis-3). JAMA. 2016;315(8):801-810.",
    ] {
        let a = assess(&Claim::from_text(text), &singer);
        assert_eq!(a.title.agreement, Agreement::Match, "{text}: {a:?}");
    }
}

#[test]
fn a_title_cited_as_is_or_without_its_subtitle_is_the_records() {
    for (text, record) in [
        (
            "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.",
            lecun(),
        ),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 436–444 (2015).",
            lecun(),
        ),
        (
            "LeCun, Y. et al. (2015) 'Deep learning', Nature, 521, pp. 436-444.",
            lecun(),
        ),
        (
            "Y. LeCun, Y. Bengio, and G. Hinton, \"Deep learning,\" Nature, vol. 521, 2015.",
            lecun(),
        ),
        ("LeCun et al. Deep learning in Nature 2015", lecun()),
        (
            "LeCun and Hinton Deep learning doi:10.1038/nature14539 2015",
            lecun(),
        ),
        (
            "LeCun et al. described in Nature (2015) — \"Deep learning\".",
            lecun(),
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer. Cell. 2011;144:646.",
            hallmarks(),
        ),
        (
            "Hanahan D, Weinberg RA. The hallmarks of cancer: the next generation. Cell. 2011.",
            hallmarks(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.title.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    let a = assess(
        &Claim::from_text("Vaswani A, Shazeer N. Attention is all you need NeurIPS 2017"),
        &attention(&[], 2017),
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    let claim = Claim {
        title: Some("Hallmarks of cancer".into()),
        authors: vec!["Hanahan, D.".into()],
        year: Some(2011),
        ..Claim::default()
    };
    assert!(assess(&claim, &hallmarks()).confidence >= VERIFIED);
    // A book's record without the subtitle cited (OpenAlex's "Imagined
    // communities"); an article's must have it.
    let mut book = record(&[], "Imagined communities", &["Benedict Anderson"], 1983);
    book.work.work_type = Some(crate::work::WorkType::Book);
    let text = "Anderson, B. (1983) Imagined Communities: Reflections on the Origin and Spread of \
                Nationalism. London: Verso.";
    assert!(assess(&Claim::from_text(text), &book).confidence >= VERIFIED);
    let claim = Claim {
        title: Some(
            "Imagined Communities: Reflections on the Origin and Spread of Nationalism".into(),
        ),
        authors: vec!["Anderson, B.".into()],
        year: Some(1983),
        ..Claim::default()
    };
    assert!(assess(&claim, &book).confidence >= VERIFIED);
    book.work.work_type = Some(crate::work::WorkType::JournalArticle);
    assert!(assess(&claim, &book).confidence < VERIFIED);
}

#[test]
fn a_year_in_the_title_is_not_the_year_cited() {
    // Red team (#102): "Cancer statistics, 2020" cited as of 2018.
    let mut stats = placed(
        &[],
        "Cancer statistics, 2020",
        &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
        2020,
        ("CA: A Cancer Journal for Clinicians", "70", "7-30"),
    );
    stats.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for text in [
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2018;70(1):7-30.",
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. 2017.",
    ] {
        let claim = Claim::from_text(text);
        let a = assess(&claim, &stats);
        assert_eq!(a.year.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
        assert!(
            !claim
                .cited_years_beside(stats.work.title.as_deref())
                .contains(&2020)
        );
    }
    // Its title (year and all), journal, volume and pages name it: the
    // year cited is the citation's error, plausible. The 2018 report, the
    // year cited and in its title, is another title.
    let text = "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2018;70(1):7-30.";
    let a2020 = assess(&Claim::from_text(text), &stats);
    assert!(
        (PLAUSIBLE..VERIFIED).contains(&a2020.confidence),
        "{a2020:?}"
    );
    let mut stats2018 = placed(
        &[],
        "Cancer statistics, 2018",
        &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
        2018,
        ("CA: A Cancer Journal for Clinicians", "68", "7-30"),
    );
    stats2018.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let a2018 = assess(&Claim::from_text(text), &stats2018);
    assert_ne!(a2018.title.agreement, Agreement::Match, "{a2018:?}");
    assert!(a2018.confidence < PLAUSIBLE, "{a2018:?}");
    // Without the place, the year alone decides.
    let text = "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. 2017.";
    assert!(assess(&Claim::from_text(text), &stats).confidence < PLAUSIBLE);
    for text in [
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. 2020;70(1):7-30.",
        "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020.",
    ] {
        let a = assess(&Claim::from_text(text), &stats);
        assert_ne!(a.year.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn another_title_at_the_right_place_is_a_different_title() {
    // Red team (#102): the title was treated as missing whenever journal
    // and volume agreed, and the place verified another work's title.
    let mut niwa = placed(
        &[],
        "Lytic enzyme, labiase for a broad range of Gram-positive bacteria",
        &["Tomoko Niwa", "Yoshiaki Kawamura"],
        2005,
        ("Journal of Microbiological Methods", "61", "251-260"),
    );
    niwa.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for (text, record) in [
        (
            "Niwa T. Transfection of plasmid DNA by nanocarriers. J Microbiol Methods. 2005;61:251.",
            niwa,
        ),
        (
            "Rivers E, et al. Vasopressin versus norepinephrine for septic shock. N Engl J Med. \
             2001;345(19):1368-1377.",
            egdt(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert_eq!(a.title.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // A short title, read from the layout, is given too (#172): Littman's
    // and Ghahramani's papers, beside LeCun's at Nature 521, aren't
    // "Deep learning".
    let lecun_text = "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. \
                      2015;521(7553):436-444. doi:10.1038/nature14540";
    for (title, authors, pages) in [
        (
            "Reinforcement learning improves behaviour from evaluative feedback",
            &["Michael L. Littman"][..],
            "445-451",
        ),
        (
            "Probabilistic machine learning and artificial intelligence",
            &["Zoubin Ghahramani"][..],
            "452-459",
        ),
    ] {
        let mut other = placed(&[], title, authors, 2015, ("Nature", "521", pages));
        other.work.work_type = Some(crate::work::WorkType::JournalArticle);
        let a = assess(&Claim::from_text(lecun_text), &other);
        assert_eq!(a.title.agreement, Agreement::Mismatch, "{title}: {a:?}");
        assert!(a.confidence < PLAUSIBLE, "{title}: {a:?}");
    }
    let a = assess(&Claim::from_text(lecun_text), &lecun());
    assert_eq!(a.title.agreement, Agreement::Match, "{a:?}");
    // Beside the work's own authors, a short label is no other title
    // (wild-2's "RESP score", for Schmidt's paper at its place).
    let mut resp = placed(
        &["10.1164/rccm.201311-2023OC"],
        "Predicting survival after extracorporeal membrane oxygenation for severe acute \
         respiratory failure. The Respiratory Extracorporeal Membrane Oxygenation Survival \
         Prediction (RESP) score",
        &["Matthieu Schmidt", "Michael Bailey"],
        2014,
        (
            "American Journal of Respiratory and Critical Care Medicine",
            "189",
            "1374-1382",
        ),
    );
    resp.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Schmidt M, et al. RESP score. Am J Respir Crit Care Med 2014;189:1374-82.";
    let a = assess(&Claim::from_text(text), &resp);
    assert_eq!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // Without a title, the place still names the work.
    for text in [
        "Rivers E, et al. N Engl J Med. 2001;345:1368.",
        "Rivers E, Nguyen B. NEJM 2001;345(19):1368-77. Epub 2001 Nov 8.",
    ] {
        let a = assess(&Claim::from_text(text), &egdt());
        assert_eq!(a.title.agreement, Agreement::Unknown, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn the_report_titled_with_its_year_and_place_outranks_its_namesakes() {
    // Live (app pre-test): "Cancer statistics, 2020 … 2018;70(1):7-30" chose
    // "Colorectal cancer statistics, 2020" (its title partial, its page
    // another), taking the title's 2020 for the year cited.
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/resolve/cancer-statistics-cited-2018.json"
    ))
    .unwrap();
    let claim = Claim::from_text(fixture["claim"].as_str().unwrap());
    assert_eq!(claim.as_read().year, Some(2018), "the year cited");
    let field = |c: &serde_json::Value, k: &str| c[k].as_str().unwrap().to_owned();
    let assessed: Vec<(String, Assessment)> = fixture["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            let authors: Vec<String> = c["authors"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| a.as_str().unwrap().to_owned())
                .collect();
            let authors: Vec<&str> = authors.iter().map(String::as_str).collect();
            let year = i16::try_from(c["year"].as_i64().unwrap()).unwrap();
            let at = (field(c, "container"), field(c, "volume"), field(c, "page"));
            let record = article(&field(c, "title"), &authors, year, (&at.0, &at.1, &at.2));
            (field(c, "id"), assess(&claim, &record))
        })
        .collect();
    let (best, a) = assessed
        .iter()
        .max_by(|x, y| x.1.confidence.total_cmp(&y.1.confidence))
        .unwrap();
    assert_eq!(best, fixture["cited"].as_str().unwrap(), "{assessed:#?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    assert_eq!(
        (a.title.agreement, a.container.agreement, a.year.agreement),
        (Agreement::Match, Agreement::Match, Agreement::Mismatch)
    );
    for (id, other) in &assessed {
        if id != best {
            assert!(other.confidence < a.confidence, "{id}: {other:?}");
        }
    }
}

#[test]
fn a_title_with_words_inserted_is_not_the_records() {
    // App pre-test: words inserted in the middle of a real title.
    let sepsis3 = article(
        "The Third International Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)",
        &[
            "Mervyn Singer",
            "Clifford S. Deutschman",
            "Christopher Warren Seymour",
        ],
        2016,
        ("JAMA", "315", "801-810"),
    );
    let cite = |title: &str| {
        Claim::from_text(format!(
            "Singer M, Deutschman CS, Seymour CW, et al. {title}. JAMA. 2016;315(8):801-810."
        ))
    };
    let a = assess(
        &cite(
            "The Third International Consensus Definitions for Sepsis and Septic Shock in Adults \
             and Children (Sepsis-3)",
        ),
        &sepsis3,
    );
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Words dropped, or the title as it is: the same title.
    for title in [
        "The Third International Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)",
        "The third international consensus definitions for sepsis and septic shock",
        "Third International Consensus Definitions for Sepsis (Sepsis-3)",
    ] {
        let a = assess(&cite(title), &sepsis3);
        assert!(a.confidence >= VERIFIED, "{title}: {a:?}");
    }
}

#[test]
fn a_title_with_a_word_replaced_is_not_the_records() {
    // Red team (#102): another drug's trial at RECOVERY's place verified.
    let mut recovery = placed(
        &[],
        "Dexamethasone in Hospitalized Patients with Covid-19",
        &["Peter Horby", "Wei Shen Lim", "Jonathan R. Emberson"],
        2021,
        ("New England Journal of Medicine", "384", "693-704"),
    );
    recovery.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Horby P, Lim WS, Emberson JR, et al. Hydroxychloroquine in hospitalized patients \
                with Covid-19. N Engl J Med. 2021;384(8):693-704.";
    let a = assess(&Claim::from_text(text), &recovery);
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // A misspelt or respelt word is the same title.
    for text in [
        "Horby P, Lim WS, Emberson JR, et al. Dexamethazone in hospitalized patients with \
         Covid-19. N Engl J Med. 2021;384(8):693-704.",
        "Horby P, Lim WS, Emberson JR, et al. Dexamethasone in hospitalized patients with \
         Covid-19. N Engl J Med. 2021;384(8):693-704.",
    ] {
        let a = assess(&Claim::from_text(text), &recovery);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn a_title_reworded_with_synonyms_is_not_the_records() {
    // h2-726: words swapped for synonyms, given apart.
    for (claimed, actual) in [
        (
            "Function of Insulin Resistance in Human Illness",
            "Role of Insulin Resistance in Human Disease",
        ),
        (
            "Breast cancer screening uptake and barriers among females in Hue, Vietnam: A \
             cross-sectional review.",
            "Breast cancer screening uptake and barriers among women in Hue, Vietnam: A \
             cross-sectional survey.",
        ),
    ] {
        assert!(
            crate::matching::titles::replaces(claimed, actual),
            "{claimed}"
        );
    }
    // The same title written otherwise is not reworded: respelt, misspelt,
    // abbreviated, a number or Greek letter spelt out, a word broken at a
    // line's end, words dropped.
    for (claimed, actual) in [
        (
            "Randomised trial of tumour necrosis factor alpha blockade vs placebo",
            "Randomized trial of tumor necrosis factor α blockade versus placebo",
        ),
        (
            "Type II diabetes in the programme: a mort ality study",
            "Type 2 diabetes in the program: a mortality study",
        ),
        (
            "Role of Insulin Resistance in Disease",
            "Role of Insulin Resistance in Human Disease",
        ),
        // A Roman numeral for a digit, years that differ by a digit aligned
        // as they are (holdout-1 #277).
        (
            "Midlife in the United States (MIDUS 2), 2004-2006",
            "National Survey of Midlife Development in the United States (MIDUS II), 2004-2006",
        ),
        (
            "Role of Insulin Resistance in Human Disease",
            "Role of Insulin Resistance in Human Disease",
        ),
        // Another title altogether isn't reworded: it differs.
        (
            "Deep residual learning for image recognition",
            "Role of Insulin Resistance in Human Disease",
        ),
    ] {
        assert!(
            !crate::matching::titles::replaces(claimed, actual),
            "{claimed}"
        );
    }
}

#[test]
fn a_journals_words_in_the_title_are_the_titles() {
    // h3-594: a reworded title whose words are the journal's too, at the
    // work's place with its DOI, was read as no title at all.
    let mut gh = placed(
        &["doi:10.1016/s1096-6374(02)00018-7"],
        "The role of growth hormone in fetal development",
        &["Michael J. Waters", "P. L. Kaye"],
        2002,
        ("Growth Hormone & IGF Research", "12", "137-146"),
    );
    gh.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let reworded = Claim::from_text(
        "27. Waters MJ, Kaye PL. The contribution of growth hormone in fetal design. Growth \
         Hormone & IGF Research. 2002;12(3):137-146. doi:10.1016/s1096-6374(02)00018-7",
    );
    let a = assess(&reworded, &gh);
    assert_ne!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence < VERIFIED, "{a:?}");
    // Without a title, the journal's words are the journal's.
    let titleless =
        Claim::from_text("Waters MJ, Kaye PL. Growth Hormone & IGF Research. 2002;12(3):137-146.");
    let a = assess(&titleless, &gh);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}
