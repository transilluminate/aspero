use super::*;

const URL: &str = "https://example.org/article/1";

fn page(html: &str) -> PageClaim {
    claim_from_page(Some(html), URL, None)
}

fn ids(p: &PageClaim) -> Vec<String> {
    p.ids.iter().map(|i| i.mention.id.to_string()).collect()
}

fn meta(name: &str, content: &str) -> String {
    format!("<meta name=\"{name}\" content=\"{content}\">")
}

#[test]
fn highwire_tags_make_a_claim() {
    let html = [
        meta(
            "citation_title",
            "Deep learning for <i>sepsis</i> prediction",
        ),
        meta("citation_author", "Smith, Jane"),
        meta("citation_author", "  John   Doe "),
        meta("citation_author", "et al."),
        meta("citation_author", ""),
        meta("citation_journal_title", "Critical Care"),
        meta("citation_volume", "24"),
        meta("citation_firstpage", "101"),
        meta("citation_lastpage", "109"),
        meta("citation_publication_date", "2020/03/01"),
        meta("citation_doi", "doi:10.1186/S13054-020-1"),
        meta("citation_pmid", "32111111"),
        meta("citation_arxiv_id", "2001.01234"),
    ]
    .concat();
    let p = page(&html);
    assert_eq!(p.source, PageSource::Highwire);
    assert_eq!(
        ids(&p),
        [
            "doi:10.1186/s13054-020-1",
            "pmid:32111111",
            "arxiv:2001.01234"
        ]
    );
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some("Deep learning for sepsis prediction")
    );
    assert_eq!(c.authors, ["Smith, Jane", "John Doe"]);
    assert_eq!(c.container.as_deref(), Some("Critical Care"));
    assert_eq!(c.volume.as_deref(), Some("24"));
    assert_eq!(c.page.as_deref(), Some("101-109"));
    assert_eq!(c.year, Some(2020));
    assert_eq!(c.ids.len(), 3);
    assert!(c.text.is_none());
    // Read from tags, not the address.
    assert!(p.ids.iter().all(|i| !i.mention.from_url));
}

#[test]
fn highwire_fallbacks() {
    let p = page(
        &[
            meta("citation_title", "Proceedings paper on graph networks"),
            meta("citation_authors", "Smith, J; Doe, J;"),
            meta("citation_conference_title", "NeurIPS"),
            meta("citation_date", "15 June 2019"),
            meta("citation_online_date", "2018"),
        ]
        .concat(),
    );
    let c = p.claim.unwrap();
    assert_eq!(c.authors, ["Smith, J", "Doe, J"]);
    assert_eq!(c.container.as_deref(), Some("NeurIPS"));
    assert_eq!(c.year, Some(2019));
    // The journal's abbreviation when it has no title.
    let c = page(
        &[
            meta("citation_title", "A b c d"),
            meta("citation_journal_abbrev", "N Engl J Med"),
        ]
        .concat(),
    )
    .claim
    .unwrap();
    assert_eq!(c.container.as_deref(), Some("N Engl J Med"));
}

#[test]
fn prism_tags() {
    let p = page(
        &[
            meta("prism.doi", "10.1038/nature14539"),
            meta("prism.publicationName", "Nature"),
            meta("prism.volume", "521"),
            meta("prism.startingPage", "436"),
            meta("prism.endingPage", "444"),
            meta("prism.publicationDate", "2015-05-27"),
        ]
        .concat(),
    );
    assert_eq!(p.source, PageSource::Prism);
    assert_eq!(ids(&p), ["doi:10.1038/nature14539"]);
    let c = p.claim.unwrap();
    assert_eq!(c.container.as_deref(), Some("Nature"));
    assert_eq!(c.page.as_deref(), Some("436-444"));
    assert_eq!(c.year, Some(2015));
    assert_eq!(c.title, None);
}

#[test]
fn dublin_core_tags_in_any_case() {
    for (prefix, identifier) in [
        ("DC", "doi:10.1136/bmjqs-2017-007444"),
        ("dc", "https://doi.org/10.1136/bmjqs-2017-007444"),
        ("DCTERMS", "info:doi/10.1136/bmjqs-2017-007444"),
    ] {
        let p = page(
            &[
                meta(
                    &format!("{prefix}.identifier"),
                    "https://repository.example.ac.uk/1/",
                ),
                meta(&format!("{prefix}.identifier"), identifier),
                meta(
                    &format!("{prefix}.title"),
                    "Nurse staffing and mortality in hospitals",
                ),
                meta(&format!("{prefix}.creator"), "Griffiths, Peter"),
                meta(&format!("{prefix}.date"), "2018-06-01"),
                meta(&format!("{prefix}.source"), "BMJ Quality &amp; Safety"),
            ]
            .concat(),
        );
        assert_eq!(p.source, PageSource::DublinCore, "{prefix}");
        assert_eq!(ids(&p), ["doi:10.1136/bmjqs-2017-007444"], "{prefix}");
        let c = p.claim.unwrap();
        assert_eq!(c.authors, ["Griffiths, Peter"]);
        assert_eq!(c.year, Some(2018));
        assert_eq!(c.container.as_deref(), Some("BMJ Quality & Safety"));
    }
    // A citation as the source is not a journal's name.
    let c = page(
        &[
            meta("dc.title", "A b c"),
            meta("dc.source", "Nature 2015 521:7553"),
        ]
        .concat(),
    )
    .claim
    .unwrap();
    assert_eq!(c.container, None);
}

