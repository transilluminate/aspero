//! The handle as an app sees it, without the network: typed answers,
//! offline from the cache, and awaitable from any executor.

use std::collections::HashMap;

use aspero_ffi::records::ReadError;
use aspero_ffi::records::{
    AnalysisKind, Author, BibFormat, BibProblem, CitationManager, CiteFormat, Claim, Coverage,
    ErrorKind, ExportFormat, KeyStatus, LookupStatus, Reason, SearchBoolean, SearchField,
    SearchFilter, SearchFilterValue, SearchNode, SearchRelation, SearchRequest, SearchSort,
    SeedProblem, StyleError, StyleSource, Verdict, Work,
};
use aspero_ffi::{Aspero, AsperoError, CallToken, Config, Patience};

fn text(text: &str) -> Claim {
    Claim {
        text: Some(text.into()),
        ..Claim::default()
    }
}

fn aspero(cache_dir: Option<String>) -> std::sync::Arc<Aspero> {
    Aspero::new(Config {
        cache_dir,
        ..Config::default()
    })
    .unwrap()
}

/// Polls `f` on a separate runtime, as an app's executor would: the
/// handle's calls run on its own.
fn block_on<T>(f: impl Future<Output = T>) -> T {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(f)
}

#[test]
fn scans_offline() {
    let found = aspero(None).scan("see PMID 40536112".into());
    assert_eq!(found[0].id, "pmid:40536112");
    assert_eq!(found[0].kind, "pmid");
}

#[test]
fn mentions_carry_utf16_ranges() {
    // "é" is one UTF-16 unit, "👩‍⚕️" five; NSString counts these.
    let text = "é 👩‍⚕️ doi:10.1234/x\r\nPMID 40536112";
    let found = aspero(None).scan(text.into());
    let utf16: Vec<u16> = text.encode_utf16().collect();
    for m in &found {
        let range = m.utf16.start as usize..m.utf16.end as usize;
        assert_eq!(String::from_utf16(&utf16[range]).unwrap(), m.text);
    }
    assert_eq!(found.len(), 2);
}

#[test]
fn pasted_lists_split_into_references() {
    let list = "1. Smith J. One.\n   2020.\n2. Jones K. Two. 2021.";
    let refs = aspero(None).split_references(list.into());
    assert_eq!(refs[0].text, "Smith J. One. 2020.");
    let utf16: Vec<u16> = list.encode_utf16().collect();
    let second = refs[1].utf16.start as usize..refs[1].utf16.end as usize;
    assert_eq!(String::from_utf16(&utf16[second]).unwrap(), refs[1].text);
}

#[test]
fn dois_broken_at_a_hyphen_carry_an_alternative() {
    let text = "1. Smith J. doi:10.1038/nat-\n   ure14539\n2. Jones K. Two. 2021.";
    let handle = aspero(None);
    let refs = handle.split_references(text.into());
    assert_eq!(
        refs[0].alternative_ids[0].alternative,
        "doi:10.1038/nature14539"
    );
    let found = handle.scan(text.into());
    assert_eq!(
        found[0].alternative_id.as_deref(),
        Some("doi:10.1038/nature14539")
    );
}

#[test]
fn laid_out_lines_split_by_indent() {
    let line = |text: &str, indent| aspero_ffi::records::Line {
        text: text.into(),
        indent,
    };
    let refs = aspero(None).split_lines(vec![
        line("World Health Organization. Global report.", 72.0),
        line("Geneva: WHO; 2020.", 90.0),
        line("Smith J. A study. Nature. 2021.", 72.0),
    ]);
    assert_eq!(refs.len(), 2);
    assert_eq!((refs[0].lines.start, refs[0].lines.end), (0, 2));
}

#[test]
fn bibliography_files_become_claims() {
    let handle = aspero(None);
    let ris = "TY  - JOUR\nID  - lecun\nAU  - LeCun, Yann\nTI  - Deep learning\nT2  - Nature\nPY  - 2015\nVL  - 521\nSP  - 436\nDO  - 10.1038/nature14539\nER  - \n";
    let read = handle.read_references(ris.into()).unwrap();
    assert_eq!(read.format, BibFormat::Ris);
    let entry = &read.entries[0];
    assert_eq!(entry.claim.key.as_deref(), Some("lecun"));
    assert_eq!(entry.claim.container.as_deref(), Some("Nature"));
    assert_eq!(entry.claim.ids, ["doi:10.1038/nature14539"]);
    assert!(entry.checkable);

    let bib = handle
        .read_references("@article{x, title = {T}, doi = {nope}}".into())
        .unwrap();
    assert_eq!(bib.format, BibFormat::Bibtex);
    let note = &bib.entries[0].notes[0];
    assert_eq!((note.field.as_str(), note.value.as_str()), ("doi", "nope"));
    assert_eq!(note.problem, BibProblem::NotADoi);

    let err = handle.read_references("1. Smith J. A study.".into());
    assert_eq!(err.unwrap_err(), ReadError::Unrecognised);
    let err = handle.read_references("TY  - JOUR\nTI  - One\nTY  - JOUR\n".into());
    assert!(matches!(
        err,
        Err(ReadError::Invalid {
            format: BibFormat::Ris,
            line: 3,
            ..
        })
    ));
}

