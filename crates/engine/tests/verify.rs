//! `resolve` and `verify` against scripted providers that replay the
//! situations seen live (DESIGN §9): lookalike re-deposits, citation
//! matchers that lack DataCite DOIs, invented and misattached DOIs.

mod common;

use std::sync::Arc;

use aspero_core::{
    Agreement, Capabilities, Claim, Error, Page, Provider, ProviderId, ResiliencePolicy, Response,
    Result, SearchMode, SearchQuery, WorkId,
};
use aspero_engine::{CitedField, Engine, Reason, Verdict, Via, WorkKind};
use async_trait::async_trait;
use common::*;

#[tokio::test]
async fn a_lookalike_from_the_matcher_leads_to_the_real_paper() {
    let (engine, openalex, _) = live_like();
    let v = engine
        .verify(
            &Claim::from_text("Vaswani 2017 Attention is all you need"),
            5,
        )
        .await;

    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(best.ids.contains(&"arXiv:1706.03762".parse().unwrap()));
    // The title the reference cites, located via the matcher's (lookalike)
    // hit, became the OpenAlex query.
    assert_eq!(
        openalex.queries(),
        [(SearchMode::Keywords, "attention is all you need".to_owned())]
    );
    let lookalike = v
        .report
        .candidates
        .iter()
        .find(|c| c.record.work.issued.unwrap().year() == 2025)
        .unwrap();
    assert_eq!(lookalike.assessment.year.agreement, Agreement::Mismatch);
}

#[tokio::test]
async fn verify_skips_paid_searches_once_the_claimed_doi_verifies() {
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![deep_learning()]),
    );
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning()]),
    );
    let engine = engine(vec![openalex.clone(), crossref]);
    let claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. doi:10.1038/nature14539",
    );

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(openalex.queries().is_empty(), "no OpenAlex search needed");

    // resolve, by contrast, gathers everything.
    engine.resolve(&claim, 5).await;
    assert_eq!(openalex.queries().len(), 1);
}

#[tokio::test]
async fn one_matchers_exact_hit_does_not_stop_the_search_for_the_original() {
    // A journal's digest of the paper: same title and lead authors, later.
    let dated = |mut r: aspero_core::SourceRecord, month| {
        r.work.issued = aspero_core::PartialDate::new(2015, Some(month), None);
        r
    };
    let digest = dated(
        work(
            "crossref",
            &["10.9999/digest"],
            "Deep learning",
            &LECUN,
            2015,
        ),
        9,
    );
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![dated(deep_learning(), 5)]),
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![digest]));
    let engine = engine(vec![openalex.clone(), crossref]);
    let claim = Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. 2015.");

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert_eq!(openalex.queries().len(), 1);
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(best.ids.contains(&"10.1038/nature14539".parse().unwrap()));
}

#[tokio::test]
async fn an_invented_doi_is_a_mismatch_pointing_at_the_real_work() {
    let (engine, _, _) = live_like();
    let claim = Claim::from_text(
        "LeCun, Bengio, Hinton. Deep learning. Nature 2015. doi:10.1038/nature99999",
    );
    let crossref_matches_real = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let engine2 = self::engine(vec![crossref_matches_real]);

    let v = engine2.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(v.reason.contains("does not exist"), "{}", v.reason);
    assert!(v.reason.contains("10.1038/nature14539"), "{}", v.reason);
    assert_eq!(v.why, Reason::IdMissingElsewhere);

    // With no matching work anywhere, it's simply not found.
    let nothing =
        Claim::from_text("Smith J. A paper that was never written. 2019. doi:10.9999/nope");
    let v = engine.verify(&nothing, 5).await;
    assert_eq!(v.verdict, Verdict::NotFound, "{}", v.reason);
    assert_eq!(v.why, Reason::IdMissing);
}

#[tokio::test(start_paused = true)]
async fn a_verifying_lookup_does_not_wait_for_slow_matchers() {
    let openalex = Arc::new(Scripted::new("openalex").with(vec![deep_learning()]));
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .slow(std::time::Duration::from_secs(60)),
    );
    let engine = engine(vec![openalex, crossref]);
    let claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. doi:10.1038/nature14539",
    );

    let start = tokio::time::Instant::now();
    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(start.elapsed() < std::time::Duration::from_secs(60));

    // Without an identifier to settle it, the matcher is waited for.
    let start = tokio::time::Instant::now();
    let text =
        Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.");
    engine.verify(&text, 5).await;
    assert!(start.elapsed() >= std::time::Duration::from_secs(60));
}