#[test]
fn json_ld_articles() {
    let html = r#"<script type="application/ld+json">[
        {"@type": "WebSite", "name": "Example Journals", "url": "https://example.org"},
        {"@type": ["ScholarlyArticle"], "headline": "Ward rounds &amp; safety: a review",
         "identifier": [{"@type": "PropertyValue", "propertyID": "PMID", "value": "31452104"},
                        "https://doi.org/10.5555/x.1"],
         "author": ["Okafor, A", {"givenName": "Sofia", "familyName": "Lindqvist"}, {"name": "et al"}],
         "datePublished": "2024-03-09", "pageStart": 7,
         "isPartOf": {"@type": "WebSite", "name": "Example Journals"}}
    ]</script>"#;
    let p = page(html);
    assert_eq!(p.source, PageSource::JsonLd);
    assert_eq!(ids(&p), ["pmid:31452104", "doi:10.5555/x.1"]);
    let c = p.claim.unwrap();
    assert_eq!(c.title.as_deref(), Some("Ward rounds & safety: a review"));
    assert_eq!(c.authors, ["Okafor, A", "Sofia Lindqvist"]);
    assert_eq!(c.year, Some(2024));
    assert_eq!(c.page.as_deref(), Some("7"));
    // A website is not a journal.
    assert_eq!(c.container, None);
}

#[test]
fn json_ld_that_isnt_an_article_or_isnt_json() {
    for html in [
        r#"<script type="application/ld+json">{"@type": "WebSite", "name": "Example Journals Online"}</script>"#,
        r#"<script type="application/ld+json">{"@type": "ScholarlyArticle", "headline": </script>"#,
        r#"<script type="application/ld+json">not json at all</script>"#,
        r#"<script type="application/ld+json">{"@type": "ItemList", "itemListElement": [{"@type": "ScholarlyArticle", "headline": "Related article title", "sameAs": "https://doi.org/10.5555/related"}]}</script>"#,
    ] {
        let p = page(html);
        assert!(p.ids.is_empty(), "{html}");
        assert_eq!(p.claim, None, "{html}");
    }
}

#[test]
fn each_field_from_the_first_vocabulary_that_has_it() {
    let html = [
        meta("citation_title", "Highwire title of the work"),
        meta("prism.doi", "10.5555/prism"),
        meta("prism.volume", "9"),
        meta("dc.title", "Dublin Core title of the work"),
        meta("dc.creator", "Dc, Author"),
        meta("dc.date", "1999"),
        meta("citation_journal_title", "Highwire Journal"),
        r#"<script type="application/ld+json">{"@type": "Article", "headline": "JSON-LD title",
            "sameAs": "https://doi.org/10.5555/jsonld", "datePublished": "2001-01-01",
            "author": "Ld, Author", "pagination": "1-2"}</script>"#
            .to_owned(),
        "<title>Page title of the work | Example</title>".to_owned(),
    ]
    .concat();
    let p = claim_from_page(
        Some(&html),
        "https://doi.org/10.5555/address",
        Some("Hinted title of the work"),
    );
    assert_eq!(p.source, PageSource::Highwire);
    assert_eq!(
        ids(&p),
        [
            "doi:10.5555/prism",
            "doi:10.5555/jsonld",
            "doi:10.5555/address"
        ]
    );
    assert_eq!(
        p.ids.iter().map(|i| i.source).collect::<Vec<_>>(),
        [PageSource::Prism, PageSource::JsonLd, PageSource::Url]
    );
    assert!(p.ids[2].mention.from_url);
    let c = p.claim.unwrap();
    assert_eq!(c.title.as_deref(), Some("Highwire title of the work"));
    assert_eq!(c.volume.as_deref(), Some("9"));
    assert_eq!(c.authors, ["Dc, Author"]);
    assert_eq!(c.year, Some(1999));
    assert_eq!(c.container.as_deref(), Some("Highwire Journal"));
    assert_eq!(c.page.as_deref(), Some("1-2"));
}

