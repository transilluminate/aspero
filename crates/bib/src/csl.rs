//! CSL-JSON files (as Zotero, Mendeley and pandoc export them): an array
//! of items, or a single item. Read with the lenient [`CslItem`] used for
//! doi.org's responses.

use aspero_core::Claim;
use aspero_core::csl::CslItem;

use crate::{BibEntry, Format, Ids, ReadError};

/// Parses a CSL-JSON file's contents into entries, in file order.
pub fn parse_csl_json(src: &str) -> Result<Vec<BibEntry>, ReadError> {
    let invalid = |e: serde_json::Error| ReadError::Invalid {
        format: Format::CslJson,
        line: e.line().max(1),
        message: e.to_string(),
    };
    let src = src.trim_start_matches('\u{FEFF}');
    let items: Vec<CslItem> = match src.trim_start().starts_with('{') {
        true => vec![serde_json::from_str(src).map_err(invalid)?],
        false => serde_json::from_str(src).map_err(invalid)?,
    };
    Ok(items
        .iter()
        .enumerate()
        .map(|(i, item)| entry(item, i + 1))
        .collect())
}

fn entry(item: &CslItem, position: usize) -> BibEntry {
    let work = item.to_work();
    let mut claim = Claim::default();
    claim.title = work.title.clone();
    claim.authors = work
        .authors
        .iter()
        .map(|p| match (&p.family, &p.given, &p.literal) {
            (Some(family), Some(given), _) => format!("{family}, {given}"),
            (Some(name), None, _) | (None, _, Some(name)) => name.clone(),
            _ => String::new(),
        })
        .filter(|a| !a.is_empty())
        .collect();
    claim.year = work.issued.map(|d| d.year());
    claim.container = work.container_title.clone();
    claim.volume = work.volume.clone();
    claim.page = work.page.clone();

    let mut ids = Ids::default();
    if let Some(doi) = &item.doi {
        ids.doi(doi);
    }
    if let Some(pmid) = &item.pmid {
        ids.pmid(&pmid.text());
    }
    if let Some(pmcid) = &item.pmcid {
        ids.urls(&format!("PMCID: {pmcid}"));
    }
    if let Some(isbn) = &item.isbn {
        ids.isbns(isbn, true);
    }
    if let Some(url) = &item.url {
        ids.urls(url);
    }
    claim.ids = ids.ids;

    BibEntry {
        key: item
            .id
            .as_ref()
            .map_or_else(|| position.to_string(), |id| id.text()),
        entry_type: item.item_type.clone().unwrap_or_default().to_lowercase(),
        claim,
        notes: ids.notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_become_claims() {
        let src = r#"[
          {"id": "lecun2015", "type": "article-journal", "title": "Deep learning",
           "author": [{"family": "LeCun", "given": "Yann"}, {"literal": "WHO"}],
           "container-title": "Nature", "volume": 521, "page": "436-444",
           "issued": {"date-parts": [["2015", 5]]}, "DOI": "10.1038/nature14539",
           "PMID": 26017442},
          {"type": "book", "title": "Deep Learning", "ISBN": "9780262035613"}
        ]"#;
        let entries = parse_csl_json(src).unwrap();
        let lecun = &entries[0];
        assert_eq!(lecun.key, "lecun2015");
        assert_eq!(lecun.entry_type, "article-journal");
        assert_eq!(lecun.claim.authors, ["LeCun, Yann", "WHO"]);
        assert_eq!(lecun.claim.year, Some(2015));
        assert_eq!(lecun.claim.volume.as_deref(), Some("521"));
        let ids: Vec<String> = lecun.claim.ids.iter().map(ToString::to_string).collect();
        assert_eq!(ids, ["doi:10.1038/nature14539", "pmid:26017442"]);
        assert_eq!(entries[1].key, "2");
        assert_eq!(entries[1].claim.ids[0].to_string(), "isbn:9780262035613");

        let one = parse_csl_json(r#"{"id": 7, "title": "Alone"}"#).unwrap();
        assert_eq!(one[0].key, "7");
    }

    #[test]
    fn broken_files_name_the_line() {
        let err = parse_csl_json("[\n  {\"title\": }\n]").unwrap_err();
        assert!(matches!(err, ReadError::Invalid { line: 2, .. }), "{err}");
    }
}