#[tokio::test(start_paused = true)]
async fn a_match_two_providers_agree_on_does_not_wait_for_slow_searches() {
    let minute = std::time::Duration::from_secs(60);
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![deep_learning(), deep_learning_for_ai()]),
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let europepmc = Arc::new(
        Scripted::new("europepmc")
            .keyword()
            .with(vec![deep_learning()])
            .slow(minute),
    );
    let engine = engine(vec![openalex, crossref, europepmc]);

    let start = tokio::time::Instant::now();
    let text =
        Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444.");
    let v = engine.verify(&text, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(start.elapsed() < minute);

    // A DOI naming another work is settled the same way.
    let start = tokio::time::Instant::now();
    let claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1145/3448250",
    );
    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(matches!(v.why, Reason::IdElsewhere { .. }), "{:?}", v.why);
    assert!(start.elapsed() < minute);
}

#[tokio::test]
async fn a_doi_broken_at_a_hyphen_is_tried_without_it() {
    let (engine, _, _) = live_like();
    let mut claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1038/nat-ure14539",
    );
    claim.alternatives = vec![aspero_core::Alternative::new(
        "10.1038/nat-ure14539".parse().unwrap(),
        "10.1038/nature14539".parse().unwrap(),
    )];

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(
        v.reason.starts_with("doi:10.1038/nature14539"),
        "{}",
        v.reason
    );
}

#[tokio::test]
async fn an_alternative_is_ignored_when_the_printed_doi_exists() {
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning(), deep_learning_for_ai()]),
    );
    let engine = engine(vec![crossref]);
    // The printed DOI is real (the 2021 CACM article); the alternative
    // happens to be the cited paper's, but the printed one was meant.
    let mut claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1145/3448250",
    );
    claim.alternatives = vec![aspero_core::Alternative::new(
        "10.1145/3448250".parse().unwrap(),
        "10.1038/nature14539".parse().unwrap(),
    )];

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(matches!(v.why, Reason::IdElsewhere { .. }), "{:?}", v.why);
    assert!(
        v.reason.starts_with("doi:10.1145/3448250 is"),
        "{}",
        v.reason
    );
}

#[tokio::test]
async fn a_real_doi_attached_to_the_wrong_citation_is_a_mismatch() {
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning(), deep_learning_for_ai()]),
    );
    let engine = engine(vec![crossref]);
    // The DOI is real but belongs to the 2021 CACM article.
    let claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015. doi:10.1145/3448250",
    );

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(v.reason.contains("Deep learning for AI"), "{}", v.reason);
    let Reason::IdElsewhere { cited } = v.why else {
        panic!("{:?}", v.why)
    };
    assert_eq!(
        v.report.candidates[cited].record.work.title.as_deref(),
        Some("Deep learning for AI")
    );
    let best = &v.report.candidates[v.best.unwrap()];
    assert_eq!(
        best.record.ids,
        ["10.1038/nature14539".parse::<WorkId>().unwrap()]
    );
}

#[tokio::test]
async fn the_right_paper_with_the_wrong_year_is_a_mismatch_not_a_miss() {
    let (engine, _, _) = live_like();
    let claim = Claim::from_text("Vaswani A, Shazeer N. Attention is all you need. NeurIPS 2019.");

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Mismatch, "{}", v.reason);
    assert!(v.reason.contains("year (2017)"), "{}", v.reason);
    let year = v.why.differs().iter().find(|d| d.field == CitedField::Year);
    assert_eq!(year.unwrap().actual.as_deref(), Some("2017"));
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(best.ids.contains(&"arXiv:1706.03762".parse().unwrap()));
}

