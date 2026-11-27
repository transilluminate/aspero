//! Red-team wild-6: flawed citations that matched the work meant, one
//! field wrong — a title reworded beside the work's place or DOI, its
//! opening words dropped, another journal, another co-author.

use super::*;

fn plausible(a: &Assessment) -> bool {
    (PLAUSIBLE..VERIFIED).contains(&a.confidence)
}

/// wild-6 #14: the title reworded around a "vs.", beside the work's DOI
/// and place: plausible, the title to check.
#[test]
fn a_title_reworded_around_vs_beside_its_doi_is_plausible() {
    let r = article(
        "Comparison of Pseudobinaural Hearing to Real Binaural Hearing Rehabilitation After \
         Cochlear Implantation in Patients With Unilateral Deafness and Tinnitus",
        &["Susan Arndt", "Antje Aschendorff", "Roland Laszig"],
        2011,
        ("Otology & Neurotology", "32", "39-47"),
    );
    let mut r = r;
    r.ids = vec!["doi:10.1097/mao.0b013e3181fcf271".parse().unwrap()];
    let claim = Claim::from_text(
        "Arndt S, et al. (2011). Pseudobinaural vs. real binaural rehabilitation after cochlear \
         implantation in unilateral deafness with tinnitus. Otology & Neurotology 32(1):39–47. \
         DOI: 10.1097/MAO.0b013e3181fcf271",
    );
    let a = assess(&claim, &r);
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!(plausible(&a), "{a:?}");
}

/// wild-6 #130: words of the title dropped in one place ("the risks of
/// low birthweight and preterm birth") and others put in another ("birth
/// outcomes"), at the work's place: reworded, plausible.
#[test]
fn a_title_reworded_in_two_places_at_its_place_is_plausible() {
    let r = article(
        "Active and passive maternal smoking during pregnancy and the risks of low birthweight \
         and preterm birth: the Generation R Study",
        &[
            "Vincent W. V. Jaddoe",
            "Ernst-Jan W. M. Troe",
            "Albert Hofman",
        ],
        2008,
        ("Paediatric and Perinatal Epidemiology", "22", "162-171"),
    );
    let claim = Claim::from_text(
        "JADDOE, V. W. V., et al. Active and passive maternal smoking during pregnancy and birth \
         outcomes: the Generation R Study. Paediatric and Perinatal Epidemiology. 2008, 22(2), \
         162–171.",
    );
    let a = assess(&claim, &r);
    assert_eq!(a.title.agreement, Agreement::Partial, "{a:?}");
    assert!(plausible(&a), "{a:?}");
}

/// wild-6 #653: a quoted title with words of its own before the record's
/// ("The effect of …") and one of the record's dropped ("professions"):
/// reworded, plausible.
#[test]
fn a_quoted_title_reworded_is_plausible() {
    let r = article(
        "Deliberate Practice and Performance in Music, Games, Sports, Education, and \
         Professions: A Meta-Analysis",
        &[
            "Brooke N. Macnamara",
            "David Z. Hambrick",
            "Frederick L. Oswald",
        ],
        2014,
        ("Psychological Science", "25", "1608-1618"),
    );
    let claim = Claim::from_text(
        "Macnamara, B. N., Hambrick, D. Z., & Oswald, F. L. (2014). \"The effect of deliberate \
         practice on performance in music, games, sports, and education: A meta-analysis.\" \
         Psychological Science, 25(8), 1608–1618.",
    );
    let a = assess(&claim, &r);
    assert!(plausible(&a), "{a:?}");
}

/// Not reworded: a title misspelt, a leading article dropped, a subtitle
/// dropped, a word's form changed, the record's furniture before it.
#[test]
fn a_title_written_as_people_do_is_no_rewording() {
    for (text, title) in [
        (
            "Swire-Thompson B, et al. (2022). Backfire effects after correcting misinformation \
             are strongly associated with reliability. J Exp Psychol Gen, 151(7), 1655–1665.",
            "The backfire effect after correcting misinformation is strongly associated with \
             reliability.",
        ),
        (
            "Partridge JS, Harari D. Frailty in older surgical patients: a review. Age Ageing. \
             2012;41(2):142-147.",
            "Frailty in the older surgical patient: a review",
        ),
        (
            "Smith J. \"Characteristics of useful code reviews.\" MSR 2015;12:146-156.",
            "Characteristics of useful code reviews: an empirical study at Microsoft",
        ),
        (
            "Garry RC. \"The effciiency of bicycle pedalling in the trained subject.\" J Physiol \
             1934;82:200.",
            "The efficiency of bicycle pedalling in the trained subject",
        ),
        (
            "Koiso H. (2024) 'Thermal expansion and phase stab- ility of BF3 (B = Sc, Y, La, Al, \
             Ga, In) from first principles', Physical Review B, 110(6), p. 064104.",
            "Thermal expansion and phase stability of B F 3 ( B = Sc , Y, La, Al, Ga, In) from \
             first principles",
        ),
        (
            "Hedges LV. \"Does money matter? A meta-analysis of studies of the effects of \
             differential school inputs on student outcomes.\" Educ Res 1994;23:5-14.",
            "An Exchange: Part I : Does Money Matter? A Meta-Analysis of Studies of the Effects \
             of Differential School Inputs on Student Outcomes",
        ),
    ] {
        assert!(!Normalised::new(text).replaces_in(title), "{text}");
        let quoted = crate::matching::titles::given_title(text);
        assert!(
            !quoted.is_some_and(|q| crate::matching::titles::replaces(&q, title)),
            "{text}"
        );
    }
}
