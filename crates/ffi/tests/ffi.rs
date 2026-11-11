//! The handle as an app sees it, without the network: typed answers,
//! offline from the cache, and awaitable from any executor.

use aspero_ffi::records::ReadError;
use aspero_ffi::records::{
    Author, BibFormat, BibProblem, CitationManager, CiteFormat, Claim, ErrorKind, ExportFormat,
    KeyStatus, LookupStatus, Reason, SearchRequest, StyleError, StyleSource, Verdict, Work,
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
        mailto: None,
        openalex_key: None,
        cache_dir,
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

#[test]
fn failed_searches_say_why() {
    let request = SearchRequest {
        text: "deep learning".into(),
        year_from: None,
        year_to: None,
        work_type: None,
        open_access_only: false,
        limit: 5,
        source: None,
        cursor: None,
    };
    let found = block_on(aspero(None).search(request, Patience::Offline, None)).unwrap();
    assert!(found.hits.is_empty());
    assert_eq!(found.failure.unwrap().kind, ErrorKind::NotCached);
}

#[test]
fn caches_report_and_clear() {
    let dir = std::env::temp_dir().join(format!("aspero-ffi-cache-{}", std::process::id()));
    let handle = aspero(Some(dir.display().to_string()));
    assert_eq!(handle.cache_stats().unwrap().unwrap().records, 0);
    handle.clear_cache().unwrap();
    assert!(aspero(None).cache_stats().unwrap().is_none());
    drop(handle);
    std::fs::remove_dir_all(dir).unwrap();
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
