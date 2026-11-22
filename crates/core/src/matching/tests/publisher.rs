//! A book's publisher: the one its citation names, and its editions'.

use super::*;

fn book(title: &str) -> SourceRecord {
    let mut r = record(&[], title, &["Otto Jespersen"], 2013);
    r.work.work_type = Some(crate::work::WorkType::Book);
    r
}

#[test]
fn the_publisher_a_citation_names() {
    let cited = |text: &str, title: &str| cited_publisher(&Claim::from_text(text), &book(title));
    // holdout-6 #94, #32: the last part, after the title's own parts.
    assert_eq!(
        cited(
            "Jespersen, O. (2013). *A Modern English Grammar on Historical Principles: Volume 4. \
             Syntax (third volume)*. Palgrave Macmillan.",
            "A Modern English Grammar on Historical Principles"
        )
        .as_deref(),
        Some("Palgrave Macmillan")
    );
    assert_eq!(
        cited(
            "Cronin, Richard. 1988. Colour and Experience in Nineteenth-Century Poetry. Routledge.",
            "Colour and Experience in Nineteenth-Century Poetry"
        )
        .as_deref(),
        Some("Routledge")
    );
    // By its words, or between a place and a year, whatever follows.
    assert_eq!(
        cited(
            "Kuhn TS. The Structure of Scientific Revolutions. Chicago: University of Chicago \
             Press; 1962.",
            "The Structure of Scientific Revolutions"
        )
        .as_deref(),
        Some("University of Chicago Press")
    );
    assert_eq!(
        cited(
            "Smith J. Grammar. London: Routledge, 2013. Reprinted 2015.",
            "Grammar"
        )
        .as_deref(),
        Some("Routledge")
    );
    // Not an edition, a note on the original, an identifier.
    assert_eq!(
        cited(
            "Firth, J. R. (1969). *Papers in linguistics*. Oxford University Press. (Original \
             work published 1957)",
            "Papers in linguistics"
        )
        .as_deref(),
        Some("Oxford University Press")
    );
    assert_eq!(
        cited(
            "Smith, J. (2010). Grammar (2nd ed.). https://doi.org/10.1/x",
            "Grammar"
        ),
        None
    );
    // Structured: BibTeX's, RIS's, a template's.
    assert_eq!(
        cited(
            "@book{x, title={Grammar}, author={Smith, J}, publisher={Springer}, year={2010}}",
            "Grammar"
        )
        .as_deref(),
        Some("Springer")
    );
    assert_eq!(
        cited(
            "TY  - BOOK\nTI  - Grammar\nPB  - Prentice Hall\nPY  - 1977\nER  -",
            "Grammar"
        )
        .as_deref(),
        Some("Prentice Hall")
    );
    assert_eq!(
        cited(
            "{{cite book |last1=Smith |title=Grammar |publisher=Macmillan |year=1936}}",
            "Grammar"
        )
        .as_deref(),
        Some("Macmillan")
    );
    // Where the title can't be placed, none.
    assert_eq!(cited("Smith (2010). Syntax. Routledge.", "Grammar"), None);
}

#[test]
fn publishers_are_one_by_their_own_words_initials_or_group() {
    for (a, b) in [
        ("Penguin Books", "Penguin"),
        ("Palgrave Macmillan UK", "Palgrave Macmillan"),
        ("OUP", "Oxford University Press"),
        ("Clarendon Press", "Oxford University Press"),
        ("CRC Press", "Routledge"),
        ("Taylor & Francis Group", "Routledge"),
        ("Lawrence Erlbaum Associates", "Psychology Press"),
        ("Wiley-Blackwell", "John Wiley & Sons"),
        ("Jossey-Bass", "Wiley"),
        ("Academic Press", "Elsevier"),
        ("Herder and Herder", "Seabury Press"),
        ("University Press", "Routledge"),
    ] {
        assert!(same_publisher(a, b), "{a} / {b}");
        assert!(same_publisher(b, a), "{b} / {a}");
    }
    for (a, b) in [
        ("Routledge", "Palgrave Macmillan UK"),
        ("Palgrave Macmillan", "Taylor & Francis Group"),
        ("Elsevier", "Springer International Publishing"),
        ("Wiley", "CRC Press"),
        ("Brill", "Peter Lang UK"),
    ] {
        assert!(!same_publisher(a, b), "{a} / {b}");
    }
}

/// The year cited.
const Y: &[i16] = &[2013];

#[test]
fn enough_editions_none_naming_the_publisher_tell() {
    let edition = |publisher: &str| Work {
        publisher: Some(publisher.into()),
        issued: Some(PartialDate::from_year(2013)),
        ..Work::default()
    };
    let record = Work::default();
    let many = [
        edition("Routledge"),
        edition("Taylor & Francis Group"),
        edition("Taylor & Francis Group"),
    ];
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, Y, &record, &many),
        PublisherCheck::Unnamed("Taylor & Francis Group".into())
    );
    assert_eq!(
        check_publisher("Psychology Press", None, Y, &record, &many),
        PublisherCheck::Named
    );
    // A name they know anywhere in the citation: read there, not where
    // the reading took another part.
    assert_eq!(
        check_publisher(
            "Abingdon",
            Some("Yapko (2013). Treating Depression. Routledge, Abingdon."),
            Y,
            &record,
            &many
        ),
        PublisherCheck::Named
    );
    // An edition's two publishers, either named.
    assert_eq!(
        check_publisher(
            "Munksgaard",
            None,
            Y,
            &record,
            &[edition("G. Allen & Unwin; E. Munksgaard")]
        ),
        PublisherCheck::Named
    );
    // Two editions are too few, but for the record naming another too.
    let two = &many[..2];
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, Y, &record, two),
        PublisherCheck::TooFew
    );
    let openalex = Work {
        container_title: Some("Routledge eBooks".into()),
        ..Work::default()
    };
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, Y, &openalex, &two[..1]),
        PublisherCheck::Unnamed("Routledge".into())
    );
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, Y, &openalex, &[]),
        PublisherCheck::TooFew
    );
    // None of the year cited: the catalogue lacks the edition cited
    // (holdout-2 #369, Freire's 1970 Herder and Herder).
    assert_eq!(
        check_publisher("Herder and Herder", None, &[1970], &record, &many),
        PublisherCheck::TooFew
    );
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, &[2014], &record, &many),
        PublisherCheck::Unnamed("Taylor & Francis Group".into())
    );
}

