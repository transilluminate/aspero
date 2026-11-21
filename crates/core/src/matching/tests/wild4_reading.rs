//! wild-4's model-written references: what reading a claim takes from
//! them (bare ISBNs, NBER numbers, the probable title, list furniture).

use super::*;
use crate::id::IdKind;

fn isbns(text: &str) -> Vec<String> {
    Claim::from_text(text)
        .ids
        .into_iter()
        .filter(|id| id.kind() == IdKind::Isbn)
        .map(|id| id.to_string())
        .collect()
}

#[test]
fn a_bare_isbn_13_is_read_as_one() {
    for (text, want) in [
        // wild-4 #321: a textbook as a table row, its ISBN a cell.
        (
            "Katzung's Basic & Clinical Pharmacology | Katzung, Vanderah | 15th (2020) | \
             978-1260452310",
            &["isbn:9781260452310"][..],
        ),
        // With a note, unhyphenated, or after a label it doesn't touch.
        (
            "Nancy Caroline's Emergency Care in the Streets | 8th (2018) | 978-1284104882 \
             (8th, verify)",
            &["isbn:9781284104882"],
        ),
        (
            "Atkinson AJ. Principles of Clinical Pharmacology. 3rd ed. 2012. 9780123854711.",
            &["isbn:9780123854711"],
        ),
        (
            "Dover reprint, 2005. ISBN-13 (Dover): 978-0486445687.",
            &["isbn:9780486445687"],
        ),
        // Two bindings.
        (
            "Cambridge University Press | 978-0-521-79540-1 (pbk); 978-0-521-79160-1 (hbk)",
            &["isbn:9780521795401", "isbn:9780521791601"],
        ),
    ] {
        assert_eq!(isbns(text), want, "{text}");
    }
}

#[test]
fn other_numbers_are_no_bare_isbn() {
    for text in [
        // A check digit that's wrong (an invented number).
        "Some Book | Author | 2nd (2010) | 978-3-540-58660-2",
        // Not a book prefix, an ISBN-10, too few or too many digits.
        "Some Book | Author | 2010 | 123-4567890123",
        "Some Book | Author | 2010 | 0-262-03561-8",
        "Call 978-555-0123 for a copy.",
        "Some Book | Author | 2010 | 97802620356130",
        // Inside a DOI or link.
        "Smith J. A Book. Cambridge; 2014. doi:10.1017/cbo9781107049963",
        // Run into letters.
        "Some Book. 2010. ref9780262035613",
    ] {
        assert_eq!(isbns(text), Vec::<String>::new(), "{text}");
    }
    // Labelled, read as before, once.
    assert_eq!(
        isbns("A Book. 2016. ISBN 978-0-262-03561-3 | 978-0262035613"),
        ["isbn:9780262035613"]
    );
}

fn dois(text: &str) -> Vec<String> {
    Claim::from_text(text)
        .ids
        .into_iter()
        .filter(|id| id.kind() == IdKind::Doi)
        .map(|id| id.to_string())
        .collect()
}

#[test]
fn an_nber_working_papers_number_is_its_doi() {
    for (text, want) in [
        // wild-4 #723: the number is another paper's, so its lookup says so.
        (
            "Korinek, A. (2023). \"Generative AI for Economic Research: Use Cases and \
             Implications for Economists.\" NBER Working Paper w31113. A survey.",
            "doi:10.3386/w31113",
        ),
        (
            "Bloom, N. (2019) 'The impact of Brexit on UK firms', NBER Working Paper No. 26218. \
             Cambridge, MA: National Bureau of Economic Research.",
            "doi:10.3386/w26218",
        ),
        (
            "Sala-i-Martin, X. (2003). \"Addressing the Natural Resource Curse.\" NBER WP 9804.",
            "doi:10.3386/w9804",
        ),
        (
            "Narayanan, V. G. (1999). ABC at Insteel Industries (Working Paper Series No. 727). \
             National Bureau of Economic Research.",
            "doi:10.3386/w0727",
        ),
    ] {
        assert_eq!(dois(text), [want], "{text}");
    }
    for text in [
        // Another series, or NBER's journal.
        "Auer, R. (2020). Rise of the central bank digital currencies (BIS Working Paper No. \
         880). Bank for International Settlements.",
        "Posen, A. (1995). \"Declarations Are Not Enough.\" NBER Macroeconomics Annual, 10, \
         253–274.",
        "Rendova, M. (2023). Household energy arrears (Working Paper No. 23-07). Centre for \
         Energy Affordability Studies.",
    ] {
        assert_eq!(dois(text), Vec::<String>::new(), "{text}");
    }
    // A DOI given names the version cited.
    assert_eq!(
        dois(
            "Korinek, A. (2023). Generative AI for Economic Research. Journal of Economic \
             Literature 61(4). doi:10.1257/jel.20231736. Also NBER Working Paper w30957."
        ),
        ["doi:10.1257/jel.20231736"]
    );
}

#[test]
fn a_journals_article_cited_as_a_working_paper_is_another_version() {
    // wild-4 #723, its number left out: the journal's article is matched,
    // the working paper cited.
    let mut article = record(
        &["doi:10.1257/jel.20231736"],
        "Generative AI for Economic Research: Use Cases and Implications for Economists",
        &["Anton Korinek"],
        2023,
    );
    article.work.work_type = Some(crate::work::WorkType::JournalArticle);
    article.work.container_title = Some("Journal of Economic Literature".into());
    let title =
        "\"Generative AI for Economic Research: Use Cases and Implications for Economists.\"";
    for series in [
        "NBER Working Paper.",
        "NBER WP.",
        "Brookings working papers.",
    ] {
        let text = format!("Korinek, A. (2023). {title} {series}");
        let a = assess(&Claim::from_text(&text), &article);
        assert_eq!(a.container.agreement, Agreement::Mismatch, "{text}");
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
    }
    // Cited in its journal: verified, as before.
    let text = format!("Korinek, A. (2023). {title} Journal of Economic Literature.");
    let a = assess(&Claim::from_text(&text), &article);
    assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
}
