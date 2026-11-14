//! Choosing sources: those turned off are never built or asked, searches
//! naming one say so, and checks work with every source off but one.
#![cfg(all(
    feature = "openalex",
    feature = "crossref",
    feature = "datacite",
    feature = "doi",
    feature = "europepmc",
    feature = "openlibrary"
))]

mod common;

use std::sync::Arc;

use aspero_core::{
    Claim, ErrorCategory, ErrorKind, FieldSet, QueryNode, SearchQuery, TermField, WorkId,
};
use aspero_engine::{EngineError, SourcesError, StandardConfig, Verdict, source_name, standard};
use common::*;

fn names(engine: &aspero_engine::Engine) -> Vec<String> {
    engine.providers().map(|p| p.id().to_string()).collect()
}

#[test]
fn sources_turned_off_are_never_built() {
    let mut config = StandardConfig::all(None);
    config.disable(&["europepmc", " OpenLibrary ", ""]).unwrap();
    let engine = standard(config).unwrap();
    assert_eq!(names(&engine), ["openalex", "crossref", "datacite", "doi"]);
    let ids: Vec<&str> = engine.disabled().iter().map(|p| p.as_str()).collect();
    assert_eq!(ids, ["europepmc", "openlibrary"]);
    let searching: Vec<String> = engine
        .search_providers()
        .map(|p| p.id().to_string())
        .collect();
    assert_eq!(searching, ["openalex", "crossref", "datacite"]);
}

#[tokio::test]
async fn a_search_naming_a_source_turned_off_says_so() {
    let mut config = StandardConfig::all(None);
    config.disable(&["europepmc"]).unwrap();
    let engine = standard(config).unwrap();

    let error = engine
        .search(Some("europepmc"), &SearchQuery::new("sepsis"))
        .await
        .unwrap_err();
    assert!(matches!(error, EngineError::Disabled(_)), "{error:?}");
    assert_eq!(error.to_string(), "Europe PMC is turned off");
    assert_eq!(error.kind(), ErrorKind::Unsupported);
    assert_eq!(error.category(), ErrorCategory::Unsupported);

    // `search_supports` agrees, offline.
    let tree = QueryNode::term(TermField::Keywords, "sepsis");
    assert_eq!(
        engine.search_supports(Some("europepmc"), &tree).as_deref(),
        Some("Europe PMC is turned off")
    );
    // A name that never was a source is still unknown.
    let unknown = engine
        .search(Some("pubmed"), &SearchQuery::new("sepsis"))
        .await
        .unwrap_err();
    assert!(matches!(unknown, EngineError::UnknownProvider(_)));
}

#[tokio::test]
async fn lookups_never_ask_a_source_turned_off() {
    // Europe PMC is the only source for PMCIDs: turned off, nothing is
    // asked at all (no attempt, so no request).
    let mut config = StandardConfig::all(None);
    config.disable(&["europepmc"]).unwrap();
    let engine = standard(config).unwrap();
    let id: WorkId = "PMC6716493".parse().unwrap();
    let report = engine.lookup(&[id], FieldSet::DEFAULT).await;
    assert!(report.results[0].attempts.is_empty());
    assert!(report.results[0].unsupported().is_some());
}

#[test]
fn unknown_names_and_no_sources_left_are_errors() {
    let mut config = StandardConfig::all(None);
    assert_eq!(
        config.disable(&["pubmed"]),
        Err(SourcesError::Unknown("pubmed".into()))
    );
    let message = SourcesError::Unknown("pubmed".into()).to_string();
    assert!(message.contains("openalex, crossref, datacite, doi, europepmc, openlibrary"));

    let all: Vec<&str> = aspero_engine::SOURCES.iter().map(|(n, _)| *n).collect();
    assert_eq!(config.disable(&all), Err(SourcesError::NoneLeft));
    // A failed call changes nothing.
    assert!(config.disabled.is_empty());
    assert_eq!(names(&standard(config.clone()).unwrap()).len(), 6);

    // All but one is fine.
    config.disable(&all[..5]).unwrap();
    assert_eq!(names(&standard(config).unwrap()), ["openlibrary"]);
}

#[test]
fn sources_have_names_to_show() {
    assert_eq!(source_name("europepmc"), "Europe PMC");
    assert_eq!(source_name("doi"), "doi.org");
    assert_eq!(source_name("other"), "other");
}

#[tokio::test]
async fn checks_work_with_only_one_source_left() {
    // Only Crossref: its matcher and keyword search find the work.
    let crossref = Arc::new(
        Scripted::new("crossref")
            .matcher(vec![deep_learning()])
            .with(vec![deep_learning()]),
    );
    let engine = engine(vec![crossref.clone()]);
    let v = engine
        .verify(
            &Claim::from_text(
                "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. \
                 doi:10.1038/nature14539",
            ),
            5,
        )
        .await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason);
    let best = &v.report.candidates[v.best.unwrap()];
    assert_eq!(best.record.source.as_str(), "crossref");
    assert!(v.report.failures().is_empty());
}