#[test]
fn initials_name_a_publisher_written_out() {
    for (short, name) in [
        ("LSU Press", "Louisiana State University Press"),
        ("N.L.B.", "New Left Books"),
        ("OUP", "Oxford University Press"),
    ] {
        assert!(same_publisher(short, name), "{short}");
        assert!(same_publisher(name, short), "{short}");
    }
}

#[test]
fn a_subtitle_an_author_or_an_edition_is_no_publisher() {
    let mut record = book("Riot and Remembrance");
    record.work.authors = vec![crate::work::Person::literal("James S. Hirsch")];
    let cited =
        |text: &str, record: &SourceRecord| cited_publisher(&Claim::from_text(text), record);
    // wild-4 #646: a subtitle the record lacks, and no publisher.
    assert_eq!(
        cited(
            "James S. Hirsch, Riot and Remembrance: The Tulsa Race War and Its Legacy (2002).",
            &record
        ),
        None
    );
    // wild-3 #948: a subtitle's last part, before the year, is the
    // subtitle's; after its end, a publisher is read (wild-3 #1017).
    let record = book("The Labor of Development");
    assert_eq!(
        cited(
            "Heller, P. The Labor of Development: Workers and the Transformation of Capitalism \
             in Kerala, India (1999).",
            &record
        ),
        None
    );
    let record = book("Signs and Cities");
    assert_eq!(
        cited(
            "Dubey, Madhu. Signs and Cities: Black Literary Postmodernism. Indiana University \
             Press, 2003.",
            &record
        )
        .as_deref(),
        Some("Indiana University Press")
    );
    // wild-4 #327: authors and an edition in a table's cells.
    let mut record = book("Medical Pharmacology and Therapeutics");
    record.work.authors = vec![
        crate::work::Person::literal("Derek G. Waller"),
        crate::work::Person::literal("Anthony P. Sampson"),
    ];
    assert_eq!(
        cited(
            "Medical Pharmacology and Therapeutics (Waller, Sampson) | Waller, Sampson | 5th \
             (2018) | not sure",
            &record
        ),
        None
    );
}

#[test]
fn only_publishers_of_known_groups_differ() {
    for known in [
        "Routledge",
        "Palgrave Macmillan UK",
        "Taylor & Francis Group",
        "Elsevier",
        "Springer International Publishing AG",
        "Prentice-Hall",
    ] {
        assert!(known_publisher(known), "{known}");
    }
    // Small presses a group bought or reissues, university presses, words
    // read from elsewhere.
    for unknown in [
        "Acumen",
        "UCL Press",
        "Roberts & Company",
        "Indiana University Press",
        "Verify",
    ] {
        assert!(!known_publisher(unknown), "{unknown}");
    }
    let edition = |publisher: &str, year: i16| Work {
        publisher: Some(publisher.into()),
        issued: Some(PartialDate::from_year(year)),
        ..Work::default()
    };
    // wild-3 #505: UCL Press's book, Routledge's reissue, and Rutgers's
    // US edition.
    let routledge = Work {
        publisher: Some("Routledge".into()),
        ..Work::default()
    };
    let editions = [
        edition("Taylor & Francis Group Plc", 2004),
        edition("Taylor & Francis Inc", 2004),
        edition("Rutgers University Press", 1999),
    ];
    assert_eq!(
        check_publisher("UCL Press", None, &[1999], &routledge, &editions),
        PublisherCheck::Unknown
    );
    // A known group cited against the record's: as before.
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, &[1999], &routledge, &editions),
        PublisherCheck::Unnamed("Taylor & Francis Group Plc".into())
    );
    // wild-2 #873: a record of no known group (OpenAlex's McGill-Queen's,
    // Acumen's co-publisher), against a known group cited.
    let mqup = Work {
        container_title: Some("McGill-Queen's University Press eBooks".into()),
        ..Work::default()
    };
    let editions = [
        edition("McGill-Queen's University Press", 2002),
        edition("Taylor & Francis Group", 2014),
        edition("Taylor & Francis Group", 2014),
        edition("Taylor & Francis Group", 2002),
    ];
    assert_eq!(
        check_publisher("Palgrave Macmillan", None, &[2002], &mqup, &editions),
        PublisherCheck::Unknown
    );
    // Editions mostly of no known group, the record naming none.
    let editions = [
        edition("University of Chicago Press", 2003),
        edition("University of Chicago Press", 2007),
        edition("University of Chicago Press", 2010),
    ];
    assert_eq!(
        check_publisher("Routledge", None, &[2003], &Work::default(), &editions),
        PublisherCheck::Unknown
    );
}
