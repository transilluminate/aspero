//! Claims from publishers' article pages: trimmed `<head>`s (and the
//! JSON-LD scripts an app adds from the body) in `tests/fixtures/pages`,
//! modelled on each publisher's markup. Web pages too: modelled ones,
//! and in `web/` real pages' heads (fetched 2026-11-16), only their
//! `<html lang>`, `<title>`, describing meta tags, canonical link and
//! JSON-LD (long prose cut).

use aspero_core::{PageClaim, PageKind, PageSource, claim_from_page};

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/pages/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn read(name: &str, url: &str) -> PageClaim {
    claim_from_page(Some(&fixture(name)), url, None)
}

fn ids(p: &PageClaim) -> Vec<String> {
    p.ids.iter().map(|i| i.mention.id.to_string()).collect()
}

#[test]
fn jama() {
    let p = read(
        "jama.html",
        "https://jamanetwork.com/journals/jama/article-abstract/2856706",
    );
    assert_eq!(p.source, PageSource::Highwire);
    // Not the script's or the comment's DOI.
    assert_eq!(ids(&p), ["doi:10.1001/jama.2026.19482"]);
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some("Treatment of Acute Ischemic Stroke: A Review")
    );
    assert_eq!(c.authors, ["Albers, Gregory W.", "Heit, Jeremy J."]);
    assert_eq!(c.container.as_deref(), Some("JAMA"));
    assert_eq!(c.year, Some(2026));
    // Online first: no volume or pages yet.
    assert_eq!(c.volume, None);
    assert_eq!(c.page, None);
    assert_eq!(c.ids.len(), 1);
}

#[test]
fn jama_json_ld_alone() {
    // Without the Highwire tags, the article's JSON-LD block says the
    // same (the website's block is passed over).
    let html: String = fixture("jama.html")
        .lines()
        .filter(|l| l.contains("ld+json"))
        .collect();
    let p = claim_from_page(
        Some(&html),
        "https://jamanetwork.com/journals/jama/fullarticle/2856706",
        None,
    );
    assert_eq!(p.source, PageSource::JsonLd);
    assert_eq!(ids(&p), ["doi:10.1001/jama.2026.19482"]);
    let c = p.claim.unwrap();
    assert_eq!(c.authors, ["Gregory W. Albers", "Jeremy J. Heit"]);
    assert_eq!(c.container.as_deref(), Some("JAMA"));
    assert_eq!(c.year, Some(2026));
}

#[test]
fn oxford_academic() {
    let p = read(
        "oup.html",
        "https://academic.oup.com/brain/article/145/4/1201/6577777",
    );
    assert_eq!(p.source, PageSource::Highwire);
    // A reference's DOI, inside `citation_reference`, isn't the page's.
    assert_eq!(ids(&p), ["doi:10.1093/brain/awac123"]);
    let c = p.claim.unwrap();
    assert_eq!(c.container.as_deref(), Some("Brain"));
    assert_eq!(c.authors, ["Mander, Bryce A", "Walker, Matthew P"]);
    assert_eq!(c.year, Some(2022));
}

#[test]
fn nature() {
    let p = read("nature.html", "https://www.nature.com/articles/nature14539");
    assert_eq!(p.source, PageSource::Highwire);
    assert_eq!(ids(&p), ["doi:10.1038/nature14539"]);
    assert_eq!(p.ids[0].source, PageSource::Highwire);
    let c = p.claim.unwrap();
    assert_eq!(c.title.as_deref(), Some("Deep learning"));
    assert_eq!(c.container.as_deref(), Some("Nature"));
    assert_eq!(c.year, Some(2015));
    assert_eq!(c.page.as_deref(), Some("436-444"));
    assert_eq!(
        c.authors,
        ["LeCun, Yann", "Bengio, Yoshua", "Hinton, Geoffrey"]
    );
}

#[test]
fn sciencedirect() {
    let url = "https://www.sciencedirect.com/science/article/pii/S0140673620309259";
    let p = read("sciencedirect.html", url);
    assert_eq!(p.source, PageSource::Highwire);
    // The DOI and the PII, the address's PII the same one.
    assert_eq!(
        ids(&p),
        ["doi:10.1016/s0140-6736(20)30925-9", "pii:S0140673620309259"]
    );
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some(
            "Global burden of 369 diseases and injuries in 204 countries and territories, \
             1990–2019: a systematic analysis for the Global Burden of Disease Study 2019"
        )
    );
    assert_eq!(c.container.as_deref(), Some("The Lancet"));
    assert_eq!(c.page.as_deref(), Some("1204-1222"));
    assert!(c.authors.is_empty());
}

