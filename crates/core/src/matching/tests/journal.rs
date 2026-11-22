//! The journal and place cited: journal, volume and first page, titleless
//! references, another or a sister journal, and copies with no place.

use super::*;

#[test]
fn journal_volume_and_page_are_checked_in_text_and_fields() {
    let text = "Meyer G, et al. Fibrinolysis for patients with intermediate-risk pulmonary embolism. N Engl J Med. 2014;370(15):1402-11.";
    let a = assess(&Claim::from_text(text), &peitho());
    assert_eq!(
        (a.container.agreement, a.volume.agreement, a.page.agreement),
        (Agreement::Match, Agreement::Match, Agreement::Match)
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");

    // A wrong first page: a real error, and what tells the article from
    // a reply printed later in the same volume.
    let wrong_page = text.replace("1402-11", "2440");
    let a = assess(&Claim::from_text(wrong_page), &peitho());
    assert_eq!(a.page.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");

    // Structured: the journal is compared by abbreviation rules. A record
    // in a journal, its type not given (Europe PMC's editorials and
    // letters; holdout-1 #478, #492), is held to it as an article is: the
    // wrong journal is plausible. For a work in proceedings, a different
    // name lowers the ranking without settling the verdict.
    let claim = Claim {
        title: Some("Fibrinolysis for patients with intermediate-risk pulmonary embolism".into()),
        authors: vec!["Meyer G".into()],
        year: Some(2014),
        container: Some("Lancet".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &peitho());
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    let mut proceedings = peitho();
    proceedings.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    let a = assess(&claim, &proceedings);
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!(a.confidence >= VERIFIED && a.confidence < 1.0, "{a:?}");
}

#[test]
fn titleless_references_that_give_journal_volume_and_page_verify() {
    let a = assess(
        &Claim::from_text("Meyer G et al. N Engl J Med 2014;370:1402"),
        &peitho(),
    );
    assert_eq!(a.title.agreement, Agreement::Unknown);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // The right paper with a wrong page: plausible, not missing.
    let a = assess(
        &Claim::from_text("Meyer G et al. N Engl J Med 2014;370:2440"),
        &peitho(),
    );
    assert_eq!(a.page.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Without the journal agreeing, the missing title still counts.
    let a = assess(
        &Claim::from_text("Meyer G et al. Lancet 2014;370:1402"),
        &peitho(),
    );
    assert_eq!(a.title.agreement, Agreement::Mismatch);
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn journal_volume_and_first_page_name_a_work_without_a_title() {
    // #23: running headers and short forms.
    let laan = placed(
        &["doi:10.1016/j.injury.2015.11.045"],
        "Chest wall thickness and decompression failure: A systematic review and meta-analysis",
        &["Laan", "Vu", "Thiels"],
        2016,
        ("Injury", "47", "797-804"),
    );
    for text in [
        "Injury, Int. J. Care Injured 47 (2016) 797\u{2013}804",
        "Injury. 2016;47(4):797-804.",
        "Laan DV, et al. Injury 2016;47:797.",
    ] {
        let a = assess(&Claim::from_text(text), &laan);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    let lecun = placed(
        &[],
        "Deep learning",
        &["LeCun", "Bengio", "Hinton"],
        2015,
        ("Nature", "521", "436-444"),
    );
    let a = assess(
        &Claim::from_text("Nature 521, 436\u{2013}444 (2015)"),
        &lecun,
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // Online first, in print the next year: plausible.
    let a = assess(&Claim::from_text("Injury. 2017;47(4):797-804."), &laan);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Another page, or other authors: not this work.
    for text in [
        "Injury, Int. J. Care Injured 47 (2016) 805\u{2013}812",
        "Smith J, et al. Injury 2016;47:797.",
    ] {
        let a = assess(&Claim::from_text(text), &laan);
        assert!(a.confidence < PLAUSIBLE, "{text}: {a:?}");
    }
}

#[test]
fn a_reference_without_a_title_is_not_a_different_title() {
    // Dogfood (#33): authors, journal, year and the DOI, no title.
    let doi = "doi:10.1183/23120541.00489-2021";
    let work = placed(
        &[doi],
        "Community-acquired pneumonia subgroups and differential response to corticosteroids",
        &["Wittermans", "Vestjens", "Blum"],
        2021,
        ("ERJ Open Research", "8", "00489-2021"),
    );
    let claim = Claim::from_text(format!("Wittermans E, et al. ERJ Open Res. 2022. {doi}"));
    let a = assess(&claim, &work);
    assert_eq!(a.title.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence >= PLAUSIBLE, "{a:?}");
    // Without an identifier or a place, authors, journal and year fit too
    // many works.
    let a = assess(
        &Claim::from_text("Wittermans E, et al. ERJ Open Res. 2021."),
        &work,
    );
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
    // A reference that does give a title still disagrees.
    let claim = Claim::from_text(format!(
        "Wittermans E, et al. Steroids in asthma. ERJ Open Res. 2021. {doi}"
    ));
    assert_eq!(assess(&claim, &work).title.agreement, Agreement::Mismatch);
}

#[test]
fn a_reference_naming_another_journal_is_not_verified() {
    for (right, wrong) in [
        (PLASTICENTA_VANCOUVER, "Environ Sci Technol"),
        (PLASTICENTA_APA, "Environmental Science & Technology"),
    ] {
        let a = assess(&Claim::from_text(right), &plasticenta());
        assert_eq!(
            (a.container.agreement, a.volume.agreement, a.page.agreement),
            (Agreement::Match, Agreement::Match, Agreement::Match),
            "{right}"
        );
        assert!(a.confidence >= VERIFIED, "{right}: {a:?}");

        let text = right
            .replace("Environ Int", wrong)
            .replace("Environment International", wrong);
        let claim = Claim::from_text(&text);
        let a = assess(&claim, &plasticenta());
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
        // The journal as read, for "cited in X, found in Y".
        assert_eq!(claim.as_read().container.as_deref(), Some(wrong));
    }
    // A structured claim naming another journal: the same.
    let claim = Claim {
        title: Some("Plasticenta: first evidence of microplastics in human placenta".into()),
        authors: vec!["Ragusa A".into()],
        year: Some(2021),
        container: Some("Environ Sci Technol".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &plasticenta());
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");

    // Its preprint, sharing the main title, is another version of a work
    // cited by journal, volume and page: plausible at most (live, it
    // outranked the capped article).
    let mut preprint = record(
        &["10.1101/2020.07.15.198325"],
        "Plasticenta: Microplastics in Human Placenta",
        &[
            "Antonio Ragusa",
            "Alessandro Svelato",
            "Criselda Santacroce",
        ],
        2020,
    );
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    for text in [
        PLASTICENTA_VANCOUVER.to_owned(),
        PLASTICENTA_VANCOUVER.replace("Environ Int", "Environ Sci Technol"),
    ] {
        let a = assess(&Claim::from_text(&text), &preprint);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
    }
    // Cited without a journal and place, it may be the preprint.
    let text =
        "Ragusa A, Svelato A, Santacroce C. Plasticenta: microplastics in human placenta. 2020.";
    let a = assess(&Claim::from_text(text), &preprint);
    assert_eq!(a.container.agreement, Agreement::Unknown);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn journals_abbreviated_with_words_left_out_are_no_mismatch() {
    let mut article = placed(
        &[],
        "Fluid resuscitation in experimental sepsis",
        &["Jane Doe", "Alan Roe"],
        2010,
        (
            "Proceedings of the Royal Society B: Biological Sciences",
            "277",
            "100-110",
        ),
    );
    article.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Doe J, Roe A. Fluid resuscitation in experimental sepsis. Proc Biol Sci. \
                2010;277:100-10.";
    let a = assess(&Claim::from_text(text), &article);
    assert_ne!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_journal_read_from_the_reference_outweighs_words_elsewhere_in_it() {
    // "SR" among the authors' initials is not Scientific Reports, nor
    // "mice" in the title Medicine, when the reference names another.
    for (text, title, author, year, at) in [
        (
            "Ragusa A, Rossi SR. Microplastics in human placenta. Nature. 2021;11:1234.",
            "Microplastics in human placenta",
            "Antonio Ragusa",
            2021,
            ("Scientific Reports", "11", "1234"),
        ),
        (
            "Doe J. Sepsis in mice. Lancet. 2020;99:e1-e9.",
            "Sepsis in mice",
            "Jane Doe",
            2020,
            ("Medicine", "99", "e1"),
        ),
    ] {
        let mut r = placed(&[], title, &[author], year, at);
        r.work.work_type = Some(crate::work::WorkType::JournalArticle);
        let a = assess(&Claim::from_text(text), &r);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(a.confidence < VERIFIED, "{text}: {a:?}");
    }
    // With no journal read, nor do they count as the journal.
    let mut r = placed(
        &[],
        "Sepsis in mice",
        &["Jane Doe"],
        2020,
        ("Medicine", "99", "e1"),
    );
    r.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let a = assess(&Claim::from_text("Doe J. Sepsis in mice. 2020."), &r);
    assert_eq!(a.container.agreement, Agreement::Unknown, "{a:?}");
}

#[test]
fn structured_and_free_text_journals_compare_alike() {
    let pnas = placed(
        &[],
        "Mitochondrial dysfunction in experimental sepsis",
        &["Jane Doe", "Alan Roe"],
        2011,
        (
            "Proceedings of the National Academy of Sciences",
            "108",
            "1000-1005",
        ),
    );
    let journal = "Proc Natl Acad Sci U S A";
    let structured = Claim {
        title: Some("Mitochondrial dysfunction in experimental sepsis".into()),
        authors: vec!["Doe J".into(), "Roe A".into()],
        year: Some(2010),
        container: Some(journal.into()),
        ..Claim::default()
    };
    let text = Claim::from_text(format!(
        "Doe J, Roe A. Mitochondrial dysfunction in experimental sepsis. {journal}. 2010."
    ));
    for claim in [structured, text] {
        let a = assess(&claim, &pnas);
        assert_ne!(a.container.agreement, Agreement::Mismatch, "{claim:?}");
        assert!(a.confidence >= VERIFIED, "{claim:?}: {a:?}");
    }
}

#[test]
fn journals_named_otherwise_or_not_at_all_are_no_mismatch() {
    let mut nejm = peitho();
    nejm.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let title = "Fibrinolysis for patients with intermediate-risk pulmonary embolism";
    for journal in [
        "N Engl J Med",
        "New England Journal of Medicine",
        "The New England Journal of Medicine",
        "NEJM",
    ] {
        let text = format!("Meyer G, Vicaut E, et al. {title}. {journal}. 2014;370(15):1402-11.");
        let a = assess(&Claim::from_text(&text), &nejm);
        assert_eq!(a.container.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // "Lancet" for "The Lancet", and back.
    let mut lancet = nejm.clone();
    lancet.work.container_title = Some("The Lancet".into());
    let text = format!("Meyer G, et al. {title}. Lancet. 2014;370:1402-11.");
    let a = assess(&Claim::from_text(&text), &lancet);
    assert_eq!(a.container.agreement, Agreement::Match);
    lancet.work.container_title = Some("Lancet".into());
    let text = format!("Meyer G, et al. {title}. The Lancet, 370(15), 1402–1411.");
    assert_eq!(
        assess(&Claim::from_text(&text), &lancet)
            .container
            .agreement,
        Agreement::Match
    );
    // No journal in the reference.
    let text = format!("Meyer G, et al. {title}. 2014.");
    let a = assess(&Claim::from_text(&text), &nejm);
    assert_eq!(a.container.agreement, Agreement::Unknown);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // A book with its publisher, in a book series: no journal to differ.
    let mut book = record(
        &["isbn:9780262035613"],
        "Deep Learning",
        &["Ian Goodfellow", "Yoshua Bengio", "Aaron Courville"],
        2016,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    book.work.container_title = Some("Adaptive Computation and Machine Learning".into());
    for text in [
        "Goodfellow I, Bengio Y, Courville A. Deep Learning. MIT Press; 2016.",
        "Goodfellow I, Bengio Y, Courville A. Deep Learning. Cambridge, MA: MIT Press; 2016:1-800.",
    ] {
        let a = assess(&Claim::from_text(text), &book);
        assert_eq!(a.container.agreement, Agreement::Unknown, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // Not a journal article: proceedings go by many names.
    let mut paper = nejm.clone();
    paper.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    let text = format!("Meyer G, et al. {title}. Lancet. 2014;370:1402-11.");
    let a = assess(&Claim::from_text(&text), &paper);
    assert_eq!(a.container.agreement, Agreement::Unknown);
}

#[test]
fn a_journal_after_a_capitalised_title_end_is_read() {
    // Red team (#102): "… Wuhan, China. Lancet." read as a dotted name
    // that may have begun earlier, so no journal, and the wrong one passed.
    let mut wang = placed(
        &[],
        "Clinical Characteristics of 138 Hospitalized Patients With 2019 Novel \
         Coronavirus–Infected Pneumonia in Wuhan, China",
        &["Dawei Wang", "Bo Hu", "Chang Hu"],
        2020,
        ("JAMA", "323", "1061-1069"),
    );
    wang.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Wang D, Hu B, Hu C, et al. Clinical characteristics of 138 hospitalized patients \
                with 2019 novel coronavirus-infected pneumonia in Wuhan, China. Lancet. \
                2020;323(11):1061-1069.";
    let a = assess(&Claim::from_text(text), &wang);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    let a = assess(&Claim::from_text(text.replace("Lancet", "JAMA")), &wang);
    assert_eq!(a.container.agreement, Agreement::Match, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_journal_cited_without_volume_and_page_is_read() {
    // Red team (#102): "… JAMA. 2020." gave no journal, so the wrong one
    // passed; with the year one off as well, two small errors verified.
    let lstm = {
        let mut r = placed(
            &[],
            "Long Short-Term Memory",
            &["Sepp Hochreiter", "Jürgen Schmidhuber"],
            1997,
            ("Neural Computation", "9", "1735-1780"),
        );
        r.work.work_type = Some(crate::work::WorkType::JournalArticle);
        r
    };
    for text in [
        "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1997.",
        "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1998.",
        "S. Hochreiter and J. Schmidhuber, \"Long short-term memory,\" Neural Networks, vol. 9, \
         1997.",
        "Hochreiter, S. and Schmidhuber, J. (1997) 'Long short-term memory', Neural Networks.",
    ] {
        let a = assess(&Claim::from_text(text), &lstm);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    for text in [
        "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Comput. 1997.",
        "S. Hochreiter and J. Schmidhuber, \"Long short-term memory,\" Neural Computation, vol. 9, \
         1997.",
    ] {
        let a = assess(&Claim::from_text(text), &lstm);
        assert_eq!(a.container.agreement, Agreement::Match, "{text}");
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // A title in title case before the year is the title, not a journal.
    let mut gravitons = placed(
        &[],
        "Hot Gravitons From Kerr Black Holes",
        &["Dan Hooper", "Gordan Krnjaic"],
        2020,
        ("Physical Review D", "101", "1"),
    );
    gravitons.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let text = "Hooper D, Krnjaic G. Hot Gravitons From Kerr Black Holes. 2020.";
    let a = assess(&Claim::from_text(text), &gravitons);
    assert_eq!(a.container.agreement, Agreement::Unknown, "{a:?}");
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_copy_without_journal_or_place_cannot_confirm_the_journal_cited() {
    // Red team (#102): a Zenodo deposit of Ioannidis's essay, no journal,
    // volume or page, verified a reference to the article in another
    // journal.
    let mut copy = record(
        &["10.5281/zenodo.13525448"],
        "Why Most Published Research Findings Are False",
        &["John P. A. Ioannidis"],
        2005,
    );
    copy.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for text in [
        "Ioannidis JP. Why most published research findings are false. BMJ. 2005.",
        "Ioannidis JP. Why most published research findings are false. PLoS Med. \
         2005;2(8):e124.",
    ] {
        let a = assess(&Claim::from_text(text), &copy);
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // Unless the claim's identifier names it, or no journal is cited.
    for text in [
        "Ioannidis JP. Why most published research findings are false. Zenodo. 2005. \
         doi:10.5281/zenodo.13525448",
        "Ioannidis JP. Why most published research findings are false. 2005.",
    ] {
        let a = assess(&Claim::from_text(text), &copy);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
    // A preprint has no journal to give.
    let mut preprint = copy.clone();
    preprint.work.work_type = Some(crate::work::WorkType::Preprint);
    let text = "Ioannidis JP. Why most published research findings are false. medRxiv. 2005.";
    assert!(assess(&Claim::from_text(text), &preprint).confidence >= VERIFIED);
    // Nor does a record in another series, without volume or page, confirm
    // a journal, volume and page all cited (PsycEXTRA's record of a paper
    // Science printed).
    let mut psycextra = record(
        &["10.1037/e301492005-001"],
        "Judgment Under Uncertainty: Heuristics and Biases",
        &["Amos Tversky", "Daniel Kahneman"],
        1974,
    );
    psycextra.work.work_type = Some(crate::work::WorkType::Dataset);
    psycextra.work.container_title = Some("PsycEXTRA Dataset".into());
    let text = "Tversky A, Kahneman D. Judgment under uncertainty: heuristics and biases. Science. \
                1974;185:1124.";
    let a = assess(&Claim::from_text(text), &psycextra);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn bibtex_harvard_and_single_page_locators_are_checked() {
    // Red team (#102): the wrong numbers went unread in these styles.
    let bibtex = |volume: &str| {
        format!(
            "@article{{l, author = {{LeCun, Yann and Bengio, Yoshua}}, title = {{Deep learning}}, \
             journal = {{Nature}}, year = {{2015}}, volume = {{{volume}}}, pages = {{436--444}}}}"
        )
    };
    for (text, right) in [
        (bibtex("521"), true),
        (bibtex("597"), false),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 436 (2015).".to_owned(),
            true,
        ),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 999 (2015).".to_owned(),
            false,
        ),
        (
            "LeCun, Y. et al. (2015) 'Deep learning', Nature, 521, pp. 436-444.".to_owned(),
            true,
        ),
        (
            "LeCun, Y. et al. (2015) 'Deep learning', Science, 521, pp. 436-444.".to_owned(),
            false,
        ),
    ] {
        let a = assess(&Claim::from_text(&text), &lecun());
        if right {
            assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
        } else {
            assert!(
                (PLAUSIBLE..VERIFIED).contains(&a.confidence),
                "{text}: {a:?}"
            );
        }
    }
}

#[test]
fn a_journal_read_right_after_the_title_is_whole() {
    // holdout-5's `w:sister-journal` (#750, #928, #120): a name read from
    // text may be the record's cut short ("Energy" of "… Applied Energy"),
    // but not one read right after the work's title: "Hand Therapy" names
    // a journal of its own, not the Journal of Hand Therapy.
    let hand = article(
        "Rigid versus semi-rigid orthotic use following TMC arthroplasty: a randomized \
         controlled trial",
        &["Ruth Prosser", "Mark J. Hancock", "Leslie Nicholson"],
        2014,
        ("Journal of Hand Therapy", "27", "265"),
    );
    let ijerph = article(
        "An Integration Method for Regional PM2.5 Pollution Control Optimization Based on \
         Meta-Analysis and Systematic Review",
        &["Bingkui Qiu", "Min Zhou", "Yang Qiu"],
        2021,
        (
            "International Journal of Environmental Research and Public Health",
            "19",
            "344",
        ),
    );
    for (claim, record) in [
        (
            "Prosser R, Hancock MJ, Nicholson L. Rigid versus semi-rigid orthotic use following \
             TMC arthroplasty: a randomized controlled trial. Hand Therapy. 2014;27(4):265-70.",
            &hand,
        ),
        (
            "Qiu B, Zhou M, Qiu Y, et al. An Integration Method for Regional PM2.5 Pollution \
             Control Optimization Based on Meta-Analysis and Systematic Review. Healthcare. \
             2021;19(1):344.",
            &ijerph,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{claim}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{claim}: {a:?}"
        );
    }
    // Its own name and abbreviation verify.
    for journal in ["Journal of Hand Therapy", "J Hand Ther", "J. Hand Ther."] {
        let claim = format!(
            "Prosser R, Hancock MJ, Nicholson L. Rigid versus semi-rigid orthotic use following \
             TMC arthroplasty: a randomized controlled trial. {journal} 2014;27(4):265-70."
        );
        let a = assess(&Claim::from_text(claim.as_str()), &hand);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
    // So does a name without its society's acronym (holdout-5 #659's).
    let bat = article(
        "A bat algorithm for SDN network scheduling",
        &["Rongheng Lin", "Zezhou Ye"],
        2018,
        (
            "EURASIP Journal on Wireless Communications and Networking",
            "2018",
            "120",
        ),
    );
    let claim = "Lin R, Ye Z. A bat algorithm for SDN network scheduling. J Wireless Com Network. \
                 2018;2018(1):120.";
    let a = assess(&Claim::from_text(claim), &bat);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_sister_journal_cited_is_another_journal() {
    // holdout-4's `w:sister-journal`: the family's parent cited for a
    // sister's article, or a sister for the parent's.
    let oncology = article(
        "Cognitive Impairment Among Older Patients With Hematologic Cancers",
        &["Tetsuya Tanimoto", "Takahiro Morita", "Kenzo Tsuda"],
        2018,
        ("JAMA Oncology", "4", "1783"),
    );
    let jama = article(
        "Study: Exercise May Match Medication in Reducing Mortality Associated With \
         Cardiovascular Disease, Diabetes",
        &["Mike Mitka"],
        2013,
        ("JAMA", "310", "2026"),
    );
    let cell = article(
        "GPAT2 is required for root cap cuticle formation in young Arabidopsis seedlings",
        &["Alice Berhin", "Aurore Guerault", "Damien De Bellis"],
        2025,
        ("Cell Reports", "44", "116686"),
    );
    for (claim, record) in [
        (
            "Tanimoto T, Morita T, Tsuda K. Cognitive Impairment Among Older Patients With \
             Hematologic Cancers. JAMA. 2018;4(12):1783.",
            &oncology,
        ),
        (
            "Mitka, M. Study: Exercise May Match Medication in Reducing Mortality Associated \
             With Cardiovascular Disease, Diabetes. JAMA Intern Med 310, 2026 (2013).",
            &jama,
        ),
        (
            "Berhin A, Guerault A, De Bellis D. GPAT2 is required for root cap cuticle \
             formation in young Arabidopsis seedlings. Cell. 2025;44:116686.",
            &cell,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{claim}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{claim}: {a:?}"
        );
    }
    // Structured, the parent for its sister.
    let claim = Claim {
        title: Some("Cognitive Impairment Among Older Patients With Hematologic Cancers".into()),
        authors: vec!["Tanimoto, T.".into()],
        year: Some(2018),
        container: Some("JAMA".into()),
        ..Claim::default()
    };
    let a = assess(&claim, &oncology);
    assert_eq!(a.container.agreement, Agreement::Mismatch);
    assert!(a.confidence < VERIFIED, "{a:?}");
    // Its own abbreviation, or its name with its subtitle, verifies.
    for (claim, record) in [
        (
            "Tanimoto T, Morita T, Tsuda K. Cognitive Impairment Among Older Patients With \
             Hematologic Cancers. JAMA Oncol. 2018;4(12):1783.",
            &oncology,
        ),
        (
            "Mitka, M. Study: Exercise May Match Medication in Reducing Mortality Associated \
             With Cardiovascular Disease, Diabetes. JAMA: The Journal of the American Medical \
             Association 310, 2026 (2013).",
            &jama,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
}