#[test]
fn identifiers_are_kept_once() {
    let p = claim_from_page(
        Some(
            &[
                meta("citation_doi", "10.1038/NATURE14539"),
                meta("dc.identifier", "doi:10.1038/nature14539"),
                meta("prism.doi", "https://doi.org/10.1038/nature14539"),
            ]
            .concat(),
        ),
        "https://www.nature.com/articles/nature14539",
        None,
    );
    assert_eq!(ids(&p), ["doi:10.1038/nature14539"]);
    assert_eq!(p.claim.unwrap().ids.len(), 1);
}

/// A publisher's article page: a page there with only a title is still a
/// work's.
const PUBLISHER: &str = "https://emj.bmj.com/content/41/2/100";

#[test]
fn page_titles_only_when_no_vocabulary_has_one() {
    let at = |html: &str| claim_from_page(Some(html), PUBLISHER, None);
    let p = at(
        "<title>Chest wall thickness and decompression failure | Emergency Medicine Journal</title>",
    );
    assert_eq!(p.source, PageSource::Title);
    assert_eq!(p.kind, PageKind::Work);
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("Chest wall thickness and decompression failure")
    );
    // `og:title` before `<title>`.
    let p = at(
        "<title>Site | Home</title><meta property=\"og:title\" content=\"Open Graph title of the work\">",
    );
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("Open Graph title of the work")
    );
    // Only the first `<title>`: an icon's in the body isn't the page's.
    let p = at("<title>Gut microbiota and depression</title><svg><title>Close menu</title></svg>");
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("Gut microbiota and depression")
    );
    // Elsewhere, a page with only a title is a web page: no claim.
    let p = page("<title>Gut microbiota and depression</title>");
    assert_eq!(p.kind, PageKind::WebPage);
    assert_eq!(p.claim, None);
    let web = p.web_page.unwrap();
    assert_eq!(web.title.as_deref(), Some("Gut microbiota and depression"));
    assert_eq!(web.key, "url:https://example.org/article/1");
}

#[test]
fn the_title_given_only_when_nothing_better() {
    let hint = Some("Gut microbiota and depression in adults | Frontiers");
    let p = claim_from_page(None, URL, hint);
    assert_eq!(p.source, PageSource::TitleHint);
    // No markup: nothing tells it isn't a work's.
    assert_eq!(p.kind, PageKind::Work);
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("Gut microbiota and depression in adults")
    );
    let p = claim_from_page(
        Some("<title>The page's own title here</title>"),
        PUBLISHER,
        hint,
    );
    assert_eq!(p.source, PageSource::Title);
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("The page's own title here")
    );
}

#[test]
fn link_text_makes_no_claim() {
    for hint in [
        "Full Text",
        "PDF",
        "Download PDF",
        "View Article",
        "here",
        "doi",
        URL,
    ] {
        let p = claim_from_page(None, URL, Some(hint));
        assert_eq!(p.claim, None, "{hint}");
        assert!(p.ids.is_empty());
        assert_eq!(p.source, PageSource::Url);
    }
    // With an identifier in the address, a claim of just it.
    let url = "https://www.sciencedirect.com/science/article/pii/S2772940026000032";
    let p = claim_from_page(None, url, Some("Full Text"));
    assert_eq!(p.source, PageSource::Url);
    assert_eq!(ids(&p), ["pii:S2772940026000032"]);
    assert!(p.ids[0].mention.from_url);
    assert_eq!(p.ids[0].text, url);
    let c = p.claim.unwrap();
    assert_eq!(c.title, None);
    assert_eq!(c.ids.len(), 1);
    // And with a title too.
    let p = claim_from_page(
        None,
        url,
        Some("Ambient air pollution and stroke - ScienceDirect"),
    );
    assert_eq!(p.source, PageSource::TitleHint);
    let c = p.claim.unwrap();
    assert_eq!(c.title.as_deref(), Some("Ambient air pollution and stroke"));
    assert_eq!(c.ids.len(), 1);
}

#[test]
fn the_address_is_never_an_identifier_itself() {
    let p = claim_from_page(
        None,
        "https://jamanetwork.com/journals/jama/fullarticle/2856706",
        Some("Semaglutide and Kidney Outcomes | JAMA | JAMA Network"),
    );
    let c = p.claim.unwrap();
    assert!(c.ids.is_empty());
    assert_eq!(c.title.as_deref(), Some("Semaglutide and Kidney Outcomes"));
    // Never the journal from the address: its host is the site, a hint.
    assert_eq!(c.container, None);
    assert_eq!(c.site.as_deref(), Some("jamanetwork.com"));
    assert_eq!(c.site_prefixes(), ["10.1001"]);
}