#[test]
fn plos() {
    let p = read(
        "plos.html",
        "https://journals.plos.org/plosone/article?id=10.1371/journal.pone.0262958",
    );
    assert_eq!(p.source, PageSource::Highwire);
    assert_eq!(ids(&p), ["doi:10.1371/journal.pone.0262958"]);
    let c = p.claim.unwrap();
    assert_eq!(c.year, Some(2022));
    assert_eq!(c.page.as_deref(), Some("e0262958"));
    assert_eq!(c.authors.len(), 4);
    assert_eq!(c.authors[0], "Binod Acharya");
    assert_eq!(c.container.as_deref(), Some("PLOS ONE"));
}

#[test]
fn arxiv() {
    let p = read("arxiv.html", "https://arxiv.org/abs/1706.03762");
    assert_eq!(p.source, PageSource::Highwire);
    assert_eq!(ids(&p), ["arxiv:1706.03762"]);
    let c = p.claim.unwrap();
    assert_eq!(c.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(c.authors.len(), 8);
    // The first version's date, not the latest's.
    assert_eq!(c.year, Some(2017));
    assert_eq!(c.container, None);
}

#[test]
fn dublin_core_repository() {
    let p = read(
        "repository-dc.html",
        "https://eprints.example.ac.uk/id/eprint/421337/",
    );
    assert_eq!(p.source, PageSource::DublinCore);
    assert_eq!(ids(&p), ["doi:10.1136/bmjqs-2017-007444"]);
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some("Nurse staffing levels and patient mortality in English hospitals")
    );
    assert_eq!(c.authors, ["Griffiths, Peter", "Ball, Jane"]);
    assert_eq!(c.year, Some(2018));
    assert_eq!(c.container.as_deref(), Some("BMJ Quality & Safety"));
}

#[test]
fn json_ld_only() {
    let p = read(
        "jsonld-only.html",
        "https://journals.example.org/jip/article/42",
    );
    assert_eq!(p.source, PageSource::JsonLd);
    // Not the cited work's.
    assert_eq!(ids(&p), ["doi:10.5555/example.2024.0042"]);
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some("Hand hygiene compliance on surgical wards: an observational study")
    );
    assert_eq!(c.authors, ["Amara Okafor", "Lindqvist, Sofia"]);
    assert_eq!(c.year, Some(2024));
    assert_eq!(c.volume.as_deref(), Some("18"));
    assert_eq!(c.page.as_deref(), Some("112-119"));
    assert_eq!(
        c.container.as_deref(),
        Some("Journal of Infection Prevention")
    );
}

#[test]
fn title_only() {
    let p = read("title-only.html", "https://emj.bmj.com/content/41/2/100");
    assert_eq!(p.source, PageSource::Title);
    assert!(p.ids.is_empty());
    let c = p.claim.unwrap();
    assert_eq!(
        c.title.as_deref(),
        Some("Chest wall thickness and needle decompression failure in trauma patients")
    );
    assert!(c.ids.is_empty());
    // Never the journal from the title or the address.
    assert_eq!(c.container, None);
}

#[test]
fn every_fixture_reads_cut_anywhere() {
    // The app may pass any part of the markup, cut anywhere: each piece
    // reads, whatever it holds.
    for name in [
        "jama.html",
        "oup.html",
        "nature.html",
        "sciencedirect.html",
        "plos.html",
        "arxiv.html",
        "repository-dc.html",
        "jsonld-only.html",
        "title-only.html",
        "wikipedia.html",
        "news.html",
        "guidance.html",
    ] {
        let html = fixture(name);
        for cut in (0..html.len())
            .step_by(61)
            .filter(|&i| html.is_char_boundary(i))
        {
            for part in [&html[..cut], &html[cut..]] {
                let p = claim_from_page(Some(part), "https://example.org/", Some("Full Text"));
                if let Some(c) = &p.claim {
                    assert_eq!(c.ids.len(), p.ids.len(), "{name} at {cut}");
                }
                // A web page always has its record, and never a claim.
                if p.kind == PageKind::WebPage {
                    assert!(p.claim.is_none() && p.web_page.is_some(), "{name} at {cut}");
                }
            }
        }
    }
}

