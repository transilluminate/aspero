//! Property tests for the identifier contract in `aspero_core::id`:
//! surface forms normalise to one canonical value, and canonical output
//! re-parses to itself.

use aspero_core::id::scan;
use aspero_core::{ArxivId, Doi, IdError, IdKind, Pmid, WorkId};
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use proptest::prelude::*;

/// Flips the ASCII case of characters where `mask` is set.
fn recase(s: &str, mask: &[bool]) -> String {
    s.chars()
        .zip(mask.iter().copied().chain(std::iter::repeat(false)))
        .map(|(c, flip)| match (flip, c.is_ascii_lowercase()) {
            (false, _) => c,
            (true, true) => c.to_ascii_uppercase(),
            (true, false) => c.to_ascii_lowercase(),
        })
        .collect()
}

prop_compose! {
    /// A canonical (lowercase) DOI. The suffix alphabet covers the punctuation
    /// real DOIs use, but not `?` or `#`, which would need URL-encoding, and
    /// never ends in what parsing strips from pasted text (sentence
    /// punctuation, `>`, an unbalanced `)`).
    fn canonical_doi()(
        registrant in "[0-9]{4,9}(\\.[0-9]{1,3})?",
        suffix in "[a-z0-9._;()/:<>-]{0,29}[a-z0-9/<-]",
    ) -> String {
        format!("10.{registrant}/{suffix}")
    }
}

prop_compose! {
    fn doi_surface()(
        canonical in canonical_doi(),
        mask in prop::collection::vec(any::<bool>(), 48),
        form in 0..7usize,
        trailing in "[.,;:]{0,2}",
        pad in "[ \t]{0,2}",
    ) -> (String, String) {
        let cased = recase(&canonical, &mask);
        let surface = match form {
            0 => cased,
            1 => format!("doi:{cased}"),
            2 => format!("DOI: {cased}"),
            3 => format!("info:doi/{cased}"),
            4 => format!("https://doi.org/{cased}"),
            5 => format!("http://dx.doi.org/{}", utf8_percent_encode(&cased, NON_ALPHANUMERIC)),
            _ => format!("doi.org/{cased}"),
        };
        (format!("{pad}{surface}{trailing}{pad}"), canonical)
    }
}

prop_compose! {
    /// A canonical arXiv id (new or old style), with or without version.
    fn canonical_arxiv()(
        new_style in any::<bool>(),
        yy in 15u32..100,
        mm in 1u32..13,
        seq in 0u32..100_000,
        archive in prop::sample::select(vec!["hep-th", "math", "cond-mat", "astro-ph", "q-bio"]),
        old_yy in prop::sample::select(vec![91u32, 95, 99, 0, 3, 6]),
        version in prop::option::of(1u32..30),
    ) -> String {
        let id = if new_style {
            format!("{yy:02}{mm:02}.{seq:05}")
        } else {
            format!("{archive}/{old_yy:02}{mm:02}{:03}", seq % 1000)
        };
        match version {
            Some(v) => format!("{id}v{v}"),
            None => id,
        }
    }
}