#[test]
fn works_export_as_bibtex_and_ris() {
    let handle = aspero(None);
    let read = handle
        .read_references(
            r#"[{"id": "a", "type": "article-journal", "title": "Deep learning",
                 "author": [{"family": "LeCun", "given": "Yann"}],
                 "issued": {"date-parts": [[2015]]}, "DOI": "10.1038/nature14539"}]"#
                .into(),
        )
        .unwrap();
    assert_eq!(read.format, BibFormat::CslJson);
    let work = Work {
        source: "test".into(),
        ids: read.entries[0].claim.ids.clone(),
        canonical: None,
        title: read.entries[0].claim.title.clone(),
        authors: vec![Author {
            name: "Yann LeCun".into(),
            orcid: None,
            family: Some("LeCun".into()),
            given: Some("Yann".into()),
            short: "LeCun Y".into(),
        }],
        issued: None,
        year: Some(2015),
        work_type: Some("article-journal".into()),
        container: Some("Nature".into()),
        volume: None,
        issue: None,
        page: None,
        publisher: None,
        url: None,
        abstract_text: None,
        notices: Vec::new(),
        notice_for: Vec::new(),
        open_access: None,
        cited_by_count: None,
        fetched_at: std::time::SystemTime::UNIX_EPOCH,
        json: "{}".into(),
    };
    let bibtex = handle.export(vec![work.clone(), work.clone()], ExportFormat::Bibtex);
    assert!(bibtex.starts_with("@article{lecun2015deep,\n  author = {LeCun, Yann},"));
    assert!(bibtex.contains("@article{lecun2015deepa,"), "{bibtex}");
    assert!(
        bibtex.contains("year = {2015}"),
        "a year without a date: {bibtex}"
    );
    let ris = handle.export(vec![work], ExportFormat::Ris);
    assert!(ris.contains("DO  - 10.1038/nature14539\n"), "{ris}");
    let back = handle.read_references(ris).unwrap();
    assert_eq!(
        back.entries[0].claim.title.as_deref(),
        Some("Deep learning")
    );
}

fn work(title: &str, author: &str, year: i16) -> Work {
    Work {
        source: "test".into(),
        ids: Vec::new(),
        canonical: None,
        title: Some(title.into()),
        authors: vec![Author {
            name: author.into(),
            orcid: None,
            family: None,
            given: None,
            short: author.into(),
        }],
        issued: None,
        year: Some(year),
        work_type: Some("article-journal".into()),
        container: Some("Nature".into()),
        volume: Some("521".into()),
        issue: None,
        page: Some("436-444".into()),
        publisher: None,
        url: None,
        abstract_text: None,
        notices: Vec::new(),
        notice_for: Vec::new(),
        open_access: None,
        cited_by_count: None,
        fetched_at: std::time::SystemTime::UNIX_EPOCH,
        json: "{}".into(),
    }
}

#[test]
fn works_cite_in_a_style() {
    let handle = aspero(None);
    let styles = handle.citation_styles();
    assert_eq!(styles.len(), 8);
    assert!(styles.iter().any(|s| s.id == "vancouver"));

    let works = vec![
        work("Zebra stripes", "Ann Zed", 2020),
        work("Deep learning", "Yann LeCun", 2015),
    ];
    let vancouver = StyleSource::Bundled {
        id: "vancouver".into(),
    };
    let cited = handle
        .cite(works.clone(), vancouver, CiteFormat::Text, None)
        .unwrap();
    assert_eq!(cited[0].index, 0, "as cited");
    assert_eq!(cited[0].label.as_deref(), Some("1."));
    assert_eq!(
        cited[1].text,
        "LeCun Y. Deep learning. Nature. 2015;521:436–44."
    );

    let apa = StyleSource::Bundled { id: "apa".into() };
    let cited = handle
        .cite(works, apa, CiteFormat::Html, Some("en-GB".into()))
        .unwrap();
    assert_eq!(cited[0].index, 1, "alphabetical");
    assert_eq!(cited[0].in_text, "(LeCun, 2015)");
    assert!(
        cited[0].text.contains("font-style: italic"),
        "{}",
        cited[0].text
    );

    let unknown = StyleSource::Bundled { id: "nope".into() };
    assert_eq!(
        handle.cite(Vec::new(), unknown, CiteFormat::Text, None),
        Err(StyleError::UnknownStyle { id: "nope".into() })
    );
    let broken = StyleSource::Csl { xml: "<p/>".into() };
    assert!(matches!(
        handle.cite(Vec::new(), broken, CiteFormat::Text, None),
        Err(StyleError::Invalid { .. })
    ));
    assert!(handle.style_credits().contains("Attribution-ShareAlike"));
}

