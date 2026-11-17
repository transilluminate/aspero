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
