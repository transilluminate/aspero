//! Venues other than a journal: conferences and meetings' proceedings,
//! and book chapters cited as articles or articles as chapters.

use super::*;

#[test]
fn a_paper_merged_with_its_preprint_is_at_the_conference_its_doi_names() {
    // h2-747: the CVPR paper's record merged with its arXiv preprint's,
    // without a venue; its IEEE DOI names the conference.
    let mut merged = arxiv_preprint(
        "doi:10.1109/cvpr.2017.632",
        "Image-to-Image Translation with Conditional Adversarial Networks",
        &["Phillip Isola", "Jun-Yan Zhu", "Tinghui Zhou"],
        2017,
    );
    merged.work.container_title = None;
    merged.ids.push("arxiv:1611.07004".parse().unwrap());
    let cite = |venue: &str| {
        Claim::from_text(format!(
            "Phillip Isola, Jun-Yan Zhu, and Tinghui Zhou. 2017. Image-to-Image Translation with \
             Conditional Adversarial Networks. In {venue}, 5967–5976."
        ))
    };
    let iccv = assess(
        &cite("IEEE International Conference on Computer Vision (ICCV)"),
        &merged,
    );
    assert_eq!(iccv.container.agreement, Agreement::Mismatch, "{iccv:?}");
    assert!(iccv.confidence < VERIFIED, "{iccv:?}");
    let cvpr = assess(
        &cite("Proceedings of the IEEE Conference on Computer Vision and Pattern Recognition"),
        &merged,
    );
    assert!(cvpr.confidence >= VERIFIED, "{cvpr:?}");
    // h2-760: a preprint merged with another repository's copy (a DataCite
    // DOI naming no venue) can't confirm the conference cited.
    let mut batchnorm = arxiv_preprint(
        "arxiv:1502.03167",
        "Batch Normalization: Accelerating Deep Network Training by Reducing Internal \
         Covariate Shift",
        &["Sergey Ioffe", "Christian Szegedy"],
        2015,
    );
    batchnorm.work.container_title = None;
    batchnorm.ids.push("doi:10.57702/o9raffed".parse().unwrap());
    let text = "S. Ioffe and C. Szegedy, \"Batch Normalization: Accelerating Deep Network \
                Training by Reducing Internal Covariate Shift,\" in Advances in Neural \
                Information Processing Systems 28 (NeurIPS 2015), 2015.";
    let a = assess(&Claim::from_text(text), &batchnorm);
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
}

