//! Property tests for the identifier contract in `aspero_core::id`:
//! surface forms normalise to one canonical value, and canonical output
//! re-parses to itself.

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
    /// never ends in the sentence punctuation that parsing strips.
    fn canonical_doi()(
        registrant in "[0-9]{4,9}(\\.[0-9]{1,3})?",
        suffix in "[a-z0-9._;()/:<>-]{0,29}[a-z0-9()/<>-]",
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
    fn bare_digits_never_guess_a_kind(n in 1u64..u64::MAX) {
        prop_assert!(
            matches!(n.to_string().parse::<WorkId>(), Err(IdError::Ambiguous { .. })),
            "expected Ambiguous error"
        );
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
            IdKind::OpenAlex => format!("W{n}"),
            IdKind::S2Paper => format!("{n:040x}"),
            _ => n.to_string(),
        };
        let id = WorkId::parse_as(kind, &input).unwrap();
        prop_assert_eq!(id.kind(), kind);
        assert_round_trips(&id)?;
    }
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