#[test]
fn the_site_is_the_addresss_host_even_with_identifiers() {
    let url = "https://WWW.ScienceDirect.com/science/article/pii/S2772940026000032";
    let c = claim_from_page(None, url, None).claim.unwrap();
    assert_eq!(c.site.as_deref(), Some("sciencedirect.com"));
    // Identifiers say which work: the site then hints nothing.
    assert!(c.site_prefixes().is_empty());
    let html = meta("citation_title", "Air pollution and stroke in adults");
    let c = claim_from_page(Some(&html), "https://pubs.acs.org/doi/abs/x", None)
        .claim
        .unwrap();
    assert_eq!(c.site.as_deref(), Some("pubs.acs.org"));
    assert_eq!(c.site_prefixes(), ["10.1021"]);
}

#[test]
fn script_contents_are_not_read_but_json_ld_is() {
    let html = r#"<script>document.write('<meta name="citation_doi" content="10.9999/fake">')</script>
        <script type="module">const t = "<title>Fake title of the page</title>";</script>
        <script type="application/ld+json">{"@type":"ScholarlyArticle","headline":"Real title from data","sameAs":"https://doi.org/10.5555/real"}</script>"#;
    let p = page(html);
    assert_eq!(ids(&p), ["doi:10.5555/real"]);
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("Real title from data")
    );
}

#[test]
fn junk_fragments_and_huge_input() {
    for junk in [
        "",
        "not html at all",
        "<meta",
        "<meta name=\"citation_doi\" content=\"not a doi\">",
        "<meta name=\"citation_pmid\" content=\"abc\">",
        "<<<meta>>>",
        "</head></html>",
        "\u{feff}\u{0}<\u{fffd}>",
    ] {
        let p = page(junk);
        assert!(p.ids.is_empty(), "{junk:?}");
        assert_eq!(p.claim, None, "{junk:?}");
    }
    // A fragment: a tag alone.
    assert_eq!(
        ids(&page(&meta("citation_doi", "10.1000/x"))),
        ["doi:10.1000/x"]
    );
    // Past the cap, nothing more is read.
    let mut huge = " ".repeat(MAX_INPUT);
    huge.push_str(&meta("citation_doi", "10.1000/late"));
    assert!(page(&huge).ids.is_empty());
    // Values too long to be one are ignored.
    let long = "word ".repeat(MAX_VALUE);
    assert_eq!(page(&meta("citation_title", &long)).claim, None);
}

#[test]
fn entities_in_values() {
    let p = page(
        &[
            meta(
                "citation_title",
                "Smith &amp; Wesson&#39;s &quot;rule&quot; &#x2013; a test",
            ),
            meta("citation_author", "M&uuml;ller, J&ouml;rg"),
        ]
        .concat(),
    );
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some("Smith & Wesson's \"rule\" – a test")
    );
    assert_eq!(c.authors, ["Müller, Jörg"]);
}

#[test]
fn authors_are_capped() {
    let html: String = (0..MAX_AUTHORS + 20)
        .map(|i| meta("citation_author", &format!("Author{i}, A")))
        .chain([meta("citation_title", "A big collaboration paper")])
        .collect();
    let c = page(&html).claim.unwrap();
    assert_eq!(c.authors.len(), MAX_AUTHORS);
    assert_eq!(c.authors[0], "Author0, A");
}

#[test]
fn years_from_dates() {
    for (date, year) in [
        ("2026/01/15", Some(2026)),
        ("2026-01-15", Some(2026)),
        ("2026", Some(2026)),
        ("Jan 24, 2022", Some(2022)),
        ("15/01/2026", Some(2026)),
        ("2015-05-27T00:00:00Z", Some(2015)),
        ("12345", None),
        ("soon", None),
        ("", None),
    ] {
        assert_eq!(year_of(date), year, "{date:?}");
    }
}

#[test]
fn sources_have_json_names() {
    assert_eq!(
        serde_json::to_value(PageSource::DublinCore).unwrap(),
        "dublin_core"
    );
    for s in [
        PageSource::Highwire,
        PageSource::Prism,
        PageSource::DublinCore,
        PageSource::JsonLd,
        PageSource::Title,
        PageSource::TitleHint,
        PageSource::Url,
    ] {
        assert_eq!(serde_json::to_value(s).unwrap(), s.as_str());
    }
}

/// Found by fuzzing: a tag's DOI with an entity for a dash kept the dash,
/// so the DOI looked up wasn't the one the page meant.
#[test]
fn look_alikes_in_tags_are_folded() {
    let p = page(&meta(
        "citation_doi",
        "10.1038/s41586&#x2013;020&#8203;-2649-2",
    ));
    assert_eq!(ids(&p), ["doi:10.1038/s41586-020-2649-2"]);
}
