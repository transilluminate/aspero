//! Claims from publishers' article pages: trimmed `<head>`s (and the
//! JSON-LD scripts an app adds from the body) in `tests/fixtures/pages`,
//! modelled on each publisher's markup.

use aspero_core::{PageClaim, PageSource, claim_from_page};

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
        Some(
            "Semaglutide and Kidney Outcomes in Adults With Type 2 Diabetes and Chronic \
             Kidney Disease: A Randomized Clinical Trial"
        )
    );
    assert_eq!(c.authors, ["Rossing, Peter", "Tuttle, Katherine R."]);
    assert_eq!(c.container.as_deref(), Some("JAMA"));
    assert_eq!(c.year, Some(2026));
    assert_eq!(c.volume.as_deref(), Some("335"));
    assert_eq!(c.page.as_deref(), Some("221-232"));
    assert_eq!(c.ids.len(), 1);
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
            }
        }
    }
}