#[tokio::test]
async fn the_cited_doi_with_the_wrong_year_partly_matches() {
    let (engine, _, _) = live_like();
    let claim = Claim::from_text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2011. doi:10.1038/nature14539",
    );

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason);
    let Reason::IdPartlyMatches { differs } = &v.why else {
        panic!("{:?}", v.why)
    };
    assert_eq!(differs.len(), 1, "{differs:?}");
    assert_eq!(differs[0].field, CitedField::Year);
    assert_eq!(differs[0].actual.as_deref(), Some("2015"));
}

#[tokio::test]
async fn identifier_only_claims_check_existence_without_searching() {
    let (engine, openalex, crossref) = live_like();
    let mut claim = Claim::default();
    claim.ids = vec!["10.1038/nature14539".parse().unwrap()];

    let v = engine.verify(&claim, 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    assert!(v.reason.contains("exists"));
    assert_eq!(v.why, Reason::IdExists);
    assert!(openalex.queries().is_empty() && crossref.queries().is_empty());
}

#[tokio::test]
async fn records_sharing_an_identifier_merge_into_one_candidate() {
    let openalex = Arc::new(
        Scripted::new("openalex")
            .keyword()
            .with(vec![deep_learning()]),
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let engine = engine(vec![openalex, crossref]);

    let report = engine
        .resolve(&Claim::from_text("Deep learning, LeCun 2015"), 5)
        .await;

    assert_eq!(report.candidates.len(), 1);
    let vias: Vec<Via> = report.candidates[0]
        .sightings
        .iter()
        .map(|s| s.via)
        .collect();
    assert_eq!(vias, [Via::CitationMatch, Via::TitleSearch]);
}

#[tokio::test]
async fn provider_failures_make_an_empty_result_unverifiable() {
    struct Down(Capabilities);
    #[async_trait]
    impl Provider for Down {
        fn id(&self) -> ProviderId {
            ProviderId::from_static("down")
        }
        fn capabilities(&self) -> &Capabilities {
            &self.0
        }
        async fn search(&self, _: &SearchQuery) -> Result<Response<Page>> {
            Err(Error::AuthRejected { message: None })
        }
    }
    let mut caps = Capabilities::default();
    caps.search = true;
    caps.citation_search = true;
    let engine = Engine::builder()
        .provider(Arc::new(Down(caps)), ResiliencePolicy::default(), 0)
        .build();

    let v = engine
        .verify(&Claim::from_text("Anything at all, 2020"), 5)
        .await;
    assert_eq!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
    assert_eq!(v.why, Reason::ProvidersFailed);
    assert_eq!(v.report.errors.len(), 1);
    assert_eq!(
        v.report.errors[0].kind(),
        aspero_core::ErrorKind::KeyRejected
    );
}

#[tokio::test]
async fn abstracts_are_filled_from_other_providers_with_provenance() {
    // OpenAlex (first) knows the work without an abstract; Crossref has one.
    let openalex =
        Arc::new(Scripted::new("openalex").with(vec![deep_learning(), attention_real()]));
    let crossref = Arc::new(Scripted::new("crossref").with(vec![
        with_abstract(
            deep_learning(),
            "Deep learning allows computational models…",
        ),
        with_abstract(attention_lookalike(), "unrelated"),
    ]));
    let engine = engine(vec![openalex, crossref]);

    let has_own = with_abstract(
        attention_real(),
        "The dominant sequence transduction models…",
    );
    let mut lacks = deep_learning();
    lacks.source = ProviderId::from_static("openalex");
    let nowhere = work("openalex", &["10.1000/no-abstract"], "Obscure", &[], 1999);

    let report = engine.abstracts(&[&lacks, &has_own, &nowhere]).await;
    let found: Vec<Option<(String, String)>> = report
        .found
        .iter()
        .map(|a| {
            a.as_ref()
                .map(|a| (a.source.to_string(), a.text.chars().take(13).collect()))
        })
        .collect();
    assert_eq!(
        found,
        [
            Some(("crossref".into(), "Deep learning".into())),
            Some(("openalex".into(), "The dominant ".into())),
            None,
        ]
    );
}

#[tokio::test]
async fn a_reference_without_a_title_is_found_by_where_it_appeared() {
    // #23: a running header, nothing but journal, volume, year and pages.
    let mut laan = work(
        "openalex",
        &["doi:10.1016/j.injury.2015.11.045"],
        "Chest wall thickness and decompression failure",
        &["Laan"],
        2016,
    );
    laan.work.container_title = Some("Injury".into());
    laan.work.volume = Some("47".into());
    laan.work.page = Some("797-804".into());
    let openalex = Arc::new(Scripted::new("openalex").placer().with(vec![laan]));
    let engine = engine(vec![openalex.clone()]);
    let v = engine
        .verify(
            &Claim::from_text("Injury, Int. J. Care Injured 47 (2016) 797\u{2013}804"),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    let found = &v.report.candidates[v.best.unwrap()];
    assert!(found.sightings.iter().any(|s| s.via == Via::PlaceSearch));
    let placed: Vec<_> = openalex
        .sent()
        .into_iter()
        .filter(|q| q.place.is_some())
        .collect();
    let [q] = &placed[..] else {
        panic!("{placed:?}")
    };
    let place = q.place.as_ref().unwrap();
    assert_eq!(
        (place.volume.as_str(), place.first_page.as_str()),
        ("47", "797")
    );
    let years = q.years.unwrap();
    assert_eq!((years.from, years.to), (Some(2015), Some(2017)));
}

#[tokio::test]
async fn extreme_years_do_not_overflow() {
    let mut ancient = work("openalex", &["doi:10.1234/old"], "Old", &["Laan"], -1);
    ancient.work.container_title = Some("Injury".into());
    ancient.work.volume = Some("47".into());
    ancient.work.page = Some("797-804".into());
    let openalex = Arc::new(Scripted::new("openalex").placer().with(vec![ancient]));
    let engine = engine(vec![openalex.clone()]);
    let mut claim = Claim::from_text("Injury, Int. J. Care Injured 47 (2016) 797\u{2013}804");
    claim.year = Some(i16::MAX);
    let v = engine.verify(&claim, 5).await;
    assert_ne!(v.verdict, Verdict::Verified, "{}", v.reason);
    let years = openalex.sent().iter().find_map(|q| q.years).unwrap();
    assert_eq!((years.from, years.to), (Some(i16::MAX - 1), Some(i16::MAX)));
}

#[tokio::test]
async fn agreeing_without_corroboration_is_unconfirmed() {
    // A title and year that agree, with nothing to corroborate them.
    let paper = work(
        "crossref",
        &["10.1234/x"],
        "Fibrinolysis for patients with intermediate-risk pulmonary embolism",
        &[],
        2014,
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![paper]));
    let v = engine(vec![crossref])
        .verify(
            &Claim::from_text(
                "Fibrinolysis for patients with intermediate-risk pulmonary embolism. 2014",
            ),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason);
    assert_eq!(v.why, Reason::Unconfirmed);
    assert_eq!(v.why.as_str(), "unconfirmed");
}

#[tokio::test]
async fn a_reply_with_the_cited_title_is_named_as_one() {
    let reply = work(
        "crossref",
        &["10.1038/nature14539-reply"],
        "Deep learning\u{2014}Reply",
        &LECUN,
        2015,
    );
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![reply]));
    let v = engine(vec![crossref])
        .verify(
            &Claim::from_text("LeCun Y, Bengio Y, Hinton G. Deep learning. 2015"),
            5,
        )
        .await;
    let kind = v.why.differs().iter().find(|d| d.field == CitedField::Kind);
    assert_eq!(
        kind.and_then(|d| d.actual.as_deref()),
        Some("reply"),
        "{:?}: {}",
        v.why,
        v.reason
    );
    assert_eq!(kind.and_then(|d| d.kind), Some(WorkKind::Reply));
}