#[test]
fn dependencies_and_notices_come_built_in() {
    let handle = aspero(None);
    let deps = handle.dependencies();
    assert!(deps.len() > 100, "{} dependencies", deps.len());
    let key = |name: &str| name.to_lowercase();
    assert!(deps.windows(2).all(|w| key(&w[0].name) <= key(&w[1].name)));
    let find = |name: &str| deps.iter().find(|d| d.name == name);
    let ours = find("aspero-ffi").expect("the library itself");
    assert_eq!(ours.version, env!("CARGO_PKG_VERSION"));
    assert_eq!(ours.license, "MIT OR Apache-2.0");
    assert!(find("uniffi").is_some_and(|d| d.license == "MPL-2.0"));
    for data in ["CSL styles (8)", "CSL locales (2)"] {
        let d = find(data).unwrap_or_else(|| panic!("{data}"));
        assert_eq!(d.license, "CC-BY-SA-3.0");
        assert!(
            d.repository
                .as_deref()
                .unwrap()
                .contains("citation-style-language")
        );
    }
    // Build and dev dependencies aren't shipped.
    assert!(find("cc").is_none() && find("proptest").is_none());

    let notices = handle.third_party_notices();
    assert!(notices.starts_with("Third-party software and data in aspero"));
    assert!(notices.contains("Permission is hereby granted, free of charge"));
    assert!(notices.contains("Apache License\n                           Version 2.0"));
    assert!(notices.contains("Used by:\n  "));
    assert!(notices.contains("Citation Style Language styles and locales (CC-BY-SA-3.0)"));
    assert!(notices.contains("Attribution-ShareAlike 3.0"));
}

#[test]
fn word_documents_give_their_citations() {
    use std::io::Write;
    let json = r#"{"citationItems":[{"id":1,"uris":["http://zotero.org/users/1/items/ABCD1234"],"itemData":{"type":"article-journal","title":"Deep learning","DOI":"10.1038/nature14539"}}]}"#;
    let instr = format!("ADDIN ZOTERO_ITEM CSL_CITATION {json}")
        .replace('&', "&amp;")
        .replace('"', "&quot;");
    let body = format!(
        r#"<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body><w:p><w:r><w:t>See </w:t></w:r><w:fldSimple w:instr="{instr}"><w:r><w:t>[1]</w:t></w:r></w:fldSimple></w:p></w:body></w:document>"#
    );
    let mut data = Vec::new();
    {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(&mut data));
        let stored = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("word/document.xml", stored).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
        zip.finish().unwrap();
    }
    let handle = aspero(None);
    let read = handle.read_docx(data).unwrap();
    assert_eq!(read.manager, Some(CitationManager::Zotero));
    assert_eq!(read.citations, 1);
    assert_eq!(read.entries[0].claim.key.as_deref(), Some("ABCD1234"));
    assert_eq!(read.entries[0].claim.ids, ["doi:10.1038/nature14539"]);
    assert_eq!(read.text, "See [1]\n");
    assert_eq!(
        handle.read_docx(b"plain".to_vec()).unwrap_err(),
        ReadError::Unrecognised
    );
}

#[test]
fn offline_lookups_answer_without_the_network() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-{}", std::process::id()));
    let handle = aspero(Some(dir.display().to_string()));
    let found = block_on(handle.lookup(
        vec!["10.1234/never-seen".into(), "not an id".into()],
        false,
        Patience::Offline,
        None,
    ))
    .unwrap();
    let never_seen = &found.results[0];
    assert_eq!(never_seen.status, LookupStatus::NotCached);
    assert_eq!(never_seen.id.as_deref(), Some("doi:10.1234/never-seen"));
    assert_eq!(never_seen.attempts[0].kind, Some(ErrorKind::NotCached));
    assert_eq!(found.results[1].status, LookupStatus::Invalid);
    assert!(found.results[1].error.is_some());
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn analyses_say_why_each_seed_is_unresolved() {
    let handle = aspero(None);
    let kinds = vec![
        AnalysisKind::OddOneOut,
        AnalysisKind::ProbablyMissing { limit: 5 },
        AnalysisKind::Related,
    ];
    let seeds = vec!["10.1234/never-seen".into(), "not an id".into()];
    let analysis =
        block_on(handle.analyse(seeds, kinds, Some(0.01), false, Patience::Offline, None)).unwrap();
    let problems: Vec<_> = analysis.seeds.iter().map(|s| s.problem).collect();
    assert_eq!(
        problems,
        [
            Some(SeedProblem::ProviderFailed),
            Some(SeedProblem::InvalidId)
        ]
    );
    assert!(analysis.not_yet.is_empty());
    // Nothing to read about: no seed resolved.
    let related = analysis.related.unwrap();
    assert!(related.items.is_empty() && related.incomplete.is_none());
    assert!(analysis.odd_one_out.unwrap().items.is_empty());
    assert!(!analysis.complete);
    assert!(analysis.failure.is_none());

    let page =
        block_on(handle.references("not an id".into(), None, 25, Patience::Offline, None)).unwrap();
    assert_eq!(page.failure.unwrap().kind, ErrorKind::InvalidId);
    assert_eq!(page.coverage, Coverage::Unknown);
}

