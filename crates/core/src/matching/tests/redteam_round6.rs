//! Round 6 of the red team (#126): `bench/redteam/holdout-5.jsonl` and
//! `wild-2.jsonl`, minimised against the records they matched.

use super::*;

/// Cases that must not verify: (label, claim, record).
fn none_verify(cases: &[(&str, &str, &SourceRecord)]) {
    let failing: Vec<String> = cases
        .iter()
        .filter_map(|(id, claim, record)| {
            let a = assess(&Claim::from_text(*claim), record);
            (a.confidence >= VERIFIED).then(|| format!("{id}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
}

/// Cases that must verify.
fn all_verify(cases: &[(&str, &str, &SourceRecord)]) {
    let failing: Vec<String> = cases
        .iter()
        .filter_map(|(id, claim, record)| {
            let a = assess(&Claim::from_text(*claim), record);
            (a.confidence < VERIFIED).then(|| format!("{id}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
}

#[test]
fn another_meeting_of_the_same_society_is_another_venue() {
    // holdout-5: ACM papers cited at another ACM meeting, no DOI.
    let icmr = proceedings(
        "Learning From Expert: Vision-Language Knowledge Distillation for Unsupervised \
         Cross-Modal Hashing Retrieval",
        &["Lina Sun", "Yewen Li", "Yumin Dong"],
        2023,
        "Proceedings of the 2023 ACM International Conference on Multimedia Retrieval",
        "499-507",
    );
    let cikm = proceedings(
        "Improve Network Embeddings with Regularization",
        &["Yi Zhang", "Jianguo Lu", "Ofer Shai"],
        2018,
        "Proceedings of the 27th ACM International Conference on Information and Knowledge \
         Management",
        "1643-1646",
    );
    let stoc = proceedings(
        "Almost-Linear Time Parameterized Algorithm for Rankwidth via Dynamic Rankwidth",
        &["Tuukka Korhonen", "Marek Sokołowski"],
        2024,
        "Proceedings of the 56th Annual ACM Symposium on Theory of Computing",
        "1538-1549",
    );
    let sigcomm = proceedings(
        "Compressing IP forwarding tables: towards entropy bounds and beyond",
        &["Gábor Rétvári", "János Tapolcai", "Attila Kőrösi"],
        2013,
        "ACM SIGCOMM Conference (SIGCOMM)",
        "111-122",
    );
    let imc = proceedings(
        "Measuring Starlink Aviation Around the World",
        &["Jinwei Zhao", "Jianping Pan"],
        2026,
        "Proceedings of the 2026 ACM Internet Measurement Conference",
        "1118-1132",
    );
    let idc = proceedings(
        "Towards Trustworthy Conversational Agents for Children",
        &["Marina Escobar-Planas"],
        2022,
        "Interaction Design and Children",
        "693-695",
    );
    let recsys = proceedings(
        "Team recommendation in open innovation networks",
        &["Michele Brocco", "Georg Groh"],
        2009,
        "Proceedings of the third ACM conference on Recommender systems",
        "365-368",
    );
    none_verify(&[
        (
            "h5-71",
            "L. Sun, Y. Li, and Y. Dong, “Learning From Expert: Vision-Language Knowledge \
             Distillation for Unsupervised Cross-Modal Hashing Retrieval,” in Proceedings of \
             the 2023 ACM International Conference on Multimedia (MM '23), 2023, pp. 499–507.",
            &icmr,
        ),
        (
            "h5-183",
            "@inproceedings{zhang2018, author = {Zhang, Yi and Lu, Jianguo and Shai, Ofer}, \
             title = {Improve Network Embeddings with Regularization}, booktitle = \
             {Proceedings of the 2018 ACM Web Conference (WWW '18)}, year = {2018}, pages = \
             {1643--1646}}",
            &cikm,
        ),
        (
            "h5-287",
            "@inproceedings{korhonen2024, author = {Korhonen, Tuukka and Sokołowski, Marek}, \
             title = {Almost-Linear Time Parameterized Algorithm for Rankwidth via Dynamic \
             Rankwidth}, booktitle = {Proceedings of the 2024 ACM Symposium on Principles of \
             Distributed Computing (PODC '24)}, year = {2024}, pages = {1538--1549}}",
            &stoc,
        ),
        (
            "h5-571",
            "@inproceedings{retvari2013, author = {Rétvári, Gábor and Tapolcai, János and \
             Kőrösi, Attila}, title = {Compressing IP forwarding tables: towards entropy bounds \
             and beyond}, booktitle = {Proceedings of the 2013 International Conference on \
             emerging Networking EXperiments and Technologies (CoNEXT '13)}, year = {2013}, \
             pages = {111--122}}",
            &sigcomm,
        ),
        (
            "h5-1070",
            "Escobar-Planas, M. (2022). Towards Trustworthy Conversational Agents for Children. \
             In Proceedings of the 2022 ACM Designing Interactive Systems Conference (DIS '22) \
             (pp. 693–695).",
            &idc,
        ),
        (
            "h5-1077",
            "M. Brocco and G. Groh, “Team recommendation in open innovation networks,” in \
             Proceedings of the 2009 ACM Web Conference (WWW '09), 2009, pp. 365–368.",
            &recsys,
        ),
    ]);
    // Left: "(SIGCOMM '26)" for an IMC paper (h5-643), as a society's
    // acronym needn't be its meeting's name ("ASSETS").
    // Their own meetings, cited as people cite them, still verify.
    all_verify(&[
        (
            "icmr",
            "L. Sun, Y. Li, and Y. Dong, “Learning From Expert: Vision-Language Knowledge \
             Distillation for Unsupervised Cross-Modal Hashing Retrieval,” in Proceedings of \
             the 2023 ACM International Conference on Multimedia Retrieval (ICMR '23), 2023, \
             pp. 499–507.",
            &icmr,
        ),
        (
            "cikm",
            "@inproceedings{zhang2018, author = {Zhang, Yi and Lu, Jianguo and Shai, Ofer}, \
             title = {Improve Network Embeddings with Regularization}, booktitle = \
             {Proceedings of the 27th ACM International Conference on Information and \
             Knowledge Management (CIKM '18)}, year = {2018}, pages = {1643--1646}}",
            &cikm,
        ),
        (
            "stoc",
            "@inproceedings{korhonen2024, author = {Korhonen, Tuukka and Sokołowski, Marek}, \
             title = {Almost-Linear Time Parameterized Algorithm for Rankwidth via Dynamic \
             Rankwidth}, booktitle = {Proceedings of the 56th Annual ACM Symposium on Theory \
             of Computing (STOC '24)}, year = {2024}, pages = {1538--1549}}",
            &stoc,
        ),
        (
            "sigcomm",
            "@inproceedings{retvari2013, author = {Rétvári, Gábor and Tapolcai, János and \
             Kőrösi, Attila}, title = {Compressing IP forwarding tables: towards entropy bounds \
             and beyond}, booktitle = {Proceedings of the ACM SIGCOMM 2013 Conference \
             (SIGCOMM '13)}, year = {2013}, pages = {111--122}}",
            &sigcomm,
        ),
        (
            "imc",
            "J. Zhao and J. Pan, “Measuring Starlink Aviation Around the World,” in Proceedings \
             of the 2026 ACM Internet Measurement Conference (IMC '26), 2026, pp. 1118–1132.",
            &imc,
        ),
        (
            "idc",
            "Escobar-Planas, M. (2022). Towards Trustworthy Conversational Agents for Children. \
             In Proceedings of the 21st Annual ACM Interaction Design and Children Conference \
             (IDC '22) (pp. 693–695).",
            &idc,
        ),
        (
            "recsys",
            "M. Brocco and G. Groh, “Team recommendation in open innovation networks,” in \
             Proceedings of the third ACM Conference on Recommender Systems (RecSys '09), 2009, \
             pp. 365–368.",
            &recsys,
        ),
    ]);
}

#[test]
fn a_journal_article_cited_in_proceedings_is_another_venue() {
    // h5-571: the SIGCOMM paper's copy in Computer Communication Review,
    // cited by BibTeX at CoNEXT; h5-658: an ICSE paper's journal version,
    // its title cited otherwise, at ESEC/FSE.
    let ccr = article(
        "Compressing IP forwarding tables: towards entropy bounds and beyond",
        &["Gábor Rétvári", "János Tapolcai", "Attila Kőrösi"],
        2013,
        ("ACM SIGCOMM Computer Communication Review", "43", "111-122"),
    );
    let spe = article(
        "Practical static analysis of context leaks in Android applications",
        &["Flavio Toffalini", "Jun Sun", "Martín Ochoa"],
        2018,
        ("Software: Practice and Experience", "49", "233-251"),
    );
    none_verify(&[
        (
            "h5-571",
            "@inproceedings{retvari2013, author = {Rétvári, Gábor and Tapolcai, János and \
             Kőrösi, Attila}, title = {Compressing IP forwarding tables: towards entropy bounds \
             and beyond}, booktitle = {Proceedings of the 2013 International Conference on \
             emerging Networking EXperiments and Technologies (CoNEXT '13)}, year = {2013}, \
             pages = {111--122}}",
            &ccr,
        ),
        (
            "h5-658",
            "Toffalini, F., Sun, J., & Ochoa, M. (2018). Static analysis of context leaks in \
             android applications. In Proceedings of the 2018 ACM Joint European Software \
             Engineering Conference and Symposium on the Foundations of Software Engineering \
             (ESEC/FSE '18) (pp. 215–224).",
            &spe,
        ),
    ]);
    all_verify(&[(
        "ccr",
        "@article{retvari2013, author = {Rétvári, Gábor and Tapolcai, János and Kőrösi, \
         Attila}, title = {Compressing IP forwarding tables: towards entropy bounds and beyond}, \
         journal = {ACM SIGCOMM Computer Communication Review}, volume = {43}, year = {2013}, \
         pages = {111--122}}",
        &ccr,
    )]);
}

#[test]
fn a_title_reworded_is_plausible_unless_the_id_or_place_says() {
    // holdout-5: journal articles cited as their preprints, under the
    // journal version's title (the preprint's record found).
    let mut research_square = record(
        &["doi:10.21203/rs.3.rs-3762213/v1"],
        "Preliminary adaptation process of cognitive behavior therapy for major depression in \
         Bangladesh: A pilot study",
        &["Jesan Ara", "Farah Deeba"],
        2023,
    );
    research_square.work.work_type = Some(crate::work::WorkType::Preprint);
    let mut preprints = record(
        &["doi:10.20944/preprints201711.0028.v1"],
        "Genetic Variation of Nitrogen Use Traits Using Maize Expired Plant Variety Protection \
         Germplasm",
        &[
            "Adriano T. Mastrodomenico",
            "C. Cole Hendrix",
            "Frederick E. Below",
        ],
        2017,
    );
    preprints.work.work_type = Some(crate::work::WorkType::Preprint);
    let mut hpv = record(
        &["doi:10.21203/rs.3.rs-17369/v1"],
        "Multiple HPV infection as possible marker of neoplastic progression",
        &[
            "Maria Teresa Bruno",
            "Guido Scalia",
            "Nazario Cassaro",
            "Sara Boemi",
        ],
        2020,
    );
    hpv.work.work_type = Some(crate::work::WorkType::Preprint);
    none_verify(&[
        (
            "h5-811",
            "Bruno MT, Scalia G, Cassaro N, Boemi S. Multiple HPV 16 infection with two \
             strains: a possible marker of neoplastic progression. Research Square [Preprint]. \
             2020.",
            &hpv,
        ),
        (
            "h5-178",
            "Ara J, Deeba F. Preliminary adaptation of cognitive behavioral therapy for major \
             depression in the Bangladeshi context: a pilot study. Research Square [Preprint]. \
             2023.",
            &research_square,
        ),
        (
            "h5-775",
            "Mastrodomenico, A., Hendrix, C., & Below, F. (2017). Nitrogen Use Efficiency and \
             the Genetic Variation of Maize Expired Plant Variety Protection Germplasm. \
             Preprints.",
            &preprints,
        ),
    ]);
    // A misspelt word, or a British spelling, is the same title.
    let thrun = article(
        "Bayesian Landmark Learning for Mobile Robot Localization",
        &["Sebastian Thrun"],
        1998,
        ("Machine Learning", "33", "41-76"),
    );
    let xi = proceedings(
        "Propensity Score Alignment of Unpaired Multimodal Data",
        &["Johnny Xi", "Jana Osea", "Zuheng Xu", "Jason Hartford"],
        2024,
        "Advances in Neural Information Processing Systems",
        "",
    );
    all_verify(&[
        (
            "h5-352",
            "Thrun, S. (1998) 'Bayesian Landmark Learning for Mobile Robot Loaclization', \
             Machine Learning, 33(1), pp. 41-76.",
            &thrun,
        ),
        (
            "h5-352, unplaced",
            "Thrun, S. (1998) 'Bayesian Landmark Learning for Mobile Robot Loaclization', \
             Machine Learning.",
            &thrun,
        ),
        (
            "h5-20",
            "@inproceedings{xi2024, author = {Xi, Johnny and Osea, Jana and Xu, Zuheng and \
             Hartford, Jason}, title = {Propensity Score Ailgnment of Unpaired Multimodal Data}, \
             booktitle = {Advances in Neural Information Processing Systems}, year = {2024}}",
            &xi,
        ),
    ]);
}

#[test]
fn an_arxiv_number_holds_no_year() {
    // wild-2 #641, #644, #645: "1811.07871" isn't 1811, nor "2009.01325"
    // 2009.
    for (text, years) in [
        (
            "Leike et al., Scalable Agent Alignment via Reward Modeling | 1811.07871 | \
             Preprint (DeepMind)",
            vec![],
        ),
        (
            "Stiennon et al., Learning to Summarize from Human Feedback | 2009.01325 | \
             NeurIPS 2020",
            vec![2020],
        ),
        (
            "Vaswani A, et al. Attention is all you need. arXiv:1706.03762. 2017.",
            vec![2017],
        ),
        // A year ending a sentence before a number is still a year.
        (
            "Smith J. A study of things. Lancet. 2009. 12345",
            vec![2009],
        ),
    ] {
        assert_eq!(Claim::from_text(text).cited_years(), years, "{text}");
    }
}

#[test]
fn a_bare_arxiv_number_standing_as_a_references_identifier_is_one() {
    // wild-2: model-written references giving the arXiv number alone,
    // after a colon, as a table's cell, or last.
    let arxiv = |id: &str| -> Vec<WorkId> { vec![format!("arxiv:{id}").parse().unwrap()] };
    for (text, ids) in [
        (
            "Kosinski, Theory of Mind May Have Spontaneously Emerged in LLMs (later retitled \
             Evaluating LLMs in Theory of Mind Tasks, PNAS 2024): 2302.02083",
            arxiv("2302.02083"),
        ),
        (
            "Chan et al., NegotiationToM: 2404.13627",
            arxiv("2404.13627"),
        ),
        (
            "Christiano et al., Deep RL from Human Preferences | 1706.03741 | NeurIPS 2017",
            arxiv("1706.03741"),
        ),
        (
            "Sap et al.'s Neural Theory-of-Mind? (2210.13312)",
            arxiv("2210.13312"),
        ),
        (
            "RAGTruth: A Hallucination Corpus for Trustworthy RAG (2024) | 2401.00396",
            arxiv("2401.00396"),
        ),
        (
            "Vaswani A, et al. Attention is all you need. 1706.03762v5.",
            arxiv("1706.03762v5"),
        ),
    ] {
        assert_eq!(Claim::from_text(text).ids, ids, "{text}");
    }
    // Not in prose, nor beside another identifier, nor two of them, nor a
    // number that isn't one (no month 13).
    for text in [
        "We trained on 1706.03762 examples and report the mean.",
        "Smith J. A paper. 2017. doi:10.1000/xyz123 | 1706.03762",
        "Two papers: 1706.03762 and 1810.04805",
        "Smith J. A paper. Results: 1713.03762",
        "Smith J. Gene expression in mice. Nature. 2016;533:1706.03762",
        "1706.03762",
    ] {
        let ids = Claim::from_text(text).ids;
        assert!(
            ids.iter().all(|id| id.kind() != crate::id::IdKind::Arxiv),
            "{text}: {ids:?}"
        );
    }
}

#[test]
fn a_titles_words_abbreviated_are_its_words() {
    // wild-2: models abbreviate titles' words ("LLMs", "ToM", "RAG",
    // "RLHF"), and a title's acronym ("Advanced RAG") is no byline.
    let mut found = 0;
    for (claim, title, authors, year, bar) in [
        (
            "van Duijn et al., Theory of Mind in LLMs: 11 Models vs. Children Aged 7-10 on \
             Advanced Tests: 2310.20320",
            "Theory of Mind in Large Language Models: Examining Performance of 11 State-of-the-Art \
             models vs. Children Aged 7-10 on Advanced Tests",
            &["Max J. van Duijn", "Bram van Dijk", "Tom Kouwenhoven"][..],
            2023,
            // Words dropped from its middle too: plausible.
            PLAUSIBLE,
        ),
        (
            "Li et al., Theory of Mind for Multi-Agent Collaboration via LLMs: 2310.10701",
            "Theory of Mind for Multi-Agent Collaboration via Large Language Models",
            &["Huao Li", "Yu Quan Chong", "Simon Stepputtis"][..],
            2023,
            VERIFIED,
        ),
        (
            "Casper et al., Open Problems and Fundamental Limitations of RLHF | 2307.15217 | \
             TMLR 2023",
            "Open Problems and Fundamental Limitations of Reinforcement Learning from Human \
             Feedback",
            &["Stephen Casper", "Xander Davies", "Claudia Shi"][..],
            2023,
            VERIFIED,
        ),
        (
            "Trott et al.'s Do LLMs Know What Humans Know? (2209.01515)",
            "Do Large Language Models Know What Humans Know?",
            &["Sean Trott", "Cameron Jones", "Tyler Chang"][..],
            2023,
            VERIFIED,
        ),
        (
            "ARAGOG: Advanced RAG Output Grading (2024) | 2404.01037",
            "ARAGOG: Advanced RAG Output Grading",
            &["Matouš Eibich", "Shivay Nagpal", "Alexander Fred-Ojala"][..],
            2024,
            VERIFIED,
        ),
        (
            "Zhou et al., How FaR Are LLMs From Agents with Theory-of-Mind?: 2310.03051",
            "How FaR Are Large Language Models From Agents with Theory-of-Mind?",
            &["Pei Zhou", "Aman Madaan", "Srividya Pranavi Potharaju"][..],
            2023,
            VERIFIED,
        ),
    ] {
        let claim = Claim::from_text(claim);
        let id = claim.ids[0].to_string();
        let r = arxiv_preprint(&id, title, authors, year);
        let a = assess(&claim, &r);
        assert!(a.confidence >= bar, "{title}: {a:?}");
        found += 1;
    }
    assert_eq!(found, 6);
    // An acronym the title's words don't spell is another word.
    let r = arxiv_preprint(
        "arxiv:2401.00396",
        "RAGTruth: A Hallucination Corpus for Developing Trustworthy Retrieval-Augmented \
         Language Models",
        &["Cheng Niu", "Yuanhao Wu", "Juno Zhu"],
        2024,
    );
    let a = assess(
        &Claim::from_text("RAGTruth: A Hallucination Corpus for Trustworthy RAG (2024)"),
        &r,
    );
    assert!(a.confidence < VERIFIED, "{a:?}");
}

#[test]
fn initials_and_parts_of_words_are_no_acronyms() {
    // h5-1106: "Kishore CP" isn't "chronic peripheral"; h5-380:
    // "DS-K3DOM" holds no "Dynamic Occupancy Mapping".
    let title = "Effectiveness of vestibular rehabilitation therapy and yoga in the management \
                 of chronic peripheral vertigo: A randomized controlled trial";
    assert_eq!(
        super::super::titles::expand_acronyms(
            "Vaishali K, Kishore CP, Sampath CPR. Effectiveness …",
            title
        ),
        None
    );
    let title = "DS-K3DOM: 3-D Dynamic Occupancy Mapping with Kernel Inference and \
                 Dempster-Shafer Evidential Theory";
    assert_eq!(
        super::super::titles::expand_acronyms("DS-K3DOM: 3-D Dynamic Occupancy Mapping", title),
        None
    );
    assert_eq!(
        super::super::titles::expand_acronyms(
            "Do LLMs Know What Humans Know?",
            "Do Large Language Models Know What Humans Know?"
        )
        .as_deref(),
        Some("Do Large Language Models Know What Humans Know?")
    );
}

#[test]
fn an_article_reprinted_later_as_a_chapter_is_another_version() {
    // wild-2 #394, #834: the free sources hold only the reprint.
    let mut crone = chapter(
        "Islam, Judeo-Christianity and Byzantine Iconoclasm",
        &["Patricia Crone"],
        2017,
        "Arab-Byzantine Relations in Early Islamic Times",
    );
    crone.work.page = Some("361-397".into());
    let kuznets = chapter(
        "Economic Growth and Income Inequality",
        &["Simon Kuznets"],
        2019,
        "Routledge eBooks",
    );
    for (claim, record) in [
        (
            "Crone, Patricia. \"Islam, Judeo-Christianity and Byzantine Iconoclasm.\" Jerusalem \
             Studies in Arabic and Islam 2 (1980): 59–95.",
            &crone,
        ),
        (
            "Kuznets, S. (1955) 'Economic growth and income inequality', The American Economic \
             Review, 45(1), pp. 1–28.",
            &kuznets,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{claim}: {a:?}"
        );
    }
    // Cited after the chapter's year, it's no reprint of the article cited.
    let a = assess(
        &Claim::from_text(
            "Kuznets, S. (2023) 'Economic growth and income inequality', The American Economic \
             Review, 45(1), pp. 1–28.",
        ),
        &kuznets,
    );
    assert!(a.confidence < PLAUSIBLE, "{a:?}");
}

#[test]
fn a_books_title_is_read_from_its_references_shape() {
    // wild-2's monographs: Harvard's year before the title without a
    // stop, and bylines written "Family, Given," before a title, place and
    // publisher.
    for (text, title) in [
        (
            "Blackburn, M.A.S. (2011) Viking coinage and currency in the British Isles. British \
             Numismatic Society Special Publication no. 7. London: British Numismatic Society.",
            "Viking coinage and currency in the British Isles",
        ),
        (
            "Vidal-Naquet, Pierre, La torture dans la République, Paris, Minuit, 1972 (1re éd. \
             Maspero, 1972).",
            "La torture dans la République",
        ),
        (
            "Ageron, Charles-Robert, Histoire de l'Algérie contemporaine, Paris, PUF, coll. « Que \
             sais-je ? » (1re éd. 1964, nombreuses rééditions).",
            "Histoire de l'Algérie contemporaine",
        ),
        (
            "Einaudi, Jean-Luc, La bataille de Paris. 17 octobre 1961, Paris, Seuil, 1991.",
            "La bataille de Paris",
        ),
        (
            "Rioux, Jean-Pierre (dir.), La guerre d'Algérie et les Français, Paris, Fayard, 1990.",
            "La guerre d'Algérie et les Français",
        ),
        (
            "Kuhn, Thomas S., The Structure of Scientific Revolutions, Chicago, University of \
             Chicago Press, 1962.",
            "The Structure of Scientific Revolutions",
        ),
        // Two authors, the second's name after "and": the sentence after.
        (
            "Ragep, F. Jamil, and Sally P. Ragep, eds. Tradition, Transmission, Transformation. \
             Leiden: Brill, 1996.",
            "Tradition, Transmission, Transformation",
        ),
        (
            "Noble, Thomas F. X. Images, Iconoclasm, and the Carolingians. Philadelphia: \
             University of Pennsylvania Press, 2009.",
            "Images, Iconoclasm, and the Carolingians",
        ),
    ] {
        assert_eq!(probable_title(text).as_deref(), Some(title), "{text}");
    }
}

#[test]
fn a_byline_in_capitals_or_of_family_names_is_complete() {
    // holdout-5: invented co-authors beside records whose names are in
    // capitals ("JOHN R. JAENIKE") or family names alone ("McCarthy").
    let jaenike = article(
        "PARASITE PRESSURE AND THE EVOLUTION OF AMANITIN TOLERANCE IN DROSOPHILA",
        &["JOHN R. JAENIKE"],
        1985,
        ("Evolution", "39", "1295"),
    );
    let guessous = article(
        "Genome-wide association studies in pharmacogenomics: untapped potential for translation",
        &["Idris Guessous", "MARTA L. GWINN", "Muin J. Khoury"],
        2009,
        ("Genome Medicine", "1", "46"),
    );
    let mccarthy = article(
        "The setting up and implementation of a venous thromboembolism prophylaxis policy in \
         clinical hospital practice",
        &["McCarthy", "Ged J. Byrne", "Silverman"],
        1998,
        ("Journal of Evaluation in Clinical Practice", "4", "1"),
    );
    none_verify(&[
        (
            "h5-641",
            "Jaenike, J., & Okonjo-Ashdown, N. (1985). Parasite Pressure and the Evolution of \
             Amanitin Tolerance in Drosophila. Evolution, 39(6), 1295.",
            &jaenike,
        ),
        (
            "h5-995",
            "Guessous, I., Kaszuba-Ilunga, K., & Khoury, M. J. (2009). Genome-wide association \
             studies in pharmacogenomics: untapped potential for translation. Genome Medicine, \
             1(4), 46.",
            &guessous,
        ),
        (
            "h5-269",
            "McCarthy MJ, Kaszuba-Whitlock S, Silverman SH. The setting up and implementation of \
             a venous thromboembolism prophylaxis policy in clinical hospital practice. J Eval \
             Clin Pract. 1998;4(1):1.",
            &mccarthy,
        ),
    ]);
    all_verify(&[
        (
            "jaenike",
            "Jaenike, J. (1985). Parasite Pressure and the Evolution of Amanitin Tolerance in \
             Drosophila. Evolution, 39(6), 1295.",
            &jaenike,
        ),
        (
            "guessous",
            "Guessous, I., Gwinn, M., & Khoury, M. J. (2009). Genome-wide association studies in \
             pharmacogenomics: untapped potential for translation. Genome Medicine, 1(4), 46.",
            &guessous,
        ),
        (
            "mccarthy",
            "McCarthy MJ, Byrne GJ, Silverman SH. The setting up and implementation of a venous \
             thromboembolism prophylaxis policy in clinical hospital practice. J Eval Clin Pract. \
             1998;4(1):1.",
            &mccarthy,
        ),
    ]);
}

#[test]
fn an_article_cited_as_a_chapter_of_a_book_named_as_its_journal_is_another_container() {
    // holdout-5 #86, #994: "In: …, editor. Journal of Applied
    // Probability. Wiley-Blackwell; 1994. p. 185-197.", no volume.
    let lagrangian = article(
        "Aspects of Lagrangian probability distributions",
        &["Masaaki Sibuya", "Norihiko Miyawaki", "Ushio Sumita"],
        1994,
        ("Journal of Applied Probability", "31", "185-197"),
    );
    let separation = article(
        "Separation index of a graph",
        &["Andrew Vince"],
        2002,
        ("Journal of Graph Theory", "41", "53-61"),
    );
    none_verify(&[
        (
            "h5-86",
            "Sibuya M, Miyawaki N, Sumita U. Aspects of Lagrangian probability distributions. In: \
             Whitcombe-Pryce E, editor. Journal of Applied Probability. Wiley-Blackwell; 1994. \
             p. 185-197.",
            &lagrangian,
        ),
        (
            "h5-994",
            "Vince, A. (2002). Separation index of a graph. In C. Gudmundsdottir-Lindgren (Ed.), \
             Journal of Graph Theory (pp. 53–61). Oxford University Press.",
            &separation,
        ),
    ]);
}

#[test]
fn a_word_replaced_after_the_main_title_is_another_title() {
    // holdout-5 #976.
    let quixote = article(
        "\"High and Noble Adventures\": Reading the Novel in \"The Female Quixote\"",
        &["Mary Patricia Martin"],
        1997,
        ("NOVEL: A Forum on Fiction", "31", "45"),
    );
    none_verify(&[(
        "h5-976",
        "M. P. Martin, “\"High and Noble Adventures\": Reading the Novel in \"The Male \
         Quixote\",” NOVEL: A Forum on Fiction, vol. 31, no. 1, Art. no. 45, 1997.",
        &quixote,
    )]);
    all_verify(&[(
        "quixote",
        "M. P. Martin, “\"High and Noble Adventures\": Reading the Novel in \"The Female \
         Quixote\",” NOVEL: A Forum on Fiction, vol. 31, no. 1, Art. no. 45, 1997.",
        &quixote,
    )]);
}

#[test]
fn a_supplements_pages_are_read_with_their_volume() {
    // holdout-5 #645: "2009;18:ii5-ii8", an older edition's place.
    let located = publication::locator(
        "Pestalozzi B. Primary breast cancer: ESMO Clinical Recommendations for diagnosis, \
         treatment and follow-up. Annals of Oncology. 2009;18:ii5-ii8.",
    );
    assert_eq!(located.volume.as_deref(), Some("18"));
    assert_eq!(located.page.as_deref(), Some("ii5"));
}

#[test]
fn a_journal_read_with_its_volume_is_one_though_the_title_has_its_word() {
    // holdout-5 #268: "Physiotherapy, 40(8)" for Physiotherapy Theory and
    // Practice, a title about physiotherapy.
    let fors = article(
        "Are illness perceptions and patient self-care enablement mediators of treatment effect \
         in best practice physiotherapy low back pain care? Secondary mediation analyses in the \
         BetterBack trial",
        &["Maria Fors", "Birgitta Öberg", "Paul Enthoven"],
        2023,
        ("Physiotherapy theory and practice", "40", "1753-1766"),
    );
    // holdout-5 #925: an LNCS paper cited as a journal's, with an issue.
    let mut stork = proceedings(
        "Computer Vision and Computer Graphics Analysis of Paintings and Drawings: An \
         Introduction to the Literature",
        &["David G. Stork"],
        2009,
        "Lecture notes in computer science",
        "9-24",
    );
    stork.work.work_type = Some(crate::work::WorkType::ConferencePaper);
    none_verify(&[
        (
            "h5-268",
            "Fors, M. et al. (2024) 'Are illness perceptions and patient self-care enablement \
             mediators of treatment effect in best practice physiotherapy low back pain care? \
             Secondary mediation analyses in the BetterBack trial', Physiotherapy, 40(8), pp. \
             1753-1766.",
            &fors,
        ),
        (
            "h5-925",
            "Stork, D. G. (2009). Computer Vision and Computer Graphics Analysis of Paintings and \
             Drawings: An Introduction to the Literature. Computer Analysis of Images and \
             Patterns, 12(4), 9–24.",
            &stork,
        ),
    ]);
    all_verify(&[(
        "fors",
        "Fors, M. et al. (2023) 'Are illness perceptions and patient self-care enablement \
         mediators of treatment effect in best practice physiotherapy low back pain care? \
         Secondary mediation analyses in the BetterBack trial', Physiotherapy Theory and \
         Practice, 40(8), pp. 1753-1766.",
        &fors,
    )]);
}

#[test]
fn a_record_without_a_byline_confirms_no_names() {
    // holdout-5 #75: every author invented, the record listing none.
    let mut jama = article(
        "New Surgical Approaches to Parkinson Disease",
        &[],
        1999,
        ("JAMA", "282", "1117-1118"),
    );
    jama.work.authors.clear();
    none_verify(&[(
        "h5-75",
        "Ostrowski-Featherstone, L. (1999). New surgical approaches to Parkinson disease. JAMA, \
         282(12), 1117–1118.",
        &jama,
    )]);
    // Cited without a byline, or by an organisation, it verifies.
    all_verify(&[
        (
            "untitled byline",
            "New surgical approaches to Parkinson disease. JAMA. 1999;282(12):1117-1118.",
            &jama,
        ),
        (
            "organisation",
            "American Medical Association. New surgical approaches to Parkinson disease. JAMA. \
             1999;282(12):1117-1118.",
            &jama,
        ),
    ]);
}

#[test]
fn a_dois_words_are_no_title() {
    // wild-2 #776, #782: authors, year, journal and a bare DOI, no title.
    let mut babikova = article(
        "Underground signals carried through common mycelial networks warn neighbouring plants \
         of aphid attack",
        &["Zdenka Babikova", "Lucy Gilbert", "Toby J. A. Bruce"],
        2013,
        ("Ecology Letters", "16", "835-843"),
    );
    babikova.ids = vec!["doi:10.1111/ele.12115".parse().unwrap()];
    let mut henriksson = article(
        "Re-examining the evidence for the mother tree hypothesis - resource sharing among \
         trees via ectomycorrhizal networks",
        &["Nils Henriksson", "John Marshall", "Mona N. Högberg"],
        2023,
        ("New Phytologist", "239", "19-28"),
    );
    henriksson.ids = vec!["doi:10.1111/nph.18935".parse().unwrap()];
    all_verify(&[
        (
            "w2-776",
            "Babikova et al. 2013, Ecology Letters | 10.1111/ele.12115",
            &babikova,
        ),
        (
            "w2-782",
            "Henriksson et al. 2023 (New Phytologist, DOI 10.1111/nph.18935)",
            &henriksson,
        ),
    ]);
}