#[tokio::test]
async fn reasons_that_list_differences_always_list_one() {
    // Every pairing of a claim shape with a candidate: whatever the verdict,
    // a reason with differences names at least one.
    let candidates = [
        deep_learning(),
        work(
            "crossref",
            &["10.1234/c"],
            "Correction: Deep learning",
            &LECUN,
            2015,
        ),
        work(
            "crossref",
            &["10.1234/r"],
            "Deep learning—Reply",
            &LECUN,
            2016,
        ),
        work("crossref", &["10.1234/n"], "Deep learning", &[], 2015),
        work("crossref", &["10.1234/s"], "Aspero", &[], 2012),
    ];
    let claims = [
        "Deep learning",
        "Deep learning. 2015",
        "Deep learning. 2016",
        "Deep learning. 2019",
        "LeCun Y. Deep learning. 2015",
        "LeCun Y. Deep learning. 2017",
        "Smith J. Deep learning. 2015",
        "Smith J. Deep learning. Nature 2015;521:436",
        "LeCun Y. Nature 2015;521:436",
        "Deep learning. doi:10.1038/nature14539",
        "Deep learning. 2019. doi:10.1038/nature14539",
        "Smith J. Shallow thoughts. 2015. doi:10.1038/nature14539",
        "aspero 2012",
    ];
    for candidate in &candidates {
        for text in claims {
            let crossref = Arc::new(
                Scripted::new("crossref")
                    .matcher(vec![candidate.clone()])
                    .with(vec![deep_learning(), candidate.clone()]),
            );
            let v = engine(vec![crossref])
                .verify(&Claim::from_text(text), 5)
                .await;
            let listing = matches!(
                v.why,
                Reason::PartlyMatches { .. }
                    | Reason::Closest { .. }
                    | Reason::IdPartlyMatches { .. }
            );
            assert!(
                !listing || !v.why.differs().is_empty(),
                "{text:?} against {:?}: {:?}",
                candidate.work.title,
                v.why
            );
        }
    }
}