#[test]
fn unreadable_claims_answer_at_once() {
    let checked = block_on(aspero(None).verify(
        vec![Claim {
            key: Some("7".into()),
            ..Claim::default()
        }],
        false,
        Patience::Offline,
        None,
    ))
    .unwrap();
    let v = &checked.results[0];
    assert_eq!(v.verdict, Verdict::Invalid);
    assert!(matches!(v.why, Reason::InvalidClaim { .. }));
    assert_eq!(v.key.as_deref(), Some("7"));
}

#[test]
fn claims_can_be_fields() {
    let claim = Claim {
        title: Some("Deep learning".into()),
        year: Some(2015),
        ids: vec!["10.1234/never-seen".into()],
        ..Claim::default()
    };
    let checked =
        block_on(aspero(None).verify(vec![claim], false, Patience::Offline, None)).unwrap();
    let v = &checked.results[0];
    assert_eq!(v.input, "Deep learning. 2015. 10.1234/never-seen");
    assert_eq!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
}

#[test]
fn failures_say_what_kind() {
    let checked = block_on(aspero(None).verify(
        vec![text(""), text("doi:10.1234/never-seen")],
        false,
        Patience::Offline,
        None,
    ))
    .unwrap();
    let v = &checked.results[1];
    assert_eq!(v.index, 1);
    assert_eq!(v.verdict, Verdict::Unverifiable, "{}", v.reason);
    assert!(matches!(v.why, Reason::ProvidersFailed));
    assert!(!v.failures.is_empty());
    assert!(v.failures.iter().all(|f| f.kind == ErrorKind::NotCached));
}

#[test]
fn a_cancelled_token_ends_calls_at_once() {
    let token = CallToken::new();
    token.cancel();
    let result = block_on(aspero(None).verify(
        vec![text("Some citation, 2020")],
        false,
        Patience::Batch,
        Some(token),
    ));
    assert!(matches!(result, Err(AsperoError::Cancelled)));
}

/// A request as the bindings' defaults make it.
fn search_request(text: &str) -> SearchRequest {
    SearchRequest {
        text: text.into(),
        year_from: None,
        year_to: None,
        work_type: None,
        open_access_only: false,
        limit: 10,
        source: None,
        cursor: None,
        author: None,
        title: None,
        container: None,
        sort: None,
        extra: HashMap::new(),
        query: None,
        since: None,
    }
}

#[test]
fn failed_searches_say_why() {
    let request = SearchRequest {
        limit: 5,
        ..search_request("deep learning")
    };
    let found = block_on(aspero(None).search(request, Patience::Offline, None)).unwrap();
    assert!(found.hits.is_empty());
    assert_eq!(found.failure.unwrap().kind, ErrorKind::NotCached);
}

#[test]
fn the_first_searching_source_is_the_one_search_picks() {
    let aspero = aspero(None);
    let sources = aspero.search_capabilities();
    let names: Vec<&str> = sources.iter().map(|s| s.source.as_str()).collect();
    assert_eq!(
        names,
        [
            "openalex",
            "crossref",
            "europepmc",
            "datacite",
            "openlibrary"
        ]
    );
    let found = block_on(aspero.search(search_request("x"), Patience::Offline, None)).unwrap();
    assert_eq!(found.failure.unwrap().provider.as_deref(), Some(names[0]));
    assert!(sources.iter().all(|s| s.sorts[0] == SearchSort::Relevance));
    let openlibrary = &sources[4];
    assert_eq!(
        openlibrary.fields,
        [
            SearchField::Author,
            SearchField::Title,
            SearchField::Keywords,
            SearchField::Identifier
        ]
    );
    assert_eq!(
        openlibrary.filters,
        [SearchFilter::Years, SearchFilter::Language]
    );
    assert_eq!(
        openlibrary.sorts,
        [SearchSort::Relevance, SearchSort::Newest]
    );
    // Every source takes keywords in a tree.
    assert!(
        sources
            .iter()
            .all(|s| s.fields.contains(&SearchField::Keywords))
    );
    let reach: Vec<_> = sources
        .iter()
        .map(|s| (s.any, s.none, s.nesting, s.relations.len()))
        .collect();
    use SearchBoolean::{Anywhere, No, WithinField};
    assert_eq!(
        reach,
        [
            (WithinField, Anywhere, true, 2),
            (No, No, false, 0),
            (Anywhere, Anywhere, true, 0),
            (Anywhere, Anywhere, true, 0),
            (Anywhere, Anywhere, true, 0),
        ]
    );
    assert_eq!(
        sources[0].relations,
        [SearchRelation::Cites, SearchRelation::CitedBy]
    );
}

