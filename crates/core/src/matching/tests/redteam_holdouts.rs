//! Red-team regressions from the second, third and fourth held-out
//! corpora and the first wild one.

use super::*;

// One table of regression cases, a few lines each.
#[allow(clippy::too_many_lines)]
#[test]
fn second_held_out_red_team_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/holdout-2.jsonl (round
    // 3), against the records they verified: none may verify now. Each
    // with its line there.
    let pix2pix = proceedings(
        "Image-to-Image Translation with Conditional Adversarial Networks",
        &["Phillip Isola", "Jun-Yan Zhu", "Tinghui Zhou"],
        2017,
        "2017 IEEE Conference on Computer Vision and Pattern Recognition (CVPR)",
        "5967-5976",
    );
    let deepwalk = proceedings(
        "DeepWalk: online learning of social representations",
        &["Bryan Perozzi", "Rami Al-Rfou", "Steven Skiena"],
        2014,
        "Proceedings of the 20th ACM SIGKDD international conference on Knowledge discovery \
         and data mining",
        "701-710",
    );
    let tpu = proceedings(
        "In-Datacenter Performance Analysis of a Tensor Processing Unit",
        &["Norman P. Jouppi", "Cliff Young", "Nishant Patil"],
        2017,
        "Proceedings - International Symposium on Computer Architecture/Proceedings",
        "1-12",
    );
    let mut attention = arxiv_preprint(
        "arxiv:1706.03762",
        "Attention Is All You Need",
        &VASWANI,
        2017,
    );
    attention
        .ids
        .push("doi:10.48550/arxiv.1706.03762".parse().unwrap());
    // Its place pooled from the proceedings' record.
    attention.work.volume = Some("30".into());
    let gan = arxiv_preprint(
        "arxiv:1406.2661",
        "Generative Adversarial Networks",
        &["Ian J. Goodfellow", "Jean Pouget-Abadie", "Mehdi Mirza"],
        2014,
    );
    let rmp = article(
        "Odd triplet superconductivity and related phenomena in superconductor-ferromagnet \
         structures",
        &["F. S. Bergeret", "A. F. Volkov", "K. B. Efetov"],
        2005,
        ("Reviews of Modern Physics", "77", "1321-1373"),
    );
    let aer = article(
        "Consumption Disasters in the Twentieth Century",
        &["Robert J. Barro", "José F. Ursúa"],
        2008,
        ("American Economic Review", "98", "58-63"),
    );
    let prb = article(
        "Spin pumping in strongly coupled magnon-photon systems",
        &["H. Maier-Flaig", "Michael Harder", "Rudolf Gross"],
        2016,
        ("Physical Review B", "94", "054433"),
    );
    let nonaka = article(
        "A Dynamic Theory of Organizational Knowledge Creation",
        &["Ikujiro Nonaka"],
        1994,
        ("Organization Science", "5", "14-37"),
    );
    let jones = article(
        "Instrumental Stakeholder Theory: A Synthesis of Ethics and Economics",
        &["Thomas M. Jones"],
        1995,
        ("Academy of Management Review", "20", "404-437"),
    );
    let bar_haim = article(
        "Threat-related attentional bias in anxious and nonanxious individuals: A \
         meta-analytic study.",
        &["Yair Bar-Haim", "Dominique Lamy", "Lee Pergamin"],
        2007,
        ("Psychological Bulletin", "133", "1-24"),
    );
    let wu = article(
        "Risk Factors Associated With Acute Respiratory Distress Syndrome and Death in Patients \
         With Coronavirus Disease 2019 Pneumonia in Wuhan, China",
        &["Chaomin Wu", "Xiaoyan Chen", "Yanping Cai"],
        2020,
        ("JAMA Internal Medicine", "180", "934-943"),
    );
    let hastings = article(
        "Monte Carlo sampling methods using Markov chains and their applications",
        &["W. K. Hastings"],
        1970,
        ("Biometrika", "57", "97-109"),
    );
    let methadone = article(
        "Methadone maintenance treatment and mortality in people with criminal convictions: A \
         population-based retrospective cohort study from Canada",
        &["Angela Russolillo", "Akm Moniruzzaman", "Julian M. Somers"],
        2018,
        ("PLoS Medicine", "15", "e1002625"),
    );
    let varia = article(
        "Intraperitoneal Radioactive Phosphorus (32P) Versus Observation After Negative \
         Second-Look Laparotomy for Stage III Ovarian Carcinoma: A Randomized Trial of the \
         Gynecologic Oncology Group",
        &["Mark A. Varia", "Frederick B. Stehman", "Brian N. Bundy"],
        2003,
        ("Journal of Clinical Oncology", "21", "2849-2855"),
    );
    let unplaced = |mut r: SourceRecord| {
        r.work.volume = None;
        r.work.page = None;
        r
    };
    let bae = unplaced(article(
        "Resolving Conflicts between Informed Consent and Patients' Privacy in Medical \
         Emergencies",
        &["Hyuna Bae", "Seok-Bae LEE", "Hye-Young JANG"],
        2007,
        ("Korean Journal of Medical Ethics", "10", "73-82"),
    ));
    let trieling = unplaced(article(
        "Monopolar vortices in an irrotational annular shear flow",
        &["R. R. Trieling", "A. H. LINSSEN", "G. J. F. van Heijst"],
        1998,
        ("Journal of Fluid Mechanics", "360", "273-294"),
    ));
    let rangel = unplaced(article(
        "La enseñanza de la accesibilidad en los cursos de Archivística en Brasil: un análisis \
         formativo de los planes de estudio",
        &[
            "Thayron Rodrigues Rangel",
            "Lucía María Velloso de Oliveira",
        ],
        2024,
        ("Inclusão Social", "16", "1"),
    ));
    let mut autophagy = record(
        &["10.4161/auto.19496"],
        "Guidelines for the use and interpretation of assays for monitoring autophagy",
        &["Daniel J. Klionsky", "Fabio C. Abdalla", "Hagai Abeliovich"],
        2012,
    );
    autophagy.work.work_type = Some(crate::work::WorkType::JournalArticle);
    autophagy.work.container_title = Some("Autophagy".into());
    // OpenAlex types it a preprint (bioRxiv's DOI prefix), in a journal.
    let mut cytoscape = record(
        &["10.1101/gr.1239303", "pmid:14597658"],
        "Cytoscape: A Software Environment for Integrated Models of Biomolecular Interaction \
         Networks",
        &["Paul Shannon", "Andrew Markiel", "Owen Ozier"],
        2003,
    );
    cytoscape.work.work_type = Some(crate::work::WorkType::Preprint);
    cytoscape.work.container_title = Some("Genome Research".into());
    let panel = arxiv_preprint(
        "arxiv:2610.36176",
        "Probabilistic Causal Panel Analysis",
        &["Carl Bonander", "Mats Ekman"],
        2026,
    );
    let noma = article(
        "On the Performance of Non-Orthogonal Multiple Access in 5G Systems with Randomly \
         Deployed Users",
        &["Zhiguo Ding", "Zheng Yang", "Pingzhi Fan"],
        2014,
        ("IEEE Signal Processing Letters", "21", "1501-1505"),
    );
    let cases: Vec<(&str, Claim, &SourceRecord)> = vec![
        // k: a conference paper at another conference.
        (
            "h2-747",
            Claim::from_text(
                "Phillip Isola, Jun-Yan Zhu, Tinghui Zhou, and Alexei A. Efros. 2017. \
                 Image-to-Image Translation with Conditional Adversarial Networks. In IEEE \
                 International Conference on Computer Vision (ICCV), 5967–5976.",
            ),
            &pix2pix,
        ),
        (
            "h2-751",
            Claim::from_text(
                "@inproceedings{perozzi2014deepwalk, title={DeepWalk: online learning of social \
                 representations}, author={Perozzi, Bryan and Al-Rfou, Rami and Skiena, \
                 Steven}, booktitle={Proceedings of the International Conference on Machine \
                 Learning (ICML)}, pages={701--710}, year={2014}, publisher={ACM}}",
            ),
            &deepwalk,
        ),
        (
            "h2-753",
            Claim::from_text(
                "Norman P. Jouppi, Cliff Young, and Nishant Patil. 2017. In-Datacenter \
                 Performance Analysis of a Tensor Processing Unit. In IEEE/ACM International \
                 Symposium on Microarchitecture (MICRO), 1–12.",
            ),
            &tpu,
        ),
        // k: a preprint's record can't confirm the conference cited.
        (
            "h2-758",
            Claim::from_text(
                "A. Vaswani et al., \"Attention Is All You Need,\" in Proceedings of the 34th \
                 International Conference on Machine Learning (ICML 2017), 2017.",
            ),
            &attention,
        ),
        (
            "h2-761",
            Claim::from_text(
                "@inproceedings{goodfellow2014generative, title={Generative Adversarial \
                 Networks}, author={Goodfellow, Ian J. and Pouget-Abadie, Jean and Mirza, \
                 Mehdi}, booktitle={International Conference on Learning Representations (ICLR \
                 2014)}, year={2014}}",
            ),
            &gan,
        ),
        // c: locators and journals in styles not read before.
        (
            "h2-562",
            Claim::from_text(
                "Bergeret, F. S., et al. \"Odd triplet superconductivity and related phenomena \
                 in superconductor-ferromagnet structures.\" Physical Review B 77.4 (2005): \
                 1321-1373.",
            ),
            &rmp,
        ),
        (
            "h2-594",
            Claim::from_text(
                "Barro, R.J. and Ursúa, J.F., 2008. Consumption Disasters in the Twentieth \
                 Century. American Economic Review, 100(2), pp.58-63.",
            ),
            &aer,
        ),
        (
            "h2-564",
            Claim::from_text(
                "Maier-Flaig, H., Harder, M., Gross, R., 2016. Spin pumping in strongly coupled \
                 magnon-photon systems. Nature Physics, 94(5), p.054433.",
            ),
            &prb,
        ),
        (
            "h2-566",
            Claim::from_text(
                "25. Nonaka, I. A Dynamic Theory of Organizational Knowledge Creation. Psychol. \
                 Sci. 5, 14–37 (1994).",
            ),
            &nonaka,
        ),
        (
            "h2-595",
            Claim::from_text(
                "T. M. Jones, \"Instrumental Stakeholder Theory: A Synthesis of Ethics and \
                 Economics,\" Acad Management Rev, vol. 20, no. 2, Art. no. 615, 1995.",
            ),
            &jones,
        ),
        (
            "h2-557",
            Claim::from_text(
                "the 2007 Bar-Haim paper in Psychol Sci (Threat-related attentional bias in \
                 anxious and nonanxious individuals: A meta-analytic study.)",
            ),
            &bar_haim,
        ),
        // j: a title with words swapped for synonyms.
        (
            "h2-720",
            Claim::from_text(
                "Wu, Chaomin, et al. \"Likelihood Determinants Associated With Acute \
                 Respiratory Distress Syndrome and Death in Patients With Coronavirus Illness \
                 2019 Pneumonia in Wuhan, China.\" JAMA Internal Medicine 180.7 (2020): 934.",
            ),
            &wu,
        ),
        (
            "h2-745",
            Claim::from_text(
                "39. Hastings, W. K. Monte Carlo sampling approaches with Markov chains and their \
                 applications. Biometrika 57, 97–109 (1970).",
            ),
            &hastings,
        ),
        (
            "h2-732",
            Claim::from_text(
                "Russolillo, A., Moniruzzaman, A. & Somers, J. M. Methadone maintenance therapy \
                 and mort- ality in people with criminal convictions: A population-driven \
                 retrospective cohort study from Canada. PLoS Med. 15, e1002625 (2018).",
            ),
            &methadone,
        ),
        (
            "h2-731",
            Claim {
                title: Some(
                    "Intraperitoneal Radioactive Phosphorus (32P) Versus Observation After \
                     Negative Second-Look Laparotomy for Stage III Ovarian Carcinoma: A \
                     Controlled Study of the Gynecologic Oncology Group"
                        .into(),
                ),
                authors: vec!["Varia, Mark A.".into(), "Stehman, Frederick B.".into()],
                year: Some(2003),
                container: Some("Journal of Clinical Oncology".into()),
                volume: Some("21".into()),
                page: Some("2849-2855".into()),
                ..Claim::default()
            },
            &varia,
        ),
        // b: co-authors added beside the real first author.
        (
            "h2-482",
            Claim::from_text(
                "Bae, H-A., LEE, S-B., Corvastine, E. and JANG, H-Y., 2007. Resolving Conflicts \
                 between Informed Consent and Patients' Privacy in Medical Emergencies. Korean \
                 Journal of Medical Ethics, 10(1), pp.73-82.",
            ),
            &bae,
        ),
        (
            "h2-496",
            Claim::from_text(
                "TRIELING, R.R. and Corveshaw, C., 1998. Monopolar vortices in an irrotational \
                 annular shear flow. Journal of Fluid Mechanics, 360, pp.273-294.",
            ),
            &trieling,
        ),
        (
            "h2-495",
            Claim::from_text(
                "Rangel TR, Thalubrecht HL, María Velloso de Oliveira L. La enseñanza de la \
                 accesibilidad en los cursos de Archivística en Brasil: un análisis formativo de \
                 los planes de estudio. Inclusão Soc. 2024;16(1).",
            ),
            &rangel,
        ),
        // h: a journal's article cited as a preprint, a preprint in a journal.
        (
            "h2-718",
            Claim::from_text(
                "Klionsky, D. J., Abdalla, F. C., & Abeliovich, H. (2011). Guidelines for the \
                 use and interpretation of assays for monitoring autophagy. bioRxiv [Preprint].",
            ),
            &autophagy,
        ),
        (
            "h2-715",
            Claim::from_text(
                "Shannon, P., Markiel, A., & Ozier, O. (2002). Cytoscape: A Software \
                 Environment for Integrated Models of Biomolecular Interaction Networks. bioRxiv \
                 [Preprint].",
            ),
            &cytoscape,
        ),
        (
            "h2-699",
            Claim::from_text(
                "Bonander, Carl, et al. \"Probabilistic Causal Panel Analysis.\" Journal of \
                 Econometrics 251.1 (2026): 743-752.",
            ),
            &panel,
        ),
        // d: another work's place under a title in Chinese.
        (
            "h2-624",
            Claim::from_text(
                "[26] 周毅艺. 化学分析仪器计量检定分析[J]. IEEE Signal Processing Letters, 2014, \
                 21(12): 1501-1505.",
            ),
            &noma,
        ),
    ];
    let failing: Vec<String> = cases
        .iter()
        .filter_map(|(id, claim, record)| {
            let a = assess(claim, record);
            (a.confidence >= VERIFIED).then(|| format!("{id}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
    // f: BibTeX's arXiv eprint is the identifier claimed.
    let eprint = Claim::from_text(
        "@misc{shanu2017, title={A Computational Approach for Designing Tiger Corridors in \
         India}, author={Shanu, Saurabh}, year={2017}, eprint={1710.64837}, \
         archivePrefix={arXiv}, primaryClass={cs.CY}}",
    );
    assert_eq!(eprint.ids, ["arxiv:1710.64837".parse::<WorkId>().unwrap()]);

    // The same works cited right still verify.
    let right: Vec<(Claim, &SourceRecord)> = vec![
        (
            Claim::from_text(
                "Phillip Isola, Jun-Yan Zhu, and Tinghui Zhou. 2017. Image-to-Image Translation \
                 with Conditional Adversarial Networks. In Proceedings of the IEEE Conference on \
                 Computer Vision and Pattern Recognition (CVPR), 5967–5976.",
            ),
            &pix2pix,
        ),
        (
            Claim::from_text(
                "@inproceedings{perozzi2014deepwalk, title={DeepWalk: online learning of social \
                 representations}, author={Perozzi, Bryan and Al-Rfou, Rami and Skiena, \
                 Steven}, booktitle={Proceedings of the 20th ACM SIGKDD International Conference \
                 on Knowledge Discovery and Data Mining (KDD)}, pages={701--710}, year={2014}}",
            ),
            &deepwalk,
        ),
        (
            Claim::from_text(
                "Norman P. Jouppi, Cliff Young, and Nishant Patil. 2017. In-Datacenter \
                 Performance Analysis of a Tensor Processing Unit. In Proceedings of the 44th \
                 Annual International Symposium on Computer Architecture (ISCA), 1–12.",
            ),
            &tpu,
        ),
        (
            Claim::from_text(
                "Vaswani A, Shazeer N, Parmar N. Attention is all you need. arXiv:1706.03762. \
                 2017.",
            ),
            &attention,
        ),
        (
            Claim::from_text(
                "Bergeret, F. S., et al. \"Odd triplet superconductivity and related phenomena \
                 in superconductor-ferromagnet structures.\" Reviews of Modern Physics 77.4 \
                 (2005): 1321-1373.",
            ),
            &rmp,
        ),
        (
            Claim::from_text(
                "Barro, R.J. and Ursúa, J.F., 2008. Consumption Disasters in the Twentieth \
                 Century. American Economic Review, 98(2), pp.58-63.",
            ),
            &aer,
        ),
        (
            Claim::from_text(
                "Maier-Flaig, H., Harder, M., Gross, R., 2016. Spin pumping in strongly coupled \
                 magnon-photon systems. Physical Review B, 94(5), p.054433.",
            ),
            &prb,
        ),
        (
            Claim::from_text(
                "25. Nonaka, I. A Dynamic Theory of Organizational Knowledge Creation. Organ. \
                 Sci. 5, 14–37 (1994).",
            ),
            &nonaka,
        ),
        (
            Claim::from_text(
                "T. M. Jones, \"Instrumental Stakeholder Theory: A Synthesis of Ethics and \
                 Economics,\" Acad Management Rev, vol. 20, no. 2, Art. no. 404, 1995.",
            ),
            &jones,
        ),
        (
            Claim::from_text(
                "Wu, Chaomin, et al. \"Risk Factors Associated With Acute Respiratory Distress \
                 Syndrome and Death in Patients With Coronavirus Disease 2019 Pneumonia in Wuhan, \
                 China.\" JAMA Internal Medicine 180.7 (2020): 934.",
            ),
            &wu,
        ),
        (
            Claim::from_text(
                "Russolillo, A., Moniruzzaman, A. & Somers, J. M. Methadone maintenance \
                 treatment and mort- ality in people with criminal convictions: A \
                 population-based retrospective cohort study from Canada. PLoS Med. 15, \
                 e1002625 (2018).",
            ),
            &methadone,
        ),
        (
            Claim::from_text(
                "Bae, H-A., LEE, S-B. and JANG, H-Y., 2007. Resolving Conflicts between Informed \
                 Consent and Patients' Privacy in Medical Emergencies. Korean Journal of Medical \
                 Ethics, 10(1), pp.73-82.",
            ),
            &bae,
        ),
        (
            Claim::from_text(
                "Rangel TR, María Velloso de Oliveira L. La enseñanza de la accesibilidad en los \
                 cursos de Archivística en Brasil: un análisis formativo de los planes de \
                 estudio. Inclusão Soc. 2024;16(1).",
            ),
            &rangel,
        ),
        (
            Claim::from_text(
                "Klionsky, D. J., Abdalla, F. C., & Abeliovich, H. (2012). Guidelines for the \
                 use and interpretation of assays for monitoring autophagy. Autophagy, 8(4), \
                 445-544.",
            ),
            &autophagy,
        ),
    ];
    let failing: Vec<String> = right
        .iter()
        .filter_map(|(claim, record)| {
            let a = assess(claim, record);
            (a.confidence < VERIFIED).then(|| format!("{claim:?}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");
}

#[test]
fn third_held_out_red_team_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/holdout-3.jsonl (round
    // 4) not covered by a test of their own, against the records they
    // verified: none may verify now.
    let mut feds = record(
        &["doi:10.17016/feds.2007.52"],
        "Do High Debt Payments Hinder Household Consumption Smoothing?",
        &["Kathleen W. Johnson", "Geng Li"],
        2007,
    );
    feds.work.work_type = Some(crate::work::WorkType::JournalArticle);
    feds.work.container_title = Some("Finance and Economics Discussion Series".into());
    feds.work.page = Some("1-40".into());
    let hepatoid = article(
        "Hepatoid adenocarcinoma in the lung",
        &["Yasufumi Hayashi", "Yoshinori Takanashi", "Hiroyuki Ohsawa"],
        2002,
        ("Lung Cancer", "38", "211-214"),
    );
    let mut alcohol = record(
        &["doi:10.15288/jsad.2020.81.297", "pmid:32527381"],
        "Alcohol Policy and Coronavirus: An Open Research Agenda",
        &["Maristela G. Monteiro", "Jürgen Rehm", "Maik Duennbier"],
        2020,
    );
    alcohol.work.work_type = Some(crate::work::WorkType::JournalArticle);
    alcohol.work.container_title = Some("Journal of Studies on Alcohol and Drugs".into());
    let cases: Vec<(&str, &str, &SourceRecord)> = vec![
        // c: a year five off, a series' number like a year in the volume.
        (
            "h3-691",
            "Johnson, K.W. and Li, G. (2012) 'Do High Debt Payments Hinder Household \
             Consumption Smoothing?', Finance and Economics Discussion Series, 2007.0(52), pp. \
             1-40.",
            &feds,
        ),
        // c: another journal, named in prose before its volume and pages.
        (
            "h3-717",
            "As Hayashi et al. (2002) showed in Cancer Epidemiology, 38, 211–214, in “Hepatoid \
             adenocarcinoma in the lung”, the effect held across subgroups.",
            &hepatoid,
        ),
        // h: a journal's article cited as a Research Square preprint.
        (
            "h3-885",
            "Monteiro, M. G., Rehm, J., & Duennbier, M. (2020). Alcohol Policy and \
             Coronavirus: An Open Research Agenda. Research Square.",
            &alcohol,
        ),
    ];
    let failing: Vec<String> = cases
        .iter()
        .filter_map(|(id, claim, record)| {
            let a = assess(&Claim::from_text(*claim), record);
            (a.confidence >= VERIFIED).then(|| format!("{id}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");

    // The same works cited right still verify.
    for (claim, record) in [
        (
            "Johnson, K.W. and Li, G. (2007) 'Do High Debt Payments Hinder Household \
             Consumption Smoothing?', Finance and Economics Discussion Series, 2007.0(52), pp. \
             1-40.",
            &feds,
        ),
        (
            "As Hayashi et al. (2002) showed in Lung Cancer, 38, 211–214, in “Hepatoid \
             adenocarcinoma in the lung”, the effect held across subgroups.",
            &hepatoid,
        ),
        (
            "Monteiro, M. G., Rehm, J., & Duennbier, M. (2020). Alcohol Policy and \
             Coronavirus: An Open Research Agenda. Journal of Studies on Alcohol and Drugs.",
            &alcohol,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
}

#[test]
fn fourth_held_out_red_team_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/holdout-4.jsonl (round
    // 5) not covered by a test of their own, against the records they
    // verified: none may verify now.
    let diabetes_care = article(
        "Nutritional Risk of High-Carbohydrate, Guar Gum Dietary Supplementation in \
         Non-Insulin-Dependent Diabetes Mellitus",
        &["M. A. S. Van Duyn", "T. A. Leo", "M. E. McIvor"],
        1986,
        ("Diabetes Care", "9", "497-503"),
    );
    // A conference's abstract printed in a journal's supplement.
    let mut abstract_ = placed(
        &["doi:10.1136/annrheumdis-2018-eular.6693"],
        "AB0300 Ultrasound evaluation of ankle and foot joints in rheumatoid arthritis",
        &["Safaa Fellous", "Hanan Rkain", "Saloua Afilal"],
        2018,
        ("Annals of the Rheumatic Diseases", "77", "1328"),
    );
    abstract_.work.work_type = Some(crate::work::WorkType::Other("conference-abstract".into()));
    let cases: Vec<(&str, &str, &SourceRecord)> = vec![
        // A BibTeX `journal` naming the parent, though the title has its
        // word.
        (
            "h4-715",
            "@article{vanduyn1986, author = {Van Duyn, M A S and Leo, T A and McIvor, M E}, \
             title = {Nutritional Risk of High-Carbohydrate, Guar Gum Dietary Supplementation \
             in Non-Insulin-Dependent Diabetes Mellitus}, journal = {Diabetes}, year = {1986}, \
             volume = {9}, pages = {497--503}}",
            &diabetes_care,
        ),
        (
            "h4-695",
            "Fellous, S., Rkain, H., & Afilal, S. (2018). AB0300 Ultrasound evaluation of ankle \
             and foot joints in rheumatoid arthritis. RMD Open, 77, 1328.",
            &abstract_,
        ),
    ];
    let failing: Vec<String> = cases
        .iter()
        .filter_map(|(id, claim, record)| {
            let a = assess(&Claim::from_text(*claim), record);
            (a.confidence >= VERIFIED).then(|| format!("{id}: {a:?}"))
        })
        .collect();
    assert!(failing.is_empty(), "{failing:#?}");

    // The same works cited right still verify.
    for (claim, record) in [
        (
            "@article{vanduyn1986, author = {Van Duyn, M A S and Leo, T A and McIvor, M E}, \
             title = {Nutritional Risk of High-Carbohydrate, Guar Gum Dietary Supplementation \
             in Non-Insulin-Dependent Diabetes Mellitus}, journal = {Diabetes Care}, year = \
             {1986}, volume = {9}, pages = {497--503}}",
            &diabetes_care,
        ),
        (
            "Fellous, S., Rkain, H., & Afilal, S. (2018). AB0300 Ultrasound evaluation of ankle \
             and foot joints in rheumatoid arthritis. Annals of the Rheumatic Diseases, 77, 1328.",
            &abstract_,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
}

#[test]
fn wild_red_team_citations_as_models_write_them() {
    // bench/redteam/wild-1.jsonl (round 5): real citations models wrote,
    // right but for a model's habits, against the records they missed.
    let emperor = article(
        "Cardiovascular and Renal Outcomes with Empagliflozin in Heart Failure",
        &[
            "Milton Packer",
            "Stefan D. Anker",
            "Javed Butler",
            "Gerasimos Filippatos",
        ],
        2020,
        ("New England Journal of Medicine", "383", "1413-1424"),
    );
    let mut braden = article(
        "The Braden Scale for Predicting Pressure Sore Risk",
        &[
            "Nancy Bergstrom",
            "Barbara J. Braden",
            "Antoinette Laguzza",
            "Victoria Holman",
        ],
        1987,
        ("Nursing Research", "36", "205-210"),
    );
    // As the record garbles its dash.
    braden.work.page = Some("205???210".into());
    let griffiths = article(
        "Nurse staffing and patient outcomes: Strengths and limitations of the evidence to \
         inform policy and practice. A review and discussion paper based on evidence reviewed \
         for the National Institute for Health and Care Excellence Safe Staffing guideline \
         development",
        &["Peter Griffiths", "Jane Ball", "Jonathan Drennan"],
        2016,
        ("International Journal of Nursing Studies", "63", "213-225"),
    );
    let mut gat = record(
        &["arxiv:1710.10903"],
        "Graph Attention Networks",
        &["Petar Veličković", "Guillem Cucurull", "Arantxa Casanova"],
        2018,
    );
    gat.work.work_type = Some(crate::work::WorkType::Preprint);
    let pulsar = article(
        "Discovery of a pulsar in a binary system",
        &["R. A. Hulse", "J. H. Taylor"],
        1975,
        ("The Astrophysical Journal", "195", "L51-L53"),
    );
    for (n, claim, record) in [
        // A trial's label before the byline is no author.
        (
            "w1-41",
            "EMPEROR-Reduced (empagliflozin, n=3,730): Packer M, Anker SD, Butler J, et al. \
             Cardiovascular and renal outcomes with empagliflozin in heart failure. N Engl J \
             Med. 2020;383:1413-1424.",
            &emperor,
        ),
        (
            "w1-90",
            "Bergstrom N, Braden BJ, Laguzza A, Holman V. The Braden Scale for predicting \
             pressure sore risk. Nurs Res. 1987;36(4):205-210.",
            &braden,
        ),
        // A record's title running on into a description, cited without.
        (
            "w1-374",
            "Griffiths P, Ball J, Drennan J, et al. Nurse staffing and patient outcomes: \
             strengths and limitations of the evidence to inform policy and practice. Int J \
             Nurs Stud. 2016;63:213-25.",
            &griffiths,
        ),
        // BibTeX's accents in its authors.
        (
            "w1-339",
            "@misc{velickovic2018graph, author = {Veli{\\v{c}}kovi{\\'c}, Petar and Cucurull, \
             Guillem and Casanova, Arantxa}, title = {Graph Attention Networks}, year = {2018}, \
             eprint = {1710.10903}, archivePrefix = {arXiv}}",
            &gat,
        ),
        // A journal's letters, numbered in its volumes, at its place.
        (
            "w1-249",
            "Hulse, R. A., & Taylor, J. H. (1975). \"Discovery of a pulsar in a binary system.\" \
             The Astrophysical Journal Letters, 195, L51–L53.",
            &pulsar,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{n}: {a:?}");
    }
    // A page with a section's letter is read: "pp. R3–R20" for pages
    // 121–138 is the wrong page.
    let ebell = article(
        "The long-term economic impact of leaving the EU",
        &["Monique Ebell", "James Warren"],
        2016,
        ("National Institute Economic Review", "236", "121-138"),
    );
    let claim = Claim::from_text(
        "Ebell, M. and Warren, J. (2016) 'The long-term economic impact of leaving the EU', \
         National Institute Economic Review, 236, pp. R3–R20.",
    );
    let a = assess(&claim, &ebell);
    assert_eq!(a.page.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // The letters cited at another place are the sister journal.
    let claim = Claim::from_text(
        "Hulse, R. A., & Taylor, J. H. (1975). \"Discovery of a pulsar in a binary system.\" \
         The Astrophysical Journal Letters, 196, L1–L3.",
    );
    assert!(assess(&claim, &pulsar).confidence < VERIFIED);
}
