//! A book's first edition, as its citation states it.

use super::*;
use crate::work::WorkType;

/// An Open Library work: first published `first`, with editions in
/// `later` too.
fn book(title: &str, authors: &[&str], first: i16, later: &[i16]) -> SourceRecord {
    let mut r = record(&[], title, authors, first);
    r.work.work_type = Some(WorkType::Book);
    r.work.other_years = later.to_vec();
    r
}

#[test]
fn a_later_editions_year_given_as_the_originals_is_plausible() {
    // holdout-6 #243, #275, #155, #974: the year given as the original's,
    // or as the first edition's, is a later edition's.
    let cases = [
        (
            "Firth, J. R. (1969). *Papers in linguistics, 1934-1951*. Oxford University \
             Press. (Original work published 1969)",
            book(
                "Papers in linguistics, 1934-1951",
                &["Firth, J. R."],
                1957,
                &[1961, 1964, 1969],
            ),
        ),
        (
            "Porter, N. (1969). *The American colleges and the American public*. Arno Press. \
             (Original work published 1890)",
            book(
                "The American colleges and the American public",
                &["Porter, Noah"],
                1870,
                &[1878, 1890, 1969],
            ),
        ),
        (
            "Wright, W. K. (1945). *General introduction to ethics* (1st ed.). Macmillan.",
            book(
                "General introduction to ethics",
                &["William Kelley Wright"],
                1929,
                &[1935, 1937, 1945],
            ),
        ),
        (
            "Ruggiers, P. G. (1987). *Versions of medieval comedy* (1st ed.). University of \
             Oklahoma Press.",
            book(
                "Versions of medieval comedy",
                &["Paul G. Ruggiers"],
                1977,
                &[1978, 1985, 1987],
            ),
        ),
    ];
    for (text, record) in cases {
        let a = assess(&Claim::from_text(text), &record);
        assert!(
            (PLAUSIBLE..VERIFIED).contains(&a.confidence),
            "{text}: {a:?}"
        );
        assert_eq!(a.year.agreement, Agreement::Mismatch, "{text}");
    }
}

#[test]
fn a_reprint_cited_with_its_originals_year_is_the_work() {
    // holdout-6 #751, #882, #997 (real reprints), and a year off by one
    // (a printing's).
    let cases = [
        (
            "Boas, F. S. (1978). *Aspects of classical legend and history in Shakespeare*. \
             R. West. (Original work published 1943)",
            book(
                "Aspects of classical legend and history in Shakespeare",
                &["Frederick S. Boas"],
                1943,
                &[1970, 1978],
            ),
        ),
        (
            "Luneburg, R. K. (2021). *Mathematical theory of optics*. University of \
             California Press. (Original work published 1944)",
            book(
                "Mathematical theory of optics",
                &["R. K. Luneburg"],
                1944,
                &[1964, 2021],
            ),
        ),
        (
            "Kuhn, T. S. (1962). *The structure of scientific revolutions* (1st ed.). \
             University of Chicago Press.",
            book(
                "The structure of scientific revolutions",
                &["Thomas S. Kuhn"],
                1962,
                &[1970, 1996],
            ),
        ),
        (
            "Kuhn, T. S. (1963). *The structure of scientific revolutions* (1st ed.). \
             University of Chicago Press.",
            book(
                "The structure of scientific revolutions",
                &["Thomas S. Kuhn"],
                1962,
                &[1970, 1996],
            ),
        ),
        // wild-2 #519: a first year a catalogue has three years off
        // (Open Library's 1935 for 1938).
        (
            "James, C. L. R. 1989. The Black Jacobins: Toussaint L'Ouverture and the San \
             Domingo Revolution. 2nd ed., rev. New York: Vintage Books. First published 1938.",
            book(
                "The Black Jacobins",
                &["C. L. R. James"],
                1935,
                &[1938, 1963, 1989],
            ),
        ),
        // A later edition cited as one: nothing said of the first.
        (
            "Kuhn, T. S. (1970). *The structure of scientific revolutions* (2nd ed.). \
             University of Chicago Press.",
            book(
                "The structure of scientific revolutions",
                &["Thomas S. Kuhn"],
                1962,
                &[1970, 1996],
            ),
        ),
    ];
    for (text, record) in cases {
        let a = assess(&Claim::from_text(text), &record);
        assert!(a.confidence >= VERIFIED, "{text}: {a:?}");
    }
}

#[test]
fn a_first_edition_in_the_title_is_no_claim() {
    let record = book(
        "Handbook of Ethics: First Edition",
        &["William Kelley Wright"],
        1929,
        &[1945],
    );
    let text = "Wright, W. K. (1945). Handbook of Ethics: First Edition. Macmillan.";
    let a = assess(&Claim::from_text(text), &record);
    assert!(a.confidence >= VERIFIED, "{a:?}");
}

#[test]
fn the_first_year_a_reference_states() {
    let first = |text: &str| {
        let t = Normalised::new(text);
        let cited = t.years_beside(None);
        super::edition::claimed_first_year(&t, None, &cited)
    };
    assert_eq!(
        first("Smith (1970). A book. X. (Original work published 1890)"),
        Some(1890)
    );
    assert_eq!(
        first("Smith (1970). A book. X. Originally published in 1890."),
        Some(1890)
    );
    assert_eq!(
        first("Smith (1970). A book. First published 1890 by Y."),
        Some(1890)
    );
    assert_eq!(
        first("Smith (1945). A book (1st ed.). Macmillan."),
        Some(1945)
    );
    assert_eq!(
        first("Smith, A book, first edition, Macmillan, 1945."),
        Some(1945)
    );
    assert_eq!(first("Smith (1945). A book (2nd ed.). Macmillan."), None);
    assert_eq!(
        first("Smith (1945). First published works. Macmillan."),
        None
    );
}