fn term(field: SearchField, value: &str) -> SearchNode {
    SearchNode::Term {
        field,
        value: value.into(),
        phrase: false,
    }
}

/// all of [keywords, any of [title, title], none of [retracted]].
fn nested_tree() -> SearchNode {
    SearchNode::All {
        nodes: vec![
            term(SearchField::Keywords, "protein folding"),
            SearchNode::Any {
                nodes: vec![
                    term(SearchField::Title, "alphafold"),
                    SearchNode::Term {
                        field: SearchField::Title,
                        value: "structure prediction".into(),
                        phrase: true,
                    },
                ],
            },
            SearchNode::None {
                nodes: vec![SearchNode::Filter {
                    filter: SearchFilterValue::NotRetracted,
                }],
            },
        ],
    }
}

#[test]
fn search_supports_says_why_and_search_fails_the_same_way() {
    let aspero = aspero(None);
    let at = |source: &str| Some(source.to_owned());
    // Retraction in a none flips to retracted works only, which OpenAlex
    // (the default) and Europe PMC can say; Crossref can't say any.
    assert_eq!(aspero.search_supports(None, nested_tree(), None), None);
    assert_eq!(
        aspero.search_supports(at("europepmc"), nested_tree(), None),
        None
    );
    let crossref = aspero.search_supports(at("crossref"), nested_tree(), None);
    assert_eq!(
        crossref.as_deref(),
        Some("Crossref can't combine terms with any")
    );
    let across = SearchNode::Any {
        nodes: vec![
            term(SearchField::Title, "deep learning"),
            term(SearchField::Author, "LeCun"),
        ],
    };
    let reason = "OpenAlex can't combine title and author with any";
    assert_eq!(
        aspero
            .search_supports(at("openalex"), across.clone(), None)
            .as_deref(),
        Some(reason)
    );
    // Without a source, any that can: Europe PMC can.
    assert_eq!(aspero.search_supports(None, across.clone(), None), None);
    let request = SearchRequest {
        query: Some(across),
        source: at("openalex"),
        ..search_request("")
    };
    let failure = block_on(aspero.search(request, Patience::Offline, None))
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.kind, ErrorKind::Unsupported);
    assert!(failure.message.ends_with(reason), "{}", failure.message);
    // A tree a source can say reaches the (empty) cache.
    let request = SearchRequest {
        query: Some(nested_tree()),
        source: at("datacite"),
        ..search_request("")
    };
    let found = block_on(aspero.search(request, Patience::Offline, None)).unwrap();
    assert_eq!(
        found.failure.unwrap().kind,
        ErrorKind::Unsupported,
        "no retractions at DataCite"
    );
    let request = SearchRequest {
        query: Some(nested_tree()),
        ..search_request("")
    };
    let found = block_on(aspero.search(request, Patience::Offline, None)).unwrap();
    assert_eq!(found.failure.unwrap().kind, ErrorKind::NotCached);
}

#[test]
fn identifiers_search_where_their_kind_is_taken() {
    let aspero = aspero(None);
    let at = |source: &str| Some(source.to_owned());
    assert!(
        aspero
            .search_capabilities()
            .iter()
            .all(|s| s.fields.contains(&SearchField::Identifier))
    );
    let id = |value: &str| SearchNode::Term {
        field: SearchField::Identifier,
        value: value.into(),
        phrase: false,
    };
    let unretracted = |node| SearchNode::All {
        nodes: vec![
            node,
            SearchNode::Filter {
                filter: SearchFilterValue::NotRetracted,
            },
        ],
    };
    let search = |source: Option<String>, query: SearchNode| {
        let request = SearchRequest {
            query: Some(query),
            source,
            ..search_request("")
        };
        block_on(aspero.search(request, Patience::Offline, None))
            .unwrap()
            .failure
            .unwrap()
    };
    // OpenAlex, the default, takes a DOI beside a filter: the (empty)
    // cache answers.
    let doi = unretracted(id("10.1038/nature14539"));
    assert_eq!(aspero.search_supports(None, doi.clone(), None), None);
    assert_eq!(search(None, doi).kind, ErrorKind::NotCached);
    // A kind a source can't take: refused, and the search fails the same.
    for (source, value, reason) in [
        (
            "openalex",
            "978-0-262-03561-3",
            "OpenAlex has no ISBN filter",
        ),
        (
            "crossref",
            "pmid:26017442",
            "Crossref finds works by DOI only",
        ),
        (
            "datacite",
            "PMC7254481",
            "DataCite finds works by DOI or arXiv id only",
        ),
        (
            "europepmc",
            "W2919115771",
            "Europe PMC can't find works by OpenAlex work id",
        ),
        (
            "openlibrary",
            "10.1038/nature14539",
            "Open Library finds works by ISBN only",
        ),
    ] {
        assert_eq!(
            aspero
                .search_supports(at(source), id(value), None)
                .as_deref(),
            Some(reason)
        );
        let failure = search(at(source), id(value));
        assert_eq!(failure.kind, ErrorKind::Unsupported);
        assert!(failure.message.ends_with(reason), "{}", failure.message);
    }
    let kinds = SearchNode::Any {
        nodes: vec![id("10.1038/nature14539"), id("pmid:26017442")],
    };
    assert_eq!(
        aspero
            .search_supports(at("openalex"), kinds.clone(), None)
            .as_deref(),
        Some("OpenAlex can't combine DOI and PMID with any, only identifiers of one kind")
    );
    assert_eq!(
        aspero.search_supports(at("europepmc"), kinds.clone(), None),
        None
    );
    // Without a source, any that can: Europe PMC.
    assert_eq!(aspero.search_supports(None, kinds, None), None);
    // Not an identifier: the caller's to fix.
    let reason =
        r#"can't search by identifier "nature14539": unrecognised identifier "nature14539""#;
    assert_eq!(
        aspero
            .search_supports(None, id("nature14539"), None)
            .as_deref(),
        Some(reason)
    );
    let failure = search(None, id("nature14539"));
    assert_eq!(failure.kind, ErrorKind::BadRequest);
    assert!(failure.message.ends_with(reason), "{}", failure.message);
}