#[tokio::test]
async fn a_doi_cut_short_at_a_line_break_is_the_work_whose_doi_it_starts() {
    // #39: the cited DOI stops where the PDF wrapped it; the work found by
    // the citation's text has it in full.
    let full = work(
        "crossref",
        &["10.1097/hjh.0b013e3282f35c67"],
        "Reduced incidence of new-onset atrial fibrillation with angiotensin II receptor blockade",
        &["Schmieder"],
        2008,
    );
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![full.clone()])
            .with(vec![full]),
    );
    let v = engine(vec![crossref])
        .verify(
            &Claim::from_text(
                "Schmieder RE, et al. Reduced incidence of new-onset atrial fibrillation with \
                 angiotensin II receptor blockade. J Hypertens 2008. https://doi.org/10.1097/HJH.0b013e",
            ),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Verified, "{:?}: {}", v.why, v.reason);
    let best = &v.report.candidates[v.best.unwrap()];
    assert!(
        best.ids
            .contains(&"doi:10.1097/hjh.0b013e3282f35c67".parse().unwrap())
    );
}

#[tokio::test]
async fn the_claim_as_read_and_cited_values_come_back() {
    // #46: a cited-vs-found table needs what was read from free text.
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![deep_learning()]));
    let v = engine(vec![crossref])
        .verify(
            &Claim::from_text(
                "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature 2019;521:436-44.",
            ),
            5,
        )
        .await;
    assert_eq!(v.read.year, Some(2019));
    assert_eq!(
        (v.read.volume.as_deref(), v.read.page.as_deref()),
        (Some("521"), Some("436"))
    );
    let year = v.why.differs().iter().find(|d| d.field == CitedField::Year);
    assert_eq!(
        year.map(|d| (d.cited.as_deref(), d.actual.as_deref())),
        Some((Some("2019"), Some("2015"))),
        "{:?}",
        v.why
    );
}

#[tokio::test]
async fn a_doi_cut_short_reads_as_written() {
    let full = work(
        "crossref",
        &["10.1097/hjh.0b013e3282f35c67"],
        "Reduced incidence of new-onset atrial fibrillation with angiotensin II receptor blockade",
        &["Schmieder"],
        2008,
    );
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![full.clone()])
            .with(vec![full]),
    );
    let v = engine(vec![crossref])
        .verify(
            &Claim::from_text(
                "Schmieder RE, et al. Reduced incidence of new-onset atrial fibrillation with \
                 angiotensin II receptor blockade. J Hypertens 2008. https://doi.org/10.1097/HJH.0b013e",
            ),
            5,
        )
        .await;
    // Found under the whole DOI, read as written.
    assert_eq!(v.verdict, Verdict::Verified);
    assert_eq!(v.read.ids[0].to_string(), "doi:10.1097/hjh.0b013e");
}
