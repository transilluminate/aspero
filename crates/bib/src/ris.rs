//! RIS: one tagged field per line (`TI  - Deep learning`), records from
//! `TY` to `ER`. Read leniently, as exporters differ: one or two spaces
//! before the hyphen, lines wrapped without a tag, the journal under any
//! of `T2`, `JO`, `JF`, `JA` or `J2`, the year in `PY`, `Y1` or `DA`.

use aspero_core::Claim;

use crate::{BibEntry, Format, Ids, ReadError, first_year};

/// A line's tag and value: `("TI", "Deep learning")`.
pub(crate) fn tag(line: &str) -> Option<(&str, &str)> {
    let (tag, after) = line.trim_end_matches('\r').split_at_checked(2)?;
    let mut chars = tag.chars();
    let tagged = chars.next().is_some_and(|c| c.is_ascii_uppercase())
        && chars
            .next()
            .is_some_and(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    // At least one space before the hyphen: "UK-based" is text.
    let rest = after.trim_start_matches(' ');
    let value = rest.strip_prefix('-')?;
    (tagged && rest.len() < after.len()).then_some((tag, value.trim()))
}

/// Parses an `.ris` file's contents into entries, in file order.
pub fn parse_ris(src: &str) -> Result<Vec<BibEntry>, ReadError> {
    let mut entries = Vec::new();
    let mut record: Option<Record> = None;
    let mut offset = 0;
    for line in src.split_inclusive('\n') {
        let at = offset;
        offset += line.len();
        let text = line.trim_end_matches(['\n', '\r']);
        let Some((tag, value)) = tag(text) else {
            // A wrapped value continues the field before it.
            if let Some(r) = &mut record
                && let Some((_, last)) = r.fields.last_mut()
                && !text.trim().is_empty()
            {
                last.push(' ');
                last.push_str(text.trim());
            }
            continue;
        };
        match (tag, &mut record) {
            ("TY", Some(_)) => return Err(invalid(src, at, "TY before the previous record's ER")),
            ("TY", None) => {
                record = Some(Record {
                    entry_type: value.to_lowercase(),
                    fields: Vec::new(),
                });
            }
            ("ER", Some(_)) => {
                let r = record.take().expect("checked");
                entries.push(r.entry(entries.len() + 1));
            }
            (_, None) => return Err(invalid(src, at, &format!("{tag} outside a record"))),
            (_, Some(r)) => r.fields.push((tag.to_owned(), value.to_owned())),
        }
    }
    match record {
        // A last record missing its ER is still a record.
        Some(r) => entries.push(r.entry(entries.len() + 1)),
        None if entries.is_empty() => return Err(invalid(src, 0, "no records")),
        None => {}
    }
    Ok(entries)
}

fn invalid(src: &str, at: usize, message: &str) -> ReadError {
    ReadError::Invalid {
        format: Format::Ris,
        line: src[..at].lines().count() + 1,
        message: message.into(),
    }
}

struct Record {
    entry_type: String,
    fields: Vec<(String, String)>,
}

impl Record {
    /// The first non-empty value under any of `tags`, in their order.
    fn first(&self, tags: &[&str]) -> Option<String> {
        tags.iter().find_map(|t| {
            self.fields
                .iter()
                .find(|(tag, v)| tag == t && !v.is_empty())
                .map(|(_, v)| v.clone())
        })
    }

    fn all<'a>(&'a self, tags: &'a [&str]) -> impl Iterator<Item = &'a str> {
        self.fields
            .iter()
            .filter(|(tag, v)| tags.contains(&tag.as_str()) && !v.is_empty())
            .map(|(_, v)| v.as_str())
    }

    fn entry(self, position: usize) -> BibEntry {
        let mut claim = Claim::default();
        claim.title = self.first(&["TI", "T1", "CT", "BT"]);
        claim.authors = self.all(&["AU", "A1"]).map(str::to_owned).collect();
        claim.year = self.first(&["PY", "Y1", "DA"]).and_then(|y| first_year(&y));
        claim.container = self.first(&["T2", "JO", "JF", "JA", "J2"]);
        claim.volume = self.first(&["VL"]);
        claim.page = self.first(&["SP"]).map(|start| match self.first(&["EP"]) {
            Some(end) if !start.contains('-') => format!("{start}-{end}"),
            _ => start,
        });

        let mut ids = Ids::default();
        for doi in self.all(&["DO"]) {
            ids.doi(doi);
        }
        // `SN` holds ISBNs for books and ISSNs for journals.
        let book = matches!(
            self.entry_type.as_str(),
            "book" | "chap" | "ebook" | "echap" | "edbook"
        );
        for sn in self.all(&["SN"]) {
            ids.isbns(sn, book);
        }
        for url in self.all(&["UR", "L1", "L2"]) {
            ids.urls(url);
        }
        claim.ids = ids.ids;

        BibEntry {
            key: self.first(&["ID"]).unwrap_or_else(|| position.to_string()),
            entry_type: self.entry_type,
            claim,
            notes: ids.notes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RIS: &str = "TY  - JOUR\r
AU  - LeCun, Yann\r
AU  - Bengio, Yoshua\r
TI  - Deep\r
  learning\r
T2  - Nature\r
PY  - 2015/05/27/\r
VL  - 521\r
SP  - 436\r
EP  - 444\r
SN  - 0028-0836\r
DO  - 10.1038/nature14539\r
ER  - \r
\r
TY  - BOOK\r
ID  - goodfellow\r
AU  - Goodfellow, Ian\r
TI  - Deep Learning\r
PY  - 2016\r
SN  - 978-0-262-03561-3\r
DO  - not-a-doi\r
ER  -\r
TY - GEN\r
TI - Attention is all you need\r
UR - https://arxiv.org/abs/1706.03762\r
";

    #[test]
    fn records_become_claims() {
        let entries = parse_ris(RIS).unwrap();
        let keys: Vec<&str> = entries.iter().map(|e| e.key.as_str()).collect();
        assert_eq!(keys, ["1", "goodfellow", "3"]);

        let lecun = &entries[0];
        assert_eq!(lecun.entry_type, "jour");
        assert_eq!(lecun.claim.title.as_deref(), Some("Deep learning"));
        assert_eq!(lecun.claim.authors, ["LeCun, Yann", "Bengio, Yoshua"]);
        assert_eq!(lecun.claim.year, Some(2015));
        assert_eq!(lecun.claim.container.as_deref(), Some("Nature"));
        assert_eq!(lecun.claim.volume.as_deref(), Some("521"));
        assert_eq!(lecun.claim.page.as_deref(), Some("436-444"));
        assert_eq!(lecun.claim.ids[0].to_string(), "doi:10.1038/nature14539");
        assert!(lecun.notes.is_empty(), "an ISSN isn't a bad ISBN");

        let book = &entries[1];
        assert_eq!(book.claim.ids[0].to_string(), "isbn:9780262035613");
        assert_eq!(book.notes, ["doi field \"not-a-doi\" is not a valid DOI"]);

        let preprint = &entries[2];
        assert_eq!(preprint.claim.ids[0].to_string(), "arxiv:1706.03762");
    }

    #[test]
    fn broken_files_name_the_line() {
        let err = parse_ris("TY  - JOUR\nTI  - One\nTY  - JOUR\nER  -\n").unwrap_err();
        assert_eq!(
            err,
            ReadError::Invalid {
                format: Format::Ris,
                line: 3,
                message: "TY before the previous record's ER".into()
            }
        );
        assert!(parse_ris("AU  - Nobody\n").is_err());
    }
}