#[test]
fn since_picks_a_source_that_can_and_one_that_cant_says_why() {
    let aspero = aspero(None);
    let at = |source: &str| Some(source.to_owned());
    let day = || Some("2026-11-01".to_owned());
    let dated: Vec<String> = aspero
        .search_capabilities()
        .into_iter()
        .filter(|s| s.filters.contains(&SearchFilter::Since))
        .map(|s| s.source)
        .collect();
    assert_eq!(dated, ["crossref", "europepmc", "datacite"]);

    // Without a source, the first that can: Crossref, not OpenAlex.
    let keywords = term(SearchField::Keywords, "sepsis");
    assert_eq!(aspero.search_supports(None, keywords.clone(), day()), None);
    let request = SearchRequest {
        since: day(),
        ..search_request("sepsis")
    };
    let failure = block_on(aspero.search(request.clone(), Patience::Offline, None))
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.kind, ErrorKind::NotCached);
    assert_eq!(failure.provider.as_deref(), Some("crossref"));

    // Named, OpenAlex and Open Library refuse, saying why.
    for (source, reason) in [
        (
            "openalex",
            "OpenAlex can't search by date added (it needs a paid plan)",
        ),
        ("openlibrary", "Open Library can't search by date added"),
    ] {
        assert_eq!(
            aspero
                .search_supports(at(source), keywords.clone(), day())
                .as_deref(),
            Some(reason)
        );
        assert_eq!(
            aspero.search_supports(at(source), keywords.clone(), None),
            None
        );
        let request = SearchRequest {
            source: at(source),
            ..request.clone()
        };
        let failure = block_on(aspero.search(request, Patience::Offline, None))
            .unwrap()
            .failure
            .unwrap();
        assert_eq!(failure.kind, ErrorKind::Unsupported);
        assert!(failure.message.ends_with(reason), "{}", failure.message);
    }

    // A date that isn't one is the caller's to fix.
    let reason = "invalid date \"1 Nov 2026\": expected YYYY-MM-DD";
    let bad = || Some("1 Nov 2026".to_owned());
    assert_eq!(
        aspero.search_supports(None, keywords, bad()).as_deref(),
        Some(reason)
    );
    let request = SearchRequest {
        since: bad(),
        ..request
    };
    let failure = block_on(aspero.search(request, Patience::Offline, None))
        .unwrap()
        .failure
        .unwrap();
    assert_eq!(failure.kind, ErrorKind::BadRequest);
    assert_eq!(failure.message, reason);
}

#[test]
fn a_query_replaces_text_and_fields() {
    let aspero = aspero(None);
    let kind = |request| {
        block_on(aspero.search(request, Patience::Offline, None))
            .unwrap()
            .failure
            .unwrap()
            .kind
    };
    let with_text = SearchRequest {
        query: Some(nested_tree()),
        ..search_request("transformers")
    };
    assert_eq!(kind(with_text), ErrorKind::BadRequest);
    let with_field = SearchRequest {
        query: Some(nested_tree()),
        author: Some("LeCun".into()),
        ..search_request("")
    };
    assert_eq!(kind(with_field), ErrorKind::BadRequest);
    // Nothing searchable in the tree.
    let empty = SearchRequest {
        query: Some(SearchNode::All {
            nodes: vec![SearchNode::Any { nodes: vec![] }],
        }),
        ..search_request("")
    };
    assert_eq!(kind(empty), ErrorKind::BadRequest);
    // An author id that's neither an ORCID nor an OpenAlex id.
    let bad_id = SearchNode::Term {
        field: SearchField::AuthorId,
        value: "Yann LeCun".into(),
        phrase: false,
    };
    assert_eq!(
        aspero
            .search_supports(None, bad_id.clone(), None)
            .as_deref(),
        Some("\"Yann LeCun\" is neither an ORCID nor an OpenAlex author id (A…)")
    );
    let request = SearchRequest {
        query: Some(bad_id),
        ..search_request("")
    };
    assert_eq!(kind(request), ErrorKind::BadRequest);
}

