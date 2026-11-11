//! `resolve` and `verify` against scripted providers that replay the
//! situations seen live (DESIGN §9): lookalike re-deposits, citation
//! matchers that lack DataCite DOIs, invented and misattached DOIs.

mod common;

use std::sync::Arc;

use aspero_core::{
    Agreement, Capabilities, Claim, Error, Page, Provider, ProviderId, ResiliencePolicy, Response,
    Result, SearchMode, SearchQuery, WorkId,
};
use aspero_engine::{CitedField, Engine, Reason, Verdict, Via};
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
