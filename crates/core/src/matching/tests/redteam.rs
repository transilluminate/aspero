//! Red-team regressions: the minimised failures of the first corpus
//! (`bench/redteam/corpus.jsonl`) and of the first held-out corpus.

use super::*;

#[test]
fn red_team_minimised_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/failures.jsonl (#102),
    // against the records they verified: none may verify now. Each with
    // its id there.
    let nature = |title: &str, authors: &[&str], year, volume, pages| {
        article(title, authors, year, ("Nature", volume, pages))
    };
    let alphafold = nature(
        "Highly accurate protein structure prediction with AlphaFold",
        &["John Jumper", "Richard Evans", "Alexander Pritzel"],
        2021,
        "596",
        "583-589",
    );
    let lstm = article(
        "Long Short-Term Memory",
        &["Sepp Hochreiter", "Jürgen Schmidhuber"],
        1997,
        ("Neural Computation", "9", "1735-1780"),
    );
    let colchicine = article(
        "Efficacy and Safety of Low-Dose Colchicine after Myocardial Infarction",
        &["Jean-Claude Tardif", "Simon Kouz", "David D. Waters"],
        2019,
        ("New England Journal of Medicine", "381", "2497-2505"),
    );
    let huang = article(
        "Clinical features of patients infected with 2019 novel coronavirus in Wuhan, China",
        &["Chaolin Huang", "Yeming Wang", "Xingwang Li"],
        2020,
        ("The Lancet", "395", "497-506"),
    );
    let mut zenodo = record(
        &["10.5281/zenodo.13525448"],
        "Why Most Published Research Findings Are False",
        &["John P. A. Ioannidis"],
        2005,
    );
    zenodo.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let mut arxiv = record(
        &["arxiv:2410.02479"],
        "Cross-Embodiment Dexterous Grasping with Reinforcement Learning",
        &["Haoqi Yuan", "Bohan Zhou"],
        2024,
    );
    arxiv.work.work_type = Some(crate::work::WorkType::Preprint);
    let cases: Vec<(&str, Claim, SourceRecord)> = vec![
        (
            "corpus-174",
            Claim::from_text(
                "Tuyls K, Rourke-Ellison D. Evolutionary game theory and multi-agent \
                 reinforcement learning. 2005.",
            ),
            article(
                "Evolutionary game theory and multi-agent reinforcement learning",
                &["Karl Tuyls", "Ann Nowé"],
                2005,
                ("The Knowledge Engineering Review", "20", "63-90"),
            ),
        ),
        (
            "adv-18",
            Claim::from_text(format!(
                "Rivers E, Smith J, Patel R, et al. {EGDT}. N Engl J Med. 2001;345(19):1368-77."
            )),
            egdt(),
        ),
        (
            "adv-21",
            Claim {
                title: Some(
                    "Fibrinolysis for patients with intermediate-risk pulmonary embolism".into(),
                ),
                authors: vec!["Meyer, G.".into(), "Smith, J.".into()],
                year: Some(2014),
                ..Claim::default()
            },
            peitho(),
        ),
        (
            "adv-01",
            Claim::from_text(
                "LeCun Y, Bengio Y, Hinton G. Deep learning for medical image analysis. Nature. \
                 2015;521:436-444.",
            ),
            lecun(),
        ),
        (
            "adv-13",
            Claim {
                title: Some("Deep learning: a primer for clinicians".into()),
                authors: vec!["LeCun, Y.".into(), "Bengio, Y.".into(), "Hinton, G.".into()],
                year: Some(2015),
                container: Some("Nature".into()),
                ..Claim::default()
            },
            lecun(),
        ),
        (
            "adv-12",
            Claim::from_text(
                "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation of targeted \
                 therapies. Cell. 2011;144(5):646-674.",
            ),
            hallmarks(),
        ),
        (
            "adv-08",
            Claim::from_text(
                "Jumper J, Evans R, Pritzel A, et al. Highly accurate protein structure \
                 prediction with AlphaFold enables structure-based drug discovery. Nature. 2021.",
            ),
            alphafold.clone(),
        ),
        (
            "adv-33",
            Claim::from_text(
                "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. \
                 2018;70(1):7-30.",
            ),
            article(
                "Cancer statistics, 2020",
                &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
                2020,
                ("CA: A Cancer Journal for Clinicians", "70", "7-30"),
            ),
        ),
        (
            "adv-39",
            Claim::from_text(
                "Hindricks G, Potpara T, Dagres N, et al. 2020 ESC Guidelines for the diagnosis \
                 and management of atrial fibrillation developed in collaboration with the \
                 European Association for Cardio-Thoracic Surgery (EACTS). Eur Heart J. 2017.",
            ),
            article(
                "2020 ESC Guidelines for the diagnosis and management of atrial fibrillation \
                 developed in collaboration with the European Association for Cardio-Thoracic \
                 Surgery (EACTS)",
                &["Gerhard Hindricks", "Tatjana Potpara", "Nikolaos Dagres"],
                2020,
                ("European Heart Journal", "42", "373-498"),
            ),
        ),
        (
            "adv-27",
            Claim::from_text(
                "Jumper J, Evans R, Pritzel A, et al. Highly accurate protein structure \
                 prediction with AlphaFold. Science. 2021;596(7873):583-589.",
            ),
            alphafold.clone(),
        ),
        (
            "corpus-203",
            Claim::from_text(
                "Huang C, et al. Clinical features of patients infected with 2019 novel \
                 coronavirus in Wuhan, China. JAMA. 2020.",
            ),
            huang,
        ),
        (
            "adv-56",
            Claim::from_text(
                "Hochreiter S, Schmidhuber J. Long short-term memory. Neural Netw. 1998.",
            ),
            lstm.clone(),
        ),
        (
            "corpus-202",
            Claim::from_text(
                "Altschul, S.F. et al. (1990) 'Basic local alignment search tool', American \
                 Journal of Epidemiology, 215(3), pp. 403-410.",
            ),
            article(
                "Basic local alignment search tool",
                &["Stephen F. Altschul", "Warren Gish", "Webb Miller"],
                1990,
                ("Journal of Molecular Biology", "215", "403-410"),
            ),
        ),
        (
            "adv-52",
            Claim::from_text(
                "Silver, D. et al. Mastering the game of Go with deep neural networks and tree \
                 search. Nature 530, 484 (2016).",
            ),
            nature(
                "Mastering the game of Go with deep neural networks and tree search",
                &["David Silver", "Aja Huang", "Chris J. Maddison"],
                2016,
                "529",
                "484-489",
            ),
        ),
        (
            "adv-53",
            Claim::from_text(
                "@article{jumper2021, author = {Jumper, John and Evans, Richard}, title = \
                 {Highly accurate protein structure prediction with {AlphaFold}}, journal = \
                 {Nature}, year = {2021}, volume = {597}, pages = {583--589}}",
            ),
            alphafold,
        ),
        (
            "corpus-219",
            Claim::from_text(
                "Niwa T. Transfection of plasmid DNA by nanocarriers. J Microbiol Methods. \
                 2005;61:251.",
            ),
            article(
                "Lytic enzyme, labiase for a broad range of Gram-positive bacteria",
                &["Tomoko Niwa", "Yoshiaki Kawamura"],
                2005,
                ("Journal of Microbiological Methods", "61", "251-260"),
            ),
        ),
        (
            "adv-44",
            Claim::from_text(
                "Horby P, Lim WS, Emberson JR, et al. Hydroxychloroquine in hospitalized \
                 patients with Covid-19. N Engl J Med. 2021;384(8):693-704.",
            ),
            article(
                "Dexamethasone in Hospitalized Patients with Covid-19",
                &["Peter Horby", "Wei Shen Lim", "Jonathan R. Emberson"],
                2021,
                ("New England Journal of Medicine", "384", "693-704"),
            ),
        ),
        (
            "adv-57",
            Claim::from_text(
                "Ioannidis JP. Why most published research findings are false. BMJ. 2006.",
            ),
            zenodo,
        ),
        (
            "corpus-271",
            Claim::from_text(
                "Yuan H. Cross-embodiment dexterous grasping with reinforcement learning. \
                 Science Robotics. 2024.",
            ),
            arxiv,
        ),
        (
            "corpus-277",
            Claim::from_text(
                "Tardif JC. Efficacy and safety of low-dose colchicine after myocardial \
                 infarction. medRxiv. 2019.",
            ),
            colchicine.clone(),
        ),
        (
            "corpus-275",
            Claim::from_text("Hochreiter S. Long short-term memory. arXiv preprint. 1997."),
            lstm,
        ),
        (
            "adv-45",
            Claim::from_text(
                "World Medical Association. Declaration of Geneva: the physician's pledge. JAMA. \
                 2013;310(20):2191-2194.",
            ),
            {
                let mut r = placed(
                    &[],
                    "World Medical Association Declaration of Helsinki",
                    &[],
                    2013,
                    ("JAMA", "310", "2191-2194"),
                );
                r.work.work_type = Some(crate::work::WorkType::JournalArticle);
                r
            },
        ),
        (
            "corpus-193",
            Claim::from_text(
                "Shiomi and SUGIURA reported in Gastroenterology (1984) — \"\
                 ヒメフタテンヨコバイが媒介するマイコプラズマ様微生物の寄主範囲による類別\".",
            ),
            {
                let mut r = record(
                    &["10.3186/jjphytopath.50.149"],
                    "ヒメフタテンヨコバイが媒介するマイコプラズマ様微生物の寄主範囲による類別",
                    &["Toshiki Shiomi", "Miyoji SUGIURA"],
                    1984,
                );
                r.work.work_type = Some(crate::work::WorkType::JournalArticle);
                r
            },
        ),
        // The app's pre-test of the fixes: the report of the year cited
        // chosen over the one titled, volume and pages; words inserted
        // inside a title.
        (
            "app-cancer-statistics",
            Claim::from_text(
                "Siegel RL, Miller KD, Jemal A. Cancer statistics, 2020. CA Cancer J Clin. \
                 2018;70(1):7-30.",
            ),
            article(
                "Cancer statistics, 2018",
                &["Rebecca L. Siegel", "Kimberly D. Miller", "Ahmedin Jemal"],
                2018,
                ("CA: A Cancer Journal for Clinicians", "68", "7-30"),
            ),
        ),
        (
            "app-sepsis-3",
            Claim::from_text(
                "Singer M, Deutschman CS, Seymour CW, et al. The Third International Consensus \
                 Definitions for Sepsis and Septic Shock in Adults and Children (Sepsis-3). JAMA. \
                 2016;315(8):801-810.",
            ),
            article(
                "The Third International Consensus Definitions for Sepsis and Septic Shock \
                 (Sepsis-3)",
                &[
                    "Mervyn Singer",
                    "Clifford S. Deutschman",
                    "Christopher Warren Seymour",
                ],
                2016,
                ("JAMA", "315", "801-810"),
            ),
        ),
    ];
    for (id, claim, record) in &cases {
        let a = assess(claim, record);
        assert!(a.confidence < VERIFIED, "{id}: {a:?}");
    }
    // The same works cited right still verify.
    for (text, record) in [
        (
            "Tardif JC, Kouz S, Waters DD, et al. Efficacy and safety of low-dose colchicine \
             after myocardial infarction. N Engl J Med. 2019;381(26):2497-2505.",
            colchicine,
        ),
        (
            "Hanahan D, Weinberg RA. Hallmarks of cancer: the next generation. Cell. \
             2011;144(5):646-674.",
            hallmarks(),
        ),
        (
            "LeCun, Y., Bengio, Y. & Hinton, G. Deep learning. Nature 521, 436–444 (2015).",
            lecun(),
        ),
    ] {
        let a = assess(&Claim::from_text(text), &record);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn held_out_red_team_failures_stay_fixed() {
    // Minimised false VERIFIEDs from bench/redteam/holdout-1.jsonl (round
    // 2), against the records they verified: none may verify now. Each
    // with its line there.
    let ids = |mut r: SourceRecord, id: &str| {
        r.ids = vec![id.parse().unwrap()];
        r
    };
    let jcp = article(
        "Anisotropic Rayleigh scattering investigations in smectic A liquid crystal phases",
        &["O. Mondain-Monval", "H. J. Coles", "T. Claverie"],
        1994,
        ("The Journal of Chemical Physics", "101", "6301"),
    );
    let tsunami = article(
        "After the Tsunami: Legal Implications of Mass Burials of Unidentified Victims in Sri \
         Lanka",
        &["Clifford Perera"],
        2005,
        ("PLoS Medicine", "2", "e185"),
    );
    let khandekar = article(
        "Graph partitioning using single commodity flows",
        &["Rohit Khandekar", "Satish Rao", "Umesh Vazirani"],
        2009,
        ("Journal of the ACM", "56", "1-15"),
    );
    let mut eclinm = record(
        &["10.1016/j.eclinm.2020.100519"],
        "Sex-bias in COVID-19-associated illness severity and mortality in cancer patients: A \
         systematic review and meta-analysis",
        &["Robin Park", "Anusha Chidharla", "Kathan Mehta"],
        2020,
    );
    eclinm.work.work_type = Some(crate::work::WorkType::JournalArticle);
    eclinm.work.container_title = Some("EClinicalMedicine".into());
    // Europe PMC's record, its type not given.
    let mut nursing = record(
        &["10.1016/j.ijnurstu.2011.09.008"],
        "Addressing the complexity conundrum in and of nursing",
        &["David R Thompson", "Alexander M Clark"],
        2011,
    );
    nursing.work.container_title = Some("International journal of nursing studies".into());
    nursing.work.other_years = vec![2012];
    let laris = placed(
        &["10.1038/s41467-021-25141-1"],
        "On the problems and promises of savanna fire regime change",
        &["Paul Laris"],
        2021,
        ("Nature communications", "12", "4891"),
    );
    let mut brain = record(
        &["10.1093/brain/awaf429"],
        "Differential selectivity of microglia and astrocytes in HIV-1 gp120-induced synaptic \
         pruning",
        &[
            "Zachary T Watson",
            "Silvana Valdebenito-Silva",
            "Michael S. Spurgat",
        ],
        2025,
    );
    brain.work.work_type = Some(crate::work::WorkType::JournalArticle);
    brain.work.container_title = Some("Brain".into());
    let mut tpami = record(
        &["10.1109/tpami.1982.4767218"],
        "Adaptive Detection and Removal of Non-Gaussian Spikes from Gaussian Data",
        &["Rachel Boucher", "J.P. Noonan"],
        1982,
    );
    tpami.work.work_type = Some(crate::work::WorkType::JournalArticle);
    tpami.work.container_title =
        Some("IEEE Transactions on Pattern Analysis and Machine Intelligence".into());
    let goldman = ids(
        article(
            "Cellular specificity of serotonin storage and axonal transport in identified \
             neurones of Aplysia californica",
            &["James E. Goldman", "James H. Schwartz"],
            1974,
            ("The Journal of Physiology", "242", "61-76"),
        ),
        "10.1113/jphysiol.1974.sp010694",
    );
    let cases: Vec<(&str, Claim, SourceRecord)> = vec![
        (
            "h1-475",
            Claim::from_text(
                "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in \
                 smectic A liquid crystal phases. J. Org. Chem. 101, 6301 (1994).",
            ),
            jcp.clone(),
        ),
        (
            "h1-478",
            Claim {
                title: Some("Addressing the complexity conundrum in and of nursing".into()),
                authors: vec!["Thompson, D.R.".into(), "Clark, A.M.".into()],
                year: Some(2012),
                container: Some("Annals of Internal Medicine".into()),
                ..Claim::default()
            },
            nursing,
        ),
        (
            "h1-483",
            Claim::from_text(
                "Perera C. After the Tsunami: Legal Implications of Mass Burials of Unidentified \
                 Victims in Sri Lanka. Cad. Saúde Pública. 2005;2(6):e185.",
            ),
            tsunami.clone(),
        ),
        (
            "h1-493",
            Claim::from_text(
                "R. Khandekar, S. Rao, and U. Vazirani, “Graph partitioning using single \
                 commodity flows,” IEEE Trans. Pattern Anal. Mach. Intell., vol. 56, no. 4, pp. \
                 1-15, 2009.",
            ),
            khandekar.clone(),
        ),
        (
            "h1-494",
            Claim::from_text(
                "Park, Robin, Anusha Chidharla, and Kathan Mehta. “Sex-bias in \
                 COVID-19-associated illness severity and mortality in cancer patients: A \
                 systematic review and meta-analysis.” American Journal of Epidemiology (2020). \
                 https://doi.org/10.1016/j.eclinm.2020.100519",
            ),
            eclinm.clone(),
        ),
        (
            "h1-503",
            Claim::from_text(
                "Laris, P. On the problems and promises of savanna fire regime change. Sci Rep \
                 12, 4891 (2021).",
            ),
            laris.clone(),
        ),
        (
            "h1-476",
            Claim::from_text(
                "Watson ZT, Valdebenito-Silva S, Spurgat M, et al. Differential selectivity of \
                 microglia and astrocytes in HIV-1 gp120-induced synaptic pruning. JAMA: The \
                 Journal of the American Medical Association. 2025. doi:10.1093/brain/awaf429",
            ),
            brain,
        ),
        (
            "h1-484",
            Claim::from_text(
                "Boucher, R. E., & Noonan, J. P. (1982). Adaptive Detection and Removal of \
                 Non-Gaussian Spikes from Gaussian Data. Proceedings of the AAAI Symposium \
                 Series. https://doi.org/10.1109/tpami.1982.4767218",
            ),
            tpami,
        ),
        (
            "h1-505",
            Claim::from_text(
                "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in \
                 smectic A liquid crystal phases. J. Chem. Phys. 102, 6301 (1994).",
            ),
            jcp.clone(),
        ),
        (
            "h1-469",
            Claim::from_text(
                "Goldman, J. E., & Schwartz, J. H. (1972). Cellular specificity of serotonin \
                 storage and axonal transport in identified neurones of Aplysia californica. \
                 The Journal of Physiology, 242(1), 61-76. \
                 https://doi.org/10.1113/jphysiol.1974.sp010694",
            ),
            goldman.clone(),
        ),
        (
            "h1-738",
            Claim::from_text(
                "C. Brukner, M. Zukowski, and A. Zeilinger, “The essence of entanglement,” Phys. \
                 Rev. A, vol. 63, no. 10, pp. 644-661, 2001.",
            ),
            article(
                "Conceptual inadequacy of the Shannon information in quantum measurements",
                &["Časlav Brukner", "Anton Zeilinger"],
                2001,
                ("Physical Review A", "63", "644"),
            ),
        ),
        (
            "h1-694",
            Claim::from_text(
                "@inproceedings{k2022, author = {Kingsley-Odhiambo, Henrik and Merriweather-Sung, \
                 Ilse}, title = {Federated contrastive pretraining for clinical note \
                 de-identification}, booktitle = {Findings of the Association for Computational \
                 Linguistics: EMNLP 2022}, year = {2022}, pages = {3321--3334}}",
            ),
            {
                let mut r = record(
                    &["isbn:9781959429432"],
                    "Findings of the Association for Computational Linguistics",
                    &["Association for Computational Linguistics"],
                    2022,
                );
                r.work.work_type = Some(crate::work::WorkType::Book);
                r
            },
        ),
        (
            "h1-496",
            Claim::from_text(
                "@article{levitt1996, author = {Levitt, S. D.}, title = {The Effect of Prison \
                 Population Size on Crime Rates: Evidence from Prison Overcrowding Litigation}, \
                 journal = {Social Science & Medicine}, year = {1996}}",
            ),
            {
                let mut r = record(
                    &[],
                    "The effect of prison population size on crime rates: evidence from prison \
                     overcrowding litigation",
                    &["Steven D. Levitt"],
                    1996,
                );
                r.work.work_type = Some(crate::work::WorkType::Book);
                r
            },
        ),
        (
            "h1-491",
            Claim::from_text(
                "Berry, Christopher J. “Adam Smith's Considerations on Language.” Revue \
                 historique 35, no. 1 (1974): 130.",
            ),
            article(
                "Adam Smith's Considerations on Language",
                &["Christopher J. Berry"],
                1974,
                ("Journal of the History of Ideas", "35", "130-138"),
            ),
        ),
        (
            "h1-458",
            Claim::from_text(
                "@article{goldman1972, author = {Goldman, J. E. and Schwartz, J. H.}, title = \
                 {Cellular specificity of serotonin storage and axonal transport in identified \
                 neurones of Aplysia californica}, journal = {The Journal of Physiology}, year = \
                 {1972}, volume = {242}, pages = {61--76}, doi = \
                 {10.1113/jphysiol.1974.sp010694}}",
            ),
            goldman.clone(),
        ),
    ];
    for (id, claim, record) in &cases {
        let a = assess(claim, record);
        assert!(a.confidence < VERIFIED, "{id}: {a:?}");
    }
    // The same works cited right still verify.
    for (claim, record) in [
        (
            "Mondain-Monval, O. et al. Anisotropic Rayleigh scattering investigations in smectic \
             A liquid crystal phases. J. Chem. Phys. 101, 6301 (1994).",
            jcp,
        ),
        (
            "Perera C. After the Tsunami: Legal Implications of Mass Burials of Unidentified \
             Victims in Sri Lanka. PLoS Med. 2005;2(6):e185.",
            tsunami,
        ),
        (
            "R. Khandekar, S. Rao, and U. Vazirani, “Graph partitioning using single commodity \
             flows,” J. ACM, vol. 56, no. 4, pp. 1-15, 2009.",
            khandekar,
        ),
        (
            "Park, Robin, Anusha Chidharla, and Kathan Mehta. “Sex-bias in COVID-19-associated \
             illness severity and mortality in cancer patients: A systematic review and \
             meta-analysis.” EClinicalMedicine (2020). \
             https://doi.org/10.1016/j.eclinm.2020.100519",
            eclinm,
        ),
        (
            "Laris, P. On the problems and promises of savanna fire regime change. Nat Commun \
             12, 4891 (2021).",
            laris,
        ),
        (
            "Goldman, J. E., & Schwartz, J. H. (1974). Cellular specificity of serotonin \
             storage and axonal transport in identified neurones of Aplysia californica. The \
             Journal of Physiology, 242(1), 61-76. https://doi.org/10.1113/jphysiol.1974.sp010694",
            goldman,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), &record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
}