#[test]
fn fields_alone_search_and_what_a_source_lacks_fails() {
    let aspero = aspero(None);
    let search = |request| {
        block_on(aspero.search(request, Patience::Offline, None))
            .unwrap()
            .failure
            .unwrap()
            .kind
    };
    // Reaches the (empty) cache: nothing refused it.
    let by_author = SearchRequest {
        author: Some("LeCun".into()),
        ..search_request("")
    };
    assert_eq!(search(by_author.clone()), ErrorKind::NotCached);
    let at = |source: &str| Some(source.to_owned());
    let in_journal = SearchRequest {
        container: Some("Nature".into()),
        source: at("openlibrary"),
        ..by_author.clone()
    };
    assert_eq!(search(in_journal), ErrorKind::Unsupported);
    let most_cited = SearchRequest {
        sort: Some(SearchSort::MostCited),
        source: at("openlibrary"),
        ..by_author
    };
    assert_eq!(search(most_cited), ErrorKind::Unsupported);
}

#[test]
fn caches_report_and_clear() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-cache-{}", std::process::id()));
    let handle = aspero(Some(dir.display().to_string()));
    let stats = handle.cache_stats().unwrap().unwrap();
    assert_eq!(stats.records, 0);
    assert!(stats.bytes > 0, "an empty cache's pages");
    handle.clear_cache().unwrap();
    assert!(aspero(None).cache_stats().unwrap().is_none());
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_cache_limit_is_at_least_a_megabyte() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-limit-{}", std::process::id()));
    let config = |mb| Config {
        cache_dir: Some(dir.display().to_string()),
        cache_max_mb: Some(mb),
        ..Config::default()
    };
    assert!(matches!(Aspero::new(config(0)), Err(AsperoError::Setup(_))));
    let handle = Aspero::new(config(200)).unwrap();
    assert!(handle.cache_stats().unwrap().unwrap().bytes < 200_000_000);
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn settings_default_as_before() {
    let config = Config::default();
    assert_eq!(config.cache_found_days, 7);
    assert_eq!(config.cache_not_found_days, 1);
    assert_eq!(config.cache_max_mb, None);
    assert_eq!(config.openalex_daily_cap, None);
    assert!(config.disabled_sources.is_empty());
}

fn all_sources_but(kept: &[&str]) -> Vec<String> {
    aspero_engine::SOURCES
        .iter()
        .map(|(name, _)| name.to_string())
        .filter(|name| !kept.contains(&name.as_str()))
        .collect()
}

#[test]
fn sources_turned_off_are_left_out_and_say_so() {
    let handle = Aspero::new(Config {
        disabled_sources: vec!["europepmc".into(), "openlibrary".into()],
        ..Config::default()
    })
    .unwrap();
    let names: Vec<String> = handle
        .search_capabilities()
        .into_iter()
        .map(|s| s.source)
        .collect();
    assert_eq!(names, ["openalex", "crossref", "datacite"]);

    let request = SearchRequest {
        source: Some("europepmc".into()),
        ..search_request("sepsis")
    };
    let found = block_on(handle.search(request, Patience::Batch, None)).unwrap();
    let failure = found.failure.unwrap();
    assert_eq!(failure.kind, ErrorKind::Unsupported);
    assert_eq!(failure.message, "Europe PMC is turned off");
    assert_eq!(failure.provider.as_deref(), Some("europepmc"));
    assert_eq!(
        handle
            .search_supports(
                Some("europepmc".into()),
                term(SearchField::Keywords, "x"),
                None
            )
            .as_deref(),
        Some("Europe PMC is turned off")
    );
}

#[test]
fn unknown_sources_and_none_left_are_setup_errors() {
    for disabled in [vec!["pubmed".to_string()], all_sources_but(&[])] {
        let made = Aspero::new(Config {
            disabled_sources: disabled.clone(),
            ..Config::default()
        });
        assert!(matches!(made, Err(AsperoError::Setup(_))), "{disabled:?}");
    }
}