fn date(d: Option<aspero_core::PartialDate>) -> Option<String> {
    d.map(|d| d.to_string())
}

/// #118: Wikipedia's JSON-LD `headline` is the article's short
/// description; its `name` is the title. #119: Wikipedia is a web page,
/// not a work to verify.
#[test]
fn wikipedia_is_a_web_page_named_by_its_name() {
    let url = "https://en.wikipedia.org/wiki/Gene_Hackman#Early_life";
    let p = read("wikipedia.html", url);
    assert_eq!(p.kind, PageKind::WebPage);
    assert!(p.ids.is_empty());
    assert_eq!(p.claim, None);
    let web = p.web_page.unwrap();
    assert_eq!(web.title.as_deref(), Some("Gene Hackman"));
    // Its title's suffix names the host; the publisher is the foundation.
    assert_eq!(web.site.as_deref(), Some("Wikipedia"));
    assert_eq!(web.url, "https://en.wikipedia.org/wiki/Gene_Hackman");
    assert_eq!(web.key, "url:https://en.wikipedia.org/wiki/Gene_Hackman");
    // "Contributors to Wikimedia projects" is an organisation.
    assert!(web.authors.is_empty());
    assert_eq!(date(web.published).as_deref(), Some("2001-09-13"));
    assert_eq!(date(web.updated).as_deref(), Some("2026-11-12"));
    assert_eq!(web.language.as_deref(), Some("en"));

    // The `<head>` and JSON-LD alone, without `<html lang>`: the rest
    // still reads.
    let head: String = fixture("wikipedia.html")
        .lines()
        .filter(|l| !l.starts_with("<html"))
        .collect();
    let web = claim_from_page(Some(&head), url, None).web_page.unwrap();
    assert_eq!(web.title.as_deref(), Some("Gene Hackman"));
    assert_eq!(web.language, None);
}

/// #118 on a scholarly page: `name` before `headline`.
#[test]
fn json_ld_names_before_headlines() {
    let html = r#"<script type="application/ld+json">{"@type": "ScholarlyArticle",
        "name": "Sepsis in adults", "headline": "A review of sepsis in adults for clinicians",
        "sameAs": "https://doi.org/10.5555/sepsis"}</script>"#;
    let p = claim_from_page(Some(html), "https://example.org/a", None);
    assert_eq!(p.kind, PageKind::Work);
    assert_eq!(p.claim.unwrap().title.as_deref(), Some("Sepsis in adults"));
    // A `name` that isn't a title: the headline.
    let html = html.replace("Sepsis in adults\"", "https://example.org/a\"");
    let p = claim_from_page(Some(&html), "https://example.org/a", None);
    assert_eq!(
        p.claim.unwrap().title.as_deref(),
        Some("A review of sepsis in adults for clinicians")
    );
}

#[test]
fn a_news_article_is_a_web_page_by_its_journalists() {
    let url = "https://www.example-times.co.uk/news/health/waiting-lists-fall-123456?utm_source=twitter#comments";
    let p = read("news.html", url);
    assert_eq!(p.kind, PageKind::WebPage);
    // A study it cites isn't the page's.
    assert!(p.ids.is_empty());
    assert_eq!(p.claim, None);
    let web = p.web_page.unwrap();
    assert_eq!(
        web.title.as_deref(),
        Some("Hospital waiting lists fall for third month running")
    );
    assert_eq!(web.site.as_deref(), Some("The Example Times"));
    // The canonical link, relative, resolved against the address.
    assert_eq!(
        web.url,
        "https://www.example-times.co.uk/news/health/waiting-lists-fall-123456"
    );
    // People only: not the paper itself.
    assert_eq!(web.authors, ["Priya Shah", "Tom O'Neill"]);
    assert_eq!(date(web.published).as_deref(), Some("2026-11-13"));
    assert_eq!(web.language.as_deref(), Some("en-GB"));
}