proptest! {
    #[test]
    fn doi_surface_forms_normalise((surface, canonical) in doi_surface()) {
        let doi: Doi = surface.parse().unwrap();
        prop_assert_eq!(doi.as_str(), canonical.as_str());

        let id: WorkId = surface.parse().unwrap();
        prop_assert_eq!(id, WorkId::Doi(doi));
    }

    #[test]
    fn arxiv_surface_forms_normalise(
        canonical in canonical_arxiv(),
        form in 0..5usize,
    ) {
        let surface = match form {
            0 => canonical.clone(),
            1 => format!("arXiv:{canonical}"),
            2 => format!("https://arxiv.org/abs/{canonical}"),
            3 => format!("https://arxiv.org/pdf/{canonical}.pdf"),
            _ => format!("ARXIV: {}", canonical.replace('v', "V")),
        };
        let parsed: ArxivId = surface.parse().unwrap();
        prop_assert_eq!(parsed.to_string(), canonical.clone());
        prop_assert_eq!(surface.parse::<WorkId>().unwrap(), WorkId::Arxiv(parsed));
    }

    #[test]
    fn pmid_forms_normalise(n in 1u64..100_000_000, zeros in 0..3usize, form in 0..3usize) {
        let digits = format!("{}{n}", "0".repeat(zeros));
        let surface = match form {
            0 => format!("PMID:{digits}"),
            1 => format!("pmid: {digits}"),
            _ => format!("https://pubmed.ncbi.nlm.nih.gov/{digits}/"),
        };
        prop_assert_eq!(surface.parse::<Pmid>().unwrap().get(), n);
        prop_assert_eq!(surface.parse::<WorkId>().unwrap().to_string(), format!("pmid:{n}"));
    }

    #[test]
    fn bare_digits_never_guess_a_kind_unless_an_isbn_13(n in 1u64..u64::MAX) {
        let digits = n.to_string();
        match digits.parse::<WorkId>() {
            Err(IdError::Ambiguous { .. }) => {}
            Ok(WorkId::Isbn(isbn)) => {
                prop_assert_eq!(isbn.as_str(), digits.as_str());
                prop_assert!(digits.starts_with("978") || digits.starts_with("979"));
            }
            other => prop_assert!(false, "{digits}: {other:?}"),
        }
    }

    #[test]
    fn isbn_13s_are_recognised_bare(body in 0u64..1_000_000_000, prefix in prop::sample::select(vec!["978", "979"])) {
        let isbn = isbn13(prefix, body);
        let parsed = isbn.parse::<WorkId>().unwrap();
        prop_assert_eq!(parsed.kind(), IdKind::Isbn);
        prop_assert_eq!(parsed.to_string(), format!("isbn:{isbn}"));
    }

    /// The core idempotence property, over arbitrary input: whatever parses
    /// displays canonically, and the canonical form re-parses to the same
    /// value — as a `WorkId`, as its own kind, and through serde.
    #[test]
    fn whatever_parses_round_trips(input in "\\PC{0,60}") {
        if let Ok(id) = input.parse::<WorkId>() {
            assert_round_trips(&id)?;
        }
    }

    /// Whatever `scan` finds, lookup's parser reads the same way: each
    /// mention as written parses to its identifier.
    #[test]
    fn scanned_mentions_parse_as_found(text in "\\PC{0,80}") {
        assert_mentions_parse(&text)?;
    }

    #[test]
    fn labelled_mentions_parse_as_found(
        n in 1u64..100_000_000,
        doi in canonical_doi(),
        arxiv in canonical_arxiv(),
        form in 0..9usize,
        gap in prop::sample::select(vec![" ", ": ", ":", "  ", "\t"]),
        before in "[a-z ,(]{0,12}",
        after in "[a-z .,)]{0,12}",
    ) {
        let (label, value) = match form {
            0 => ("PMID", n.to_string()),
            1 => ("pmid", n.to_string()),
            2 => ("PMCID", format!("PMC{n}")),
            3 => ("ISBN", isbn13("978", n % 1_000_000_000)),
            4 => ("ISBN-13", isbn13("979", n % 1_000_000_000)),
            5 => ("doi", doi),
            6 => ("DOI", doi.to_uppercase()),
            7 => ("arXiv", arxiv),
            _ => ("NBK", format!("NBK{n}")),
        };
        let text = format!("{before} {label}{gap}{value} {after}");
        prop_assert!(!scan(&text).is_empty(), "{text:?}");
        assert_mentions_parse(&text)?;
    }

    /// A DOI broken across lines, at a hyphen or not, as PDFs break them.
    #[test]
    fn broken_dois_have_canonical_alternatives(
        doi in canonical_doi(),
        at in any::<prop::sample::Index>(),
        hyphen in any::<bool>(),
        next in "[a-z0-9]{0,6}",
    ) {
        let cut = at.index(doi.len() + 1);
        let (head, tail) = doi.split_at(cut);
        let hyphen = if hyphen { "-" } else { "" };
        let text = format!("See doi:{head}{hyphen}\n   {tail} {next}.");
        assert_alternatives_round_trip(&text)?;
    }

    #[test]
    fn generated_ids_round_trip(
        doi in canonical_doi(),
        arxiv in canonical_arxiv(),
        n in 1u64..u64::MAX,
        kind in prop::sample::select(IdKind::ALL.to_vec()),
    ) {
        let input = match kind {
            IdKind::Doi => doi,
            IdKind::Arxiv => arxiv,
            IdKind::Pmcid => format!("PMC{n}"),
            IdKind::Nbk => format!("NBK{n}"),
            IdKind::OpenAlex => format!("W{n}"),
            IdKind::S2Paper => format!("{n:040x}"),
            IdKind::Isbn => isbn13("978", n % 1_000_000_000),
            _ => n.to_string(),
        };
        let id = WorkId::parse_as(kind, &input).unwrap();
        prop_assert_eq!(id.kind(), kind);
        assert_round_trips(&id)?;
    }
}

/// A checksum-valid ISBN-13 from a prefix and nine digits.
fn isbn13(prefix: &str, body: u64) -> String {
    let first12 = format!("{prefix}{body:09}");
    let sum: u32 = first12
        .bytes()
        .enumerate()
        .map(|(i, b)| u32::from(b - b'0') * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    format!("{first12}{}", (10 - sum % 10) % 10)
}

fn assert_round_trips(id: &WorkId) -> Result<(), TestCaseError> {
    let shown = id.to_string();
    prop_assert_eq!(&shown.parse::<WorkId>().unwrap(), id);

    let (prefix, bare) = shown.split_once(':').unwrap();
    prop_assert_eq!(prefix, id.kind().prefix());
    prop_assert_eq!(&WorkId::parse_as(id.kind(), bare).unwrap(), id);

    let json = serde_json::to_string(id).unwrap();
    prop_assert_eq!(&serde_json::from_str::<WorkId>(&json).unwrap(), id);
    Ok(())
}

fn assert_mentions_parse(text: &str) -> Result<(), TestCaseError> {
    for m in scan(text) {
        let written = &text[m.span.clone()];
        prop_assert_eq!(
            &written.parse::<WorkId>(),
            &Ok(m.id.clone()),
            "{:?} in {:?}",
            written,
            text
        );
    }
    assert_alternatives_round_trip(text)
}

/// Each mention's other reading, if any, is another DOI, and canonical.
fn assert_alternatives_round_trip(text: &str) -> Result<(), TestCaseError> {
    for m in scan(text) {
        if let Some(other) = m.alternative(text) {
            prop_assert_eq!(other.kind(), IdKind::Doi, "{:?}", text);
            prop_assert_ne!(&other, &m.id);
            assert_round_trips(&other)?;
        }
    }
    Ok(())
}