#[test]
fn checks_work_with_one_source_left() {
    use aspero_core::{FieldSet, PartialDate, Person, ProviderId, Raw, SourceRecord, WorkId};
    use aspero_engine::{Cache, SqliteCache};

    let dir = std::env::temp_dir().join(format!("aspero-ffi-one-{}", std::process::id()));
    // Crossref's record of the work, cached as an earlier check left it.
    let doi: WorkId = "10.1038/nature14539".parse().unwrap();
    let mut work = aspero_core::Work::default();
    work.title = Some("Deep learning".into());
    work.authors = vec![
        Person::new("Yann", "LeCun"),
        Person::new("Yoshua", "Bengio"),
        Person::new("Geoffrey", "Hinton"),
    ];
    work.issued = Some(PartialDate::from_year(2015));
    work.container_title = Some("Nature".into());
    work.volume = Some("521".into());
    work.page = Some("436-444".into());
    let now = jiff::Timestamp::now();
    let crossref = ProviderId::from_static("crossref");
    let record = SourceRecord::new(
        crossref.clone(),
        vec![doi.clone()],
        work,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        },
        now,
    );
    let cache = SqliteCache::open(&dir.join("cache.sqlite")).unwrap();
    cache
        .put_lookups(&crossref, &[(&doi, Some(&record))], now)
        .unwrap();
    drop(cache);

    let handle = Aspero::new(Config {
        cache_dir: Some(dir.display().to_string()),
        disabled_sources: all_sources_but(&["crossref"]),
        ..Config::default()
    })
    .unwrap();
    let names: Vec<String> = handle
        .search_capabilities()
        .into_iter()
        .map(|s| s.source)
        .collect();
    assert_eq!(names, ["crossref"]);
    let claim = text(
        "LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. \
         doi:10.1038/nature14539",
    );
    let checked = block_on(handle.verify(vec![claim], false, Patience::Offline, None)).unwrap();
    let v = &checked.results[0];
    assert_eq!(v.verdict, Verdict::Verified, "{v:?}");
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_daily_cap_must_be_an_amount() {
    for cap in [-0.5, f64::NAN, f64::INFINITY] {
        let made = Aspero::new(Config {
            openalex_daily_cap: Some(cap),
            ..Config::default()
        });
        assert!(matches!(made, Err(AsperoError::Setup(_))), "{cap}");
    }
    for cap in [0.0, 0.5] {
        let config = Config {
            openalex_daily_cap: Some(cap),
            ..Config::default()
        };
        assert!(Aspero::new(config).is_ok());
    }
}

#[test]
fn without_a_key_refreshing_budgets_asks_nothing() {
    let handle = aspero(None);
    let budgets = block_on(handle.refresh_budgets(Patience::Batch, None)).unwrap();
    assert!(budgets.is_empty(), "{budgets:?}");
    assert!(handle.budgets().is_empty());
}

#[test]
fn blank_keys_are_rejected_without_asking() {
    let checked = block_on(aspero(None).check_openalex_key("  ".into(), None)).unwrap();
    assert_eq!(checked.status, KeyStatus::Rejected);
}

/// Asks OpenAlex: `cargo test -p aspero-ffi -- --ignored`.
#[test]
#[ignore = "network"]
fn live_key_checks() {
    let handle = aspero(None);
    let checked = block_on(handle.check_openalex_key("not-a-real-key".into(), None)).unwrap();
    eprintln!("{checked:?}");
    assert_eq!(checked.status, KeyStatus::Rejected);
    if let Ok(key) = std::env::var("OPENALEX_API_KEY") {
        let checked = block_on(handle.check_openalex_key(key, None)).unwrap();
        eprintln!("{checked:?}");
        assert_eq!(checked.status, KeyStatus::Valid);
        assert!(checked.budget.is_some_and(|b| b.remaining.is_some()));
    }
}

/// Times the path where a DOI's reading without its line-break hyphen is
/// needed: `cargo test -p aspero-ffi alternative_timing -- --ignored --nocapture`.
#[test]
#[ignore = "network"]
fn alternative_timing() {
    let lists = [
        (
            "alt",
            "1. LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. doi:10.1038/nature-\n14539",
        ),
        (
            "whole",
            "1. LeCun Y, Bengio Y, Hinton G. Deep learning. Nature. 2015;521:436-444. doi:10.1038/nature14539",
        ),
    ];
    for run in 1..=3 {
        for (name, list) in lists {
            let handle = aspero(None);
            let claims: Vec<Claim> = handle
                .split_references(list.into())
                .into_iter()
                .map(|r| Claim {
                    text: Some(r.text),
                    alternative_ids: r.alternative_ids,
                    ..Claim::default()
                })
                .collect();
            let start = std::time::Instant::now();
            let v = block_on(handle.verify(claims, false, Patience::Interactive, None)).unwrap();
            eprintln!(
                "{run} {name}: {:.1}s {:?} {}",
                start.elapsed().as_secs_f64(),
                v.results[0].verdict,
                v.results[0].reason
            );
        }
    }
}