#[test]
fn an_organisations_guidance_has_no_author() {
    let p = read(
        "guidance.html",
        "https://www.example-health.org.uk/guidance/ng51?fbclid=x",
    );
    assert_eq!(p.kind, PageKind::WebPage);
    assert_eq!(p.claim, None);
    let web = p.web_page.unwrap();
    assert_eq!(
        web.title.as_deref(),
        Some("Sepsis: recognition, diagnosis and early management")
    );
    assert_eq!(web.site.as_deref(), Some("Example Health Institute"));
    assert_eq!(web.url, "https://www.example-health.org.uk/guidance/ng51");
    assert!(web.authors.is_empty());
    assert_eq!(date(web.published).as_deref(), Some("2016-07-13"));
    assert_eq!(date(web.updated).as_deref(), Some("2024-03-19"));
    assert_eq!(web.language.as_deref(), Some("en-GB"));

    // Its `<title>` alone: as the page gives it, but for the site's own
    // name (`og:site_name`'s).
    let p = claim_from_page(
        Some(
            "<title>Sepsis: recognition, diagnosis and early management | Guidance | Example Health Institute</title>\
             <meta property=\"og:site_name\" content=\"Example Health Institute\">",
        ),
        "https://www.example-health.org.uk/guidance/ng51",
        None,
    );
    let web = p.web_page.unwrap();
    assert_eq!(
        web.title.as_deref(),
        Some("Sepsis: recognition, diagnosis and early management")
    );
}

#[test]
fn scholarly_pages_stay_works() {
    for (name, url) in [
        (
            "jama.html",
            "https://jamanetwork.com/journals/jama/article-abstract/2856706",
        ),
        (
            "jsonld-only.html",
            "https://journals.example.org/jip/article/42",
        ),
        (
            "repository-dc.html",
            "https://eprints.example.ac.uk/id/eprint/421337/",
        ),
        ("arxiv.html", "https://arxiv.org/abs/1706.03762"),
        ("title-only.html", "https://emj.bmj.com/content/41/2/100"),
    ] {
        let p = read(name, url);
        assert_eq!(p.kind, PageKind::Work, "{name}");
        assert!(p.claim.is_some(), "{name}");
    }
    // A work's page has a web page record too, to cite when its work
    // isn't found.
    let p = read(
        "jama.html",
        "https://jamanetwork.com/journals/jama/article-abstract/2856706",
    );
    let web = p.web_page.unwrap();
    assert_eq!(web.site.as_deref(), Some("JAMA Network"));
    assert_eq!(
        web.url,
        "https://jamanetwork.com/journals/jama/fullarticle/2856706"
    );
    assert_eq!(web.authors, ["Gregory W. Albers", "Jeremy J. Heit"]);
}

