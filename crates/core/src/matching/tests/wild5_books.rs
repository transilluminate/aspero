//! Red-team wild-5: books not found because the title searched for was
//! a publisher's name, a note's or the whole reference, and a university
//! press read as a byline.

use super::*;

fn assert_titles(cases: &[(&str, &str)]) {
    for (text, title) in cases {
        assert_eq!(probable_title(text).as_deref(), Some(*title), "{text}");
    }
}

#[test]
fn a_publishers_name_or_an_identifier_is_no_title() {
    assert_titles(&[
        // wild-5 #525: a short title beside a longer publisher's name.
        (
            "Christensen, C. M. (1997). The Innovator's Dilemma. Harvard Business School Press.",
            "The Innovator's Dilemma",
        ),
        // wild-3 #653, #647: a publisher with its year, an ISBN.
        (
            "Wald, R. M. General Relativity. University of Chicago Press, 1984. ISBN-13: \
             978-0226870335 (paperback).",
            "General Relativity",
        ),
        (
            "Srednicki, M. Quantum Field Theory. Cambridge University Press, 2007. ISBN-13: \
             978-0521864497 (ISBN-10: 0521864496).",
            "Quantum Field Theory",
        ),
        // holdout-6 #9: a chapter's title beside its book's publisher.
        (
            "Palkamsetti, S., & Khandhar, P. B. (2026). Laryngotracheobronchitis. In \
             *StatPearls*. StatPearls Publishing.",
            "Laryngotracheobronchitis",
        ),
        // holdout-4 #204: a DOI after the journal.
        (
            "Thakur R, Al Hadidi S. From Physician-Driven to Patient-Centered: Transforming \
             Mulitple Myeloma Care Decisions. JCO Oncology Practice. 2026;22. doi: \
             10.1200/op-25-00712",
            "From Physician-Driven to Patient-Centered: Transforming Mulitple Myeloma Care \
             Decisions",
        ),
    ]);
    // A citation template's title holds its identifiers too: it stays a
    // candidate (holdout-2 #374).
    let template = "{{cite book |last1=Ethier |first1=Stewart N. |last2=Kurtz |first2=Thomas \
                    G. |title=Markov Processes: Characterization and Convergence \
                    |publisher=Wiley |year=1986 |doi=10.1002/9780470316658 \
                    |isbn=9780471081869}}";
    assert!(
        probable_title(template).is_some_and(|t| t.contains("Markov Processes")),
        "{template}"
    );
}

#[test]
fn a_bylines_initials_and_given_name_are_no_title() {
    // wild-5 #638: "Connor, W. Robert." split at its full stops.
    assert_titles(&[(
        "Connor, W. Robert. Thucydides. Princeton University Press, 1984.",
        "Thucydides",
    )]);
}

#[test]
fn a_title_leaves_out_its_publisher_volumes_and_role_notes() {
    assert_titles(&[
        // wild-5 #848: the publisher after a comma.
        (
            "Kołodko, G. W. (2011), Truth, Errors and Lies: Politics and Economics in a \
             Volatile World, Columbia UP.",
            "Truth, Errors and Lies: Politics and Economics in a Volatile World",
        ),
        // wild-5 #478: a count of volumes.
        (
            "Bristol, Africa and the Eighteenth-Century Slave Trade to America, 4 vols. \
             (Bristol Record Society, 1986-96).",
            "Bristol, Africa and the Eighteenth-Century Slave Trade to America",
        ),
        // wild-5 #601, #664, wild-4 #582: an editor's or translator's note
        // in brackets, whose full stops split the title's sentence.
        (
            "Krings, H. P. (2001). Repairing Texts: Empirical Investigations of Machine \
             Translation Post-Editing Processes (ed. G. S. Koby). Kent State UP.",
            "Repairing Texts: Empirical Investigations of Machine Translation Post-Editing \
             Processes",
        ),
        (
            "Beethoven: The Philosophy of Music (ed. Tiedemann, trans. Jephcott, 1998)",
            "Beethoven: The Philosophy of Music",
        ),
        (
            "Chaudenson, Robert (2001). Creolization of Language and Culture (ed. Salikoko \
             Mufwene). London: Routledge.",
            "Creolization of Language and Culture",
        ),
    ]);
}

#[test]
fn a_listed_references_title_is_between_its_byline_and_publisher() {
    // wild-5 #789-#806: "Name, Title, Publisher, City Year".
    assert_titles(&[
        (
            "Cesare Mori, Con la mafia ai ferri corti, Mondadori, 1932",
            "Con la mafia ai ferri corti",
        ),
        (
            "Pino Arlacchi, La mafia imprenditrice, il Mulino, Bologna 1983",
            "La mafia imprenditrice",
        ),
        (
            "Rocco Sciarrone, Mafie vecchie, mafie nuove, Donzelli, 1998 (edizione aggiornata \
             2009)",
            "Mafie vecchie, mafie nuove",
        ),
        (
            "Giovanni Falcone e Marcelle Padovani, Cose di Cosa Nostra, Rizzoli, Milano 1991",
            "Cose di Cosa Nostra",
        ),
        (
            "Eric J. Hobsbawm, Primitive Rebels, Manchester University Press, 1959 (ed. it. I \
             ribelli, Einaudi, 1966)",
            "Primitive Rebels",
        ),
        (
            "Diego Gambetta, La mafia siciliana. Il business della protezione privata, \
             Einaudi, Torino 1992",
            "La mafia siciliana. Il business della protezione privata",
        ),
        // Without a byline: the title before the publisher.
        (
            "La mafia. Centosessant'anni di storia, Donzelli, 2018",
            "La mafia. Centosessant'anni di storia",
        ),
        // Chicago's state and publisher are no listed reference.
        (
            "Herrin, Judith. The Formation of Christendom. Princeton, NJ: Princeton University \
             Press, 1987.",
            "The Formation of Christendom",
        ),
    ]);
    // A title first, then names: neither part is told for the title.
    assert_ne!(
        probable_title("Machine Learning, Tom Mitchell, McGraw Hill, 1997").as_deref(),
        Some("Tom Mitchell")
    );
}

/// wild-5 #651: "Oxford UP" is a publisher, not a byline naming someone
/// the book's record lacks.
#[test]
fn a_university_press_is_no_byline() {
    let mut r = record(
        &[],
        "Shakespeare, race, and colonialism",
        &["Ania Loomba"],
        2002,
    );
    r.work.work_type = Some(crate::work::WorkType::Book);
    for text in [
        "---. Shakespeare, Race, and Colonialism. Oxford UP, 2002.",
        "Shakespeare, Race, and Colonialism. Oxford UP, 2002.",
    ] {
        let a = assess(&Claim::from_text(text), &r);
        assert_eq!(a.authors.agreement, Agreement::Unknown, "{text}: {a:?}");
        assert!(a.confidence >= PLAUSIBLE, "{text}: {a:?}");
    }
    // Someone else's name beside it still is.
    let a = assess(
        &Claim::from_text("Smith, J. Shakespeare, Race, and Colonialism. Oxford UP, 2002."),
        &r,
    );
    assert_eq!(a.authors.agreement, Agreement::Mismatch, "{a:?}");
}