#[test]
fn a_paper_cited_at_another_meeting_is_told_by_its_subject() {
    // h3-890, h3-896, h3-912: no named conference on either side.
    let cite = |authors: &str, title: &str, venue: &str| {
        Claim::from_text(format!("{authors}. {title}. In {venue}"))
    };
    for (n, claim, record) in [
        (
            "h3-890",
            cite(
                "MacCallum, K., & Parsons, D. (2017)",
                "Evaluating a Mobile Toolkit for Designing Mobile Learning Activities",
                "Proceedings of the 2nd International Workshop on User Interface Test \
                 Automation (pp. 1–6).",
            ),
            proceedings(
                "Evaluating a Mobile Toolkit for Designing Mobile Learning Activities",
                &["Kathryn MacCallum", "David Parsons"],
                2017,
                "Proceedings of the 16th World Conference on Mobile and Contextual Learning",
                "1-6",
            ),
        ),
        (
            "h3-896",
            cite(
                "James Kennedy, Paul Baxter, and Tony Belpaeme. 2015",
                "The Robot Who Tried Too Hard",
                "The 26th International ACM SIGACCESS Conference on Computers and \
                 Accessibility (ASSETS '15). 67–74.",
            ),
            proceedings(
                "The Robot Who Tried Too Hard",
                &["James Kennedy", "Paul Baxter", "Tony Belpaeme"],
                2015,
                "Proceedings of the Tenth Annual ACM/IEEE International Conference on \
                 Human-Robot Interaction",
                "67-74",
            ),
        ),
        (
            "h3-912",
            cite(
                "Xiaoyu Wang, Eric Bier, Thomas Butkiewicz, William Ribarsky, and Wenwen Dou. \
                 2011",
                "Designing visual analytics systems for organizational environments",
                "Interaction Design and Children (IDC '11). 1–9.",
            ),
            proceedings(
                "Designing visual analytics systems for organizational environments",
                &[
                    "Xiaoyu Wang",
                    "Eric Bier",
                    "Thomas Butkiewicz",
                    "William Ribarsky",
                    "Wenwen Dou",
                ],
                2011,
                "Proceedings of the 2011 Visual Information Communication - International \
                 Symposium",
                "1-9",
            ),
        ),
    ] {
        let a = assess(&claim, &record);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{n}: {a:?}");
        assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{n}: {a:?}");
    }

    // Names compared by subject, acronyms and furniture aside.
    for (cited, actual, expected) in [
        (
            "Proceedings of the 9th International Conference on Information Communication \
             and Management (ICICM 2013). 129–130.",
            "Proceedings of the 2013 Conference on the Theory of Information Retrieval",
            Some(false),
        ),
        (
            "Fifteenth ACM Conference on Recommender Systems (RecSys '26). 1803–1820.",
            "Proceedings of the 31st International Conference on Intelligent User Interfaces",
            Some(false),
        ),
        (
            "Companion of the 2025 ACM/IEEE International Conference on Human-Robot \
             Interaction (HRI '25). 544–548.",
            "Proceedings of the 2025 International Conference on Artificial Intelligence and \
             Smart Manufacturing",
            Some(false),
        ),
        // An acronym alone, against a name with its own.
        (
            "RecSys '26",
            "CUI 2021 - 3rd Conference on Conversational User Interfaces",
            Some(false),
        ),
        // The same meeting, written otherwise.
        (
            "Proc. IEEE Int. Conf. Robot. Autom., pp. 1–6",
            "2015 IEEE International Conference on Robotics and Automation (ICRA)",
            Some(true),
        ),
        (
            "Proceedings of the 2019 CHI Conference on Human Factors in Computing Systems \
             (CHI '19). Association for Computing Machinery, New York, NY, USA, 1–12.",
            "Proceedings of the 2019 CHI Conference on Human Factors in Computing Systems",
            Some(true),
        ),
        (
            "IEEE/RSJ Int. Conf. Intell. Robots Syst. (IROS)",
            "2021 IEEE/RSJ International Conference on Intelligent Robots and Systems (IROS)",
            Some(true),
        ),
        (
            "HRI '15",
            "Proceedings of the Tenth Annual ACM/IEEE International Conference on \
             Human-Robot Interaction (HRI)",
            Some(true),
        ),
        // An acronym against a name without one: can't be told.
        (
            "ASSETS '15",
            "Proceedings of the 17th International ACM SIGACCESS Conference on Computers & \
             Accessibility",
            None,
        ),
        (
            "Proc. UIST",
            "Proceedings of the 28th Annual ACM Symposium on User Interface Software & \
             Technology",
            None,
        ),
    ] {
        assert_eq!(
            publication::proceedings_agree(cited, actual),
            expected,
            "{cited} | {actual}"
        );
    }

    // Cited at its own meeting, however written, it still verifies.
    let hri = proceedings(
        "The Robot Who Tried Too Hard",
        &["James Kennedy", "Paul Baxter", "Tony Belpaeme"],
        2015,
        "Proceedings of the Tenth Annual ACM/IEEE International Conference on Human-Robot \
         Interaction",
        "67-74",
    );
    for venue in [
        "Proceedings of the Tenth Annual ACM/IEEE International Conference on Human-Robot \
         Interaction (HRI '15). 67–74.",
        "Proc. ACM/IEEE Int. Conf. Human-Robot Interaction, pp. 67–74, 2015.",
        "HRI '15. 67–74.",
    ] {
        let claim = cite(
            "James Kennedy, Paul Baxter, and Tony Belpaeme. 2015",
            "The Robot Who Tried Too Hard",
            venue,
        );
        let a = assess(&claim, &hri);
        assert!(a.confidence >= VERIFIED, "{venue}: {a:?}");
    }
    // A journal called "Proceedings of …" is held as a journal.
    let pnas = article(
        "Neural networks and physical systems with emergent collective computational abilities",
        &["J J Hopfield"],
        1982,
        (
            "Proceedings of the National Academy of Sciences",
            "79",
            "2554-2558",
        ),
    );
    let a = assess(
        &Claim::from_text(
            "Hopfield JJ. Neural networks and physical systems with emergent collective \
             computational abilities. Proc Natl Acad Sci U S A. 1982;79(8):2554-8.",
        ),
        &pnas,
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_journal_article_cited_in_a_meetings_proceedings_is_another_venue() {
    // h3-908: the HRI paper's namesake in a journal, found for a citation
    // of a symposium's proceedings.
    let journal = article(
        "Is a Robot better than Video for Initiating Remote Social Connections among Children?",
        &["Nuri Kim", "Jeonghye Han", "Wendy Ju"],
        2014,
        (
            "Journal of Institute of Control, Robotics and Systems",
            "20",
            "208-209",
        ),
    );
    let claim = Claim::from_text(
        "Kim, N., Han, J., & Ju, W. (2014). Is a robot better than video for initiating remote \
         social connections among children? In Proceedings of the 8th International Symposium \
         on Visual Information Communication and Interaction (pp. 208–209).",
    );
    let a = assess(&claim, &journal);
    assert_eq!(a.container.agreement, Agreement::Mismatch, "{a:?}");
    assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{a:?}");
    // Cited in its journal, it verifies.
    let claim = Claim::from_text(
        "Kim, N., Han, J., & Ju, W. (2014). Is a robot better than video for initiating remote \
         social connections among children? Journal of Institute of Control, Robotics and \
         Systems, 20, 208–209.",
    );
    let a = assess(&claim, &journal);
    assert!(a.confidence >= VERIFIED, "{a:?}");
    // A journal called "Proceedings of …", abbreviated, isn't a meeting.
    let prsb = article(
        "Rapid evolution of a sexually selected trait",
        &["Alice Smith-Holloway"],
        2003,
        (
            "Proceedings of the Royal Society B: Biological Sciences",
            "270",
            "1234-1240",
        ),
    );
    let claim = Claim::from_text(
        "Smith-Holloway A. Rapid evolution of a sexually selected trait. In Proc. R. Soc. B \
         270, 1234–1240 (2003).",
    );
    let a = assess(&claim, &prsb);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn a_chapter_cited_as_a_journals_article_is_plausible() {
    let methane = chapter(
        "Methane Cycling and Climate",
        &["Han Dolman"],
        2019,
        "Biogeochemical Cycles and Climate",
    );
    let psychotherapy = chapter(
        "Integrative Psychotherapy with Children",
        &["Athena A. Drewes", "John W. Seymour"],
        2019,
        "Handbook of Psychotherapy Integration",
    );
    let culpability = chapter(
        "Law, Neuroscience, and Criminal Culpability",
        &["Lisa Claydon"],
        2011,
        "Law and Neuroscience",
    );
    let structured = Claim {
        title: Some("Law, Neuroscience, and Criminal Culpability".into()),
        authors: vec!["Lisa Claydon".into()],
        year: Some(2011),
        container: Some("Journal of Neuroscience".into()),
        volume: Some("60".into()),
        page: Some("141-170".into()),
        ..Claim::default()
    };
    for (n, claim, record, venue) in [
        // The book's title as the journal, with a volume and issue.
        (
            "h3-947",
            Claim::from_text(
                "Dolman H. Methane Cycling and Climate. Biogeochemical Cycles and Climate. \
                 2019;23(5):159-175.",
            ),
            &methane,
            Agreement::Match,
        ),
        (
            "h3-948 (APA)",
            Claim::from_text(
                "Dolman, H. (2019). Methane Cycling and Climate. Biogeochemical Cycles and \
                 Climate, 23(5), 159–175.",
            ),
            &methane,
            Agreement::Match,
        ),
        // A real journal of the field.
        (
            "h3-964",
            Claim::from_text(
                "Drewes AA, Seymour JW. Integrative Psychotherapy with Children. Psychotherapy \
                 Research. 2019;54(12):341-356.",
            ),
            &psychotherapy,
            Agreement::Mismatch,
        ),
        ("h3-959", structured, &culpability, Agreement::Mismatch),
    ] {
        let a = assess(&claim, record);
        assert_eq!(a.container.agreement, venue, "{n}: {a:?}");
        assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{n}: {a:?}");
    }
    // Cited as a chapter, or in its series' volume, it verifies.
    let mut lncs = chapter(
        "U-Net: Convolutional Networks for Biomedical Image Segmentation",
        &["Olaf Ronneberger", "Philipp Fischer", "Thomas Brox"],
        2015,
        "Lecture Notes in Computer Science",
    );
    lncs.work.page = Some("234-241".into());
    for (claim, record) in [
        (
            "Dolman H. Methane Cycling and Climate. In: Dolman H, ed. Biogeochemical Cycles and \
             Climate. Oxford University Press; 2019:159-175.",
            &methane,
        ),
        (
            "Ronneberger O, Fischer P, Brox T. U-Net: Convolutional Networks for Biomedical \
             Image Segmentation. Lecture Notes in Computer Science, vol. 9351, pp. 234–241. \
             Springer, 2015.",
            &lncs,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
    // Nor does the book, its title where the journal stands.
    let mut book = record(
        &[],
        "Biogeochemical Cycles and Climate",
        &["Han Dolman"],
        2019,
    );
    book.work.work_type = Some(crate::work::WorkType::Book);
    let a = assess(
        &Claim::from_text(
            "Dolman H. Methane Cycling and Climate. Biogeochemical Cycles and Climate. \
             2019;23(5):159-175.",
        ),
        &book,
    );
    assert!(a.confidence < VERIFIED, "{a:?}");
    let a = assess(
        &Claim::from_text("Dolman H. Biogeochemical Cycles and Climate. Oxford: OUP; 2019."),
        &book,
    );
    assert!(a.confidence >= VERIFIED, "{a:?}");
    assert!(publication::cites_issue(
        "Psychotherapy Research. 2019;54(12):341-356."
    ));
    assert!(publication::cites_issue(
        "Psychotherapy Research, 54(12), 341–356."
    ));
    assert!(!publication::cites_issue("Injury 47 (2016) 797–804"));
    assert!(!publication::cites_issue("N Engl J Med. 2017;376:2545-55."));
}

#[test]
fn a_chapter_cited_in_its_book_as_a_journals_volume_is_plausible() {
    // holdout-6's `w:chapter-as-article`: the book named as the journal,
    // a volume without an issue.
    let migration = chapter(
        "6 Life after Migration: Japanese Immigrants’ Experience of Migration",
        &["Jun Nagatomo"],
        2014,
        "Migration as Transnational Leisure",
    );
    let crops = chapter(
        "Crop physiology: applications for breeding and agronomy",
        &["Victor O. Sadras", "Daniel F. Calderini"],
        2015,
        "Crop Physiology",
    );
    let africa = chapter(
        "Structural Adjustment – by the World Bank",
        &["Michael Barratt Brown"],
        2019,
        "Africa’s Choices",
    );
    for (n, claim, record) in [
        (
            "h6-89 (APA)",
            "Nagatomo, J. (2014). 6 Life after Migration: Japanese Immigrants’ Experience of \
             Migration. Migration as Transnational Leisure, 33, 128–178.",
            &migration,
        ),
        (
            "h6-583 (Vancouver)",
            "Sadras VO, Calderini DF. Crop physiology: applications for breeding and agronomy. \
             Crop Physiology. 2015;35:1-14.",
            &crops,
        ),
        (
            "h6-745 (Harvard)",
            "Brown, M.B. (2019) 'Structural Adjustment – by the World Bank', Africa’s Choices, \
             34, pp. 65–82.",
            &africa,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert_eq!(a.container.agreement, Agreement::Match, "{n}: {a:?}");
        assert_eq!(a.volume.agreement, Agreement::Mismatch, "{n}: {a:?}");
        assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{n}: {a:?}");
    }
    // A series' chapter at its volume, which its record lacks, verifies:
    // labelled, named as a series, or named by its DOI.
    let mut lncs = chapter(
        "U-Net: Convolutional Networks for Biomedical Image Segmentation",
        &["Olaf Ronneberger", "Philipp Fischer", "Thomas Brox"],
        2015,
        "Lecture Notes in Computer Science",
    );
    lncs.work.page = Some("234-241".into());
    let mut aesp = chapter(
        "The attachment behavioral system in adulthood: Activation, psychodynamics, and \
         interpersonal processes",
        &["Mario Mikulincer", "Phillip R. Shaver"],
        2003,
        "Advances in experimental social psychology",
    );
    aesp.ids = vec!["doi:10.1016/s0065-2601(03)01002-5".parse().unwrap()];
    aesp.work.page = Some("53-152".into());
    for (claim, record) in [
        (
            "Ronneberger, O., Fischer, P., & Brox, T. (2015). U-Net: Convolutional Networks for \
             Biomedical Image Segmentation. Lecture Notes in Computer Science, 9351, 234–241.",
            &lncs,
        ),
        (
            "Mikulincer, M., & Shaver, P. R. (2003). The attachment behavioral system in \
             adulthood: Activation, psychodynamics, and interpersonal processes. Advances in \
             Experimental Social Psychology, 35, 53–152.",
            &aesp,
        ),
        (
            "Mikulincer, M., & Shaver, P. R. (2003). The attachment behavioral system in \
             adulthood: Activation, psychodynamics, and interpersonal processes. Advances in \
             Experimental Social Psychology, 35, 53–152. \
             https://doi.org/10.1016/S0065-2601(03)01002-5",
            &aesp,
        ),
        // Cited as a chapter, in its book.
        (
            "Nagatomo, J. (2014). 6 Life after Migration: Japanese Immigrants’ Experience of \
             Migration. In: Migration as Transnational Leisure. Leiden: Brill, pp. 128–178.",
            &migration,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
    assert!(publication::cites_journal_place(
        "Crop Physiology, 35, 1–14."
    ));
    assert!(!publication::cites_journal_place(
        "Lecture Notes in Computer Science, vol. 9351, pp. 234–241."
    ));
}

#[test]
fn a_meeting_of_the_same_society_cited_is_another_venue() {
    // holdout-4's `w:conference-same-society`: another meeting of the
    // society holding the record's (AAAI's ICWSM for its AAAI conference),
    // or of its field, sharing words with it.
    let mut aaai = proceedings(
        "Sparsity-Guided Holistic Explanation for LLMs with Interpretable Inference-Time \
         Intervention",
        &["Zhen Tan", "Tianlong Chen", "Zhenyu Zhang"],
        2024,
        "Proceedings of the AAAI Conference on Artificial Intelligence",
        "21619-21627",
    );
    // OpenAlex types AAAI's proceedings as a journal's articles.
    aaai.work.work_type = Some(crate::work::WorkType::JournalArticle);
    aaai.work.volume = Some("38".into());
    let mut icwsm = proceedings(
        "Link Me Baby One More Time: Social Music Discovery on Spotify",
        &["Shazia Babul", "Desislava Hristova", "Antonio Lima"],
        2025,
        "Proceedings of the International AAAI Conference on Web and Social Media",
        "146-159",
    );
    icwsm.work.work_type = Some(crate::work::WorkType::JournalArticle);
    let mwscas = proceedings(
        "Micropower class AB CMOS current conveyor based on quasi-floating gate techniques",
        &["A. J. Lopez-Martin", "L. Acosta", "J. M. Algueta"],
        2009,
        "2009 52nd IEEE International Midwest Symposium on Circuits and Systems",
        "140-143",
    );
    let iscas = proceedings(
        "All-digital frequency and clock synthesis architectures from a signals and systems \
         perspective, current state and future directions",
        &["P. P. Sotiriadis"],
        2010,
        "Proceedings of 2010 IEEE International Symposium on Circuits and Systems",
        "233-236",
    );
    let mm_asia = proceedings(
        "PMCMatcher: A Parallel Multi-Scale Cascaded Transformer-Based Network for Multimodal \
         Feature Matching",
        &["Yun Liao", "Jiayi Lyu", "Junhui Liu"],
        2025,
        "Proceedings of the 7th ACM International Conference on Multimedia in Asia",
        "1-8",
    );
    // A repository's copy: no venue, volume or page.
    let mut copy = record(
        &["doi:10.5445/ir/1000029612"],
        "Towards Social Integration of Humanoid Robots by Conversational Concept Learning",
        &["F. Kraft", "K. Kilgour", "R. Saam"],
        2011,
    );
    copy.work.work_type = Some(crate::work::WorkType::JournalArticle);
    // A talk's record at DataCite (Underline's), typed an article.
    let mut talk = record(
        &["doi:10.48448/z49g-ka35"],
        "Attack On Prompt: Backdoor Attack in Prompt-Based Continual Learning",
        &["Thien Trang Nguyen Vu", "Anh Tuan Tran", "Nhat Ho"],
        2025,
    );
    talk.work.work_type = Some(crate::work::WorkType::Preprint);
    for (n, claim, record) in [
        (
            "h4-784",
            "T. Nguyen, A. Tran, and N. Ho, \"Attack on Prompt: Backdoor Attack in Prompt-Based \
             Continual Learning,\" in Proceedings of the International AAAI Conference on Web \
             and Social Media (ICWSM-25), 2025, pp. 19686–19694.",
            &talk,
        ),
        (
            "h4-771",
            "Tan, Z., Chen, T., and Zhang, Z. 2024. Sparsity-Guided Holistic Explanation for \
             LLMs with Interpretable Inference-Time Intervention. In Proceedings of the \
             International AAAI Conference on Web and Social Media (ICWSM-24), 21619–21627.",
            &aaai,
        ),
        (
            "h4-793",
            "Babul, S., Hristova, D., and Lima, A. 2025. Link Me Baby One More Time: Social \
             Music Discovery on Spotify. In Proceedings of the AAAI Conference on Artificial \
             Intelligence (AAAI-25), 146–159.",
            &icwsm,
        ),
        (
            "h4-776",
            "A. J. Lopez-Martin, L. Acosta, and J. M. Algueta, \"Micropower class AB CMOS \
             current conveyor based on quasi-floating gate techniques,\" in Proc. 2009 IEEE \
             International Conference on Electronics, Circuits and Systems (ICECS), 2009, pp. \
             140–143.",
            &mwscas,
        ),
        (
            "h4-788",
            "P. P. Sotiriadis, \"All-digital frequency and clock synthesis architectures from a \
             signals and systems perspective, current state and future directions,\" in Proc. \
             2010 IEEE International Conference on Electronics, Circuits and Systems (ICECS), \
             2010, pp. 233–236.",
            &iscas,
        ),
        (
            "h4-780",
            "@inproceedings{liao2025, author = {Liao, Yun and Lyu, Jiayi and Liu, Junhui}, title \
             = {PMCMatcher: A Parallel Multi-Scale Cascaded Transformer-Based Network for \
             Multimodal Feature Matching}, booktitle = {Proceedings of the 2025 International \
             Conference on Multimedia Retrieval (ICMR '25)}, year = {2025}, pages = {1--8}}",
            &mm_asia,
        ),
        (
            "h4-792",
            "F. Kraft, K. Kilgour, and R. Saam, \"Towards social integration of humanoid robots \
             by conversational concept learning,\" in Proc. 2010 IEEE International Conference \
             on Robotics and Automation (ICRA), 2010, pp. 352–357.",
            &copy,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{n}: {a:?}");
        assert!((PLAUSIBLE..VERIFIED).contains(&a.confidence), "{n}: {a:?}");
    }
    // At their own meetings, however written, they verify.
    for (claim, record) in [
        (
            "Tan, Z., Chen, T., and Zhang, Z. 2024. Sparsity-Guided Holistic Explanation for \
             LLMs with Interpretable Inference-Time Intervention. In Proceedings of the AAAI \
             Conference on Artificial Intelligence (AAAI-24), 21619–21627.",
            &aaai,
        ),
        (
            "Babul, S., Hristova, D., and Lima, A. 2025. Link Me Baby One More Time: Social \
             Music Discovery on Spotify. In Proceedings of the International AAAI Conference \
             on Web and Social Media (ICWSM-25), 146–159.",
            &icwsm,
        ),
        (
            "A. J. Lopez-Martin, L. Acosta, and J. M. Algueta, \"Micropower class AB CMOS \
             current conveyor based on quasi-floating gate techniques,\" in Proc. 52nd IEEE \
             International Midwest Symposium on Circuits and Systems (MWSCAS), 2009, pp. \
             140–143.",
            &mwscas,
        ),
        (
            "P. P. Sotiriadis, \"All-digital frequency and clock synthesis architectures from a \
             signals and systems perspective, current state and future directions,\" in Proc. \
             2010 IEEE International Symposium on Circuits and Systems (ISCAS), 2010, pp. \
             233–236.",
            &iscas,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
    // An acronym the name can make, its letters among its words' initials.
    for (cited, actual, expected) in [
        (
            "Proceedings of the 2019 AAAI/ACM Conference on AI, Ethics, and Society (AIES '19)",
            "Proceedings of the AAAI/ACM Conference on AI, Ethics, and Society",
            Some(true),
        ),
        (
            "Proceedings of the 2nd Conference on Causal Learning and Reasoning, PMLR 213",
            "Conference on Causal Learning and Reasoning",
            Some(true),
        ),
        (
            "Proceedings of the International AAAI Conference on Web and Social Media \
             (ICWSM-22)",
            "Proceedings of the AAAI Conference on Artificial Intelligence",
            Some(false),
        ),
    ] {
        assert_eq!(
            publication::proceedings_agree(cited, actual),
            expected,
            "{cited} | {actual}"
        );
    }
}

#[test]
fn a_journal_article_cited_as_a_chapter_is_another_container() {
    // holdout-4's `w:article-as-chapter`: a journal's article cited in an
    // edited book, the reverse of round 4's chapter cited as an article.
    let midwifery = article(
        "The use of amnioinfusion in nurse-midwifery practice",
        &["Barbara Snell"],
        1993,
        ("Journal of Nurse-Midwifery", "38", "S62-S71"),
    );
    let forensic = article(
        "Nurses and Medicolegal Death Investigation",
        &["Stacy A. Drake", "Yijiong Yang"],
        2020,
        ("Journal of Forensic Nursing", "16", "207-214"),
    );
    let ecology = article(
        "Concerning the Ecology of Urtica Dioica L.",
        &["J. D. Ivins"],
        1952,
        ("Journal of Ecology", "40", "380"),
    );
    for (claim, record) in [
        (
            "SNELL B. The use of amnioinfusion in nurse-midwifery practice. In: Corvaro IR, \
             Mirkrand P, editors. Handbook of Nurse-Midwifery. Elsevier BV; 1993. p. S62-S71.",
            &midwifery,
        ),
        (
            "Drake, S.A. et al. (2020) 'Nurses and Medicolegal Death Investigation', in \
             Korvanne, E.T., Grathwell, A. (eds) Handbook of Forensic Nursing. Ovid \
             Technologies (Wolters Kluwer Health). pp. 207–214.",
            &forensic,
        ),
        (
            "Ivins (1952). Concerning the Ecology of Urtica Dioica L. In E. Ulmova, O. S. \
             Grathquist (Eds.), Handbook of Ecology (pp. 380). JSTOR.",
            &ecology,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{claim}: {a:?}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{claim}: {a:?}"
        );
    }
    // In its journal, or in a series' volume of that name, it verifies; so
    // does an edition's "2nd ed." or a prose "in".
    let mut enzymology = article(
        "Purification of membrane proteins",
        &["Jane Doe-Alvarez"],
        1990,
        ("Methods in Enzymology", "182", "253-264"),
    );
    enzymology.work.work_type = Some(crate::work::WorkType::JournalArticle);
    for (claim, record) in [
        (
            "Snell B. The use of amnioinfusion in nurse-midwifery practice. J Nurse Midwifery. \
             1993;38:S62-S71.",
            &midwifery,
        ),
        (
            "Doe-Alvarez J. Purification of membrane proteins. In: Deutscher MP, editor. \
             Methods in Enzymology. Vol 182. 1990. p. 253-264.",
            &enzymology,
        ),
    ] {
        let a = assess(&Claim::from_text(claim), record);
        assert!(a.confidence >= VERIFIED, "{claim}: {a:?}");
    }
}