/// Real pages' heads: each a web page, read as a careful person would
/// cite it. One falls short, and says so: a title whose site nothing
/// names stays as the page gives it.
#[test]
fn real_pages_read_as_cited() {
    /// File, address, title, site, authors, published.
    type Case<'a> = (
        &'a str,
        &'a str,
        &'a str,
        &'a str,
        &'a [&'a str],
        Option<&'a str>,
    );
    let cases: [Case; 19] = [
        (
            "wikipedia",
            "https://en.wikipedia.org/wiki/Gene_Hackman",
            "Gene Hackman",
            "Wikipedia",
            &[],
            Some("2001-09-13"),
        ),
        (
            "govuk",
            "https://www.gov.uk/guidance/the-highway-code",
            "The Highway Code",
            "GOV.UK",
            &[],
            Some("2015-10-01"),
        ),
        (
            // Its JSON-LD names the page "Detail", its template: the
            // `og:title` first.
            "who",
            "https://www.who.int/news-room/fact-sheets/detail/sepsis",
            "Sepsis",
            "who.int",
            &[],
            None,
        ),
        (
            // "NHS website", its JSON-LD author, is an organisation.
            "nhs",
            "https://www.nhs.uk/conditions/sepsis/",
            "Sepsis",
            "nhs.uk",
            &[],
            Some("2017-10-23"),
        ),
        (
            "nice",
            "https://www.nice.org.uk/guidance/ng51",
            "Suspected sepsis: recognition, diagnosis and early management",
            "NICE",
            &[],
            None,
        ),
        (
            "bbc",
            "https://www.bbc.co.uk/news/articles/cm9d7pe8y559o",
            "Russell Brand prison message claiming innocence being investigated, No 10 says",
            "BBC News",
            &["Sima Kotcha"],
            Some("2026-11-16"),
        ),
        (
            // JSON-LD's author is "Guardian staff reporter", a byline
            // holding the site's name: `article:author`'s person instead.
            "guardian",
            "https://www.theguardian.com/lifeandstyle/2026/nov/16/the-pet-ill-never-forget-mr-baxter-who-ate-our-belongings-and-broke-our-hearts",
            "The pet I’ll never forget: Mr Baxter, who ate our belongings – and broke our hearts",
            "the Guardian",
            &["Camilla Balshaw"],
            Some("2026-11-16"),
        ),
        (
            "spiegel",
            "https://www.spiegel.de/karriere/karriere-wie-weiterbildungen-mit-50-die-laufbahn-ausbremsen-koennen-a-1699cdde-97cd-4a15-b6f9-1a52579e0c11",
            "Tipps vom Karrierecoach: Wie man sich mit Weiterbildungen die Karriere ruiniert",
            "DER SPIEGEL",
            &["Ines Schöffmann"],
            Some("2026-11-16"),
        ),
        (
            "elpais",
            "https://elpais.com/economia/2026-11-16/el-gobierno-aprobara-este-martes-la-subida-del-salario-minimo-de-2027-a-1286-euros.html",
            "El Gobierno aprobará este martes la subida del salario mínimo de 2027 a 1.286 euros",
            "El País",
            &["Emilio Sánchez Hidalgo"],
            Some("2026-11-16"),
        ),
        (
            "npr",
            "https://www.npr.org/2026/11/16/nx-s1-6041738/trump-xi-and-putin-china-nvidia",
            "Trump heads to Asia to meet with Xi and Putin amid Nvidia fallout",
            "NPR",
            &["Deepa Shivaram"],
            Some("2026-11-16"),
        ),
        (
            "substack",
            "https://www.astralcodexten.com/p/the-psychopolitics-of-trauma",
            "The Psychopolitics Of Trauma",
            "Astral Codex Ten",
            &["Scott Alexander"],
            Some("2024-01-25"),
        ),
        (
            "code-repository",
            "https://github.com/rust-lang/rust",
            "rust-lang/rust: Empowering everyone to build reliable and efficient software.",
            "GitHub",
            &[],
            None,
        ),
        (
            // `&lt;meta&gt;` decodes to text that looks like a tag: it
            // stays, as it isn't formatting. " - HTML | MDN" go as short
            // title-cased segments.
            "mdn",
            "https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/meta",
            "<meta> HTML metadata element",
            "MDN Web Docs",
            &[],
            None,
        ),
        (
            "python",
            "https://docs.python.org/3/library/json.html",
            "json — JSON encoder and decoder",
            "Python documentation",
            &[],
            None,
        ),
        (
            // The site's own name, whole, though it holds a separator.
            "mozblog",
            "https://hacks.mozilla.org/2019/03/standardizing-wasi-a-webassembly-system-interface/",
            "Standardizing WASI: A system interface to run WebAssembly outside the web",
            "Mozilla Hacks – the Web developer blog",
            &[],
            Some("2019-03-27"),
        ),
        (
            // Short of right: nothing names "Rust Blog" as the site's
            // (no `og:site_name`, not the host's name), so the title
            // stays as the page gives it rather than risk a wrong trim.
            "rustblog",
            "https://blog.rust-lang.org/2015/05/15/Rust-1.0/",
            "Announcing Rust 1.0 | Rust Blog",
            "blog.rust-lang.org",
            &[],
            None,
        ),
        (
            "cam",
            "https://www.cam.ac.uk/about-the-university/history",
            "History",
            "University of Cambridge",
            &[],
            None,
        ),
        (
            "mitnews",
            "https://news.mit.edu/2026/new-research-could-lead-faster-trustworthy-ai-models-climate-prediction-1112",
            "New research could lead to faster, trustworthy AI models for climate prediction",
            "MIT News | Massachusetts Institute of Technology",
            &[],
            None,
        ),
        (
            "stripe",
            "https://stripe.dev/blog/payment-api-design",
            "Stripe’s payments APIs: The first 10 years",
            "Stripe Dot Dev Blog",
            &["Michelle Bu"],
            Some("2020-12-15"),
        ),
    ];
    for (name, url, title, site, authors, published) in cases {
        let p = read(&format!("web/{name}.html"), url);
        assert_eq!(p.kind, PageKind::WebPage, "{name}");
        assert_eq!(p.claim, None, "{name}");
        let web = p.web_page.unwrap();
        assert_eq!(web.title.as_deref(), Some(title), "{name}");
        assert_eq!(web.site.as_deref(), Some(site), "{name}");
        assert_eq!(web.authors, authors, "{name}");
        assert_eq!(date(web.published).as_deref(), published, "{name}");
        assert_eq!(web.url, url, "{name}");
    }
}
