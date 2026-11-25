//! Red-team wild-5: books not found because the title searched for was
//! a publisher's name, a note's or the whole reference, and a university
//! press read as a byline.

use super::*;

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
