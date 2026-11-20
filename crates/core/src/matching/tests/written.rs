//! Free text's title and authors as written (#169): where the work found
//! has its title in the text, and from the layout where nothing was found.

use super::super::written::names;
use super::*;

const SEYMOUR: &str = "Seymour CW, Liu VX, Iwashyna TJ, et al. Assessment of clinical criteria \
     for sepsis. JAMA. 2016;315(8):762-774. doi:10.1001/jama.2016.0287";

fn titled(title: &str) -> Work {
    Work {
        title: Some(title.into()),
        ..Work::default()
    }
}

fn read(text: &str, work: Option<&Work>) -> (Option<String>, Vec<String>) {
    let read = Claim::from_text(text).as_read_beside(work);
    (read.title, read.authors)
}

fn pair(title: &str, authors: &[&str]) -> (Option<String>, Vec<String>) {
    (
        Some(title.to_owned()),
        authors.iter().map(|a| (*a).to_owned()).collect(),
    )
}

#[test]
fn a_found_works_title_and_the_byline_before_it_are_read_as_written() {
    let work = titled("Assessment of Clinical Criteria for Sepsis");
    assert_eq!(
        read(SEYMOUR, Some(&work)),
        pair(
            "Assessment of clinical criteria for sepsis",
            &["Seymour CW", "Liu VX", "Iwashyna TJ"]
        )
    );
    let apa = "Seymour, C. W., Liu, V. X., & Iwashyna, T. J. (2016). Assessment of clinical \
               criteria for sepsis. JAMA, 315(8), 762–774.";
    assert_eq!(
        read(apa, Some(&work)),
        pair(
            "Assessment of clinical criteria for sepsis",
            &["Seymour, C. W.", "Liu, V. X.", "Iwashyna, T. J."]
        )
    );
}

#[test]
fn with_nothing_found_the_layout_tells_the_title_and_authors() {
    // The issue's example: Vancouver, an invented work.
    let vancouver = "Hartwell J, Okafor N. Microplastic shedding from imaginary kettles. \
                     J Made Up Sci. 2024;1:1-9.";
    let kettles = pair(
        "Microplastic shedding from imaginary kettles",
        &["Hartwell J", "Okafor N"],
    );
    assert_eq!(read(vancouver, None), kettles);
    let apa = "Hartwell, J., & Okafor, N. (2024). Microplastic shedding from imaginary \
               kettles. Journal of Made Up Science, 1(1), 1–9.";
    assert_eq!(
        read(apa, None),
        pair(
            "Microplastic shedding from imaginary kettles",
            &["Hartwell, J.", "Okafor, N."]
        )
    );
    let harvard = "Hartwell, J. and Okafor, N. (2024) 'Microplastic shedding from imaginary \
                   kettles', Journal of Made Up Science, 1(1), pp. 1-9.";
    assert_eq!(
        read(harvard, None),
        pair(
            "Microplastic shedding from imaginary kettles",
            &["Hartwell, J.", "Okafor, N."]
        )
    );
    assert_eq!(
        read(SEYMOUR, None),
        pair(
            "Assessment of clinical criteria for sepsis",
            &["Seymour CW", "Liu VX", "Iwashyna TJ"]
        )
    );
}

#[test]
fn a_layout_that_cant_be_told_reads_no_title() {
    for text in [
        // No byline to start from.
        "Microplastic shedding from imaginary kettles J Made Up Sci 2024 1 1",
        // A byline, but nothing placing the sentence after it as a title.
        "Hartwell J, Okafor N. Microplastic shedding from imaginary kettles.",
        // An organisation's byline.
        "World Health Organization. Global tuberculosis report 2023. Geneva: WHO; 2023.",
        // Names going on past where the byline seemed to end.
        "Lionel P. Robert. 2013. A multi-level analysis of the impact of shared leadership \
         in diverse virtual teams. In Proceedings of the 2013 conference, 1-12.",
        "Stephanie E. Smallwood, Saltwater Slavery: A Middle Passage from Africa to \
         American Diaspora. Cambridge, MA: Harvard University Press, 2007.",
    ] {
        assert_eq!(read(text, None).0, None, "{text}");
    }
}

#[test]
fn a_title_runs_on_through_its_subtitle_not_into_the_journal() {
    for (text, title) in [
        (
            "Friese, M., Loschelder, D. D., Gieseler, K., Frankenbach, J., & Inzlicht, M. \
             (2019). Is ego depletion real? An analysis of arguments. Personality and Social \
             Psychology Review, 23(2), 107-131.",
            "Is ego depletion real? An analysis of arguments",
        ),
        (
            "Dyar OJ, Beović B, Vlahović-Palčevski V, Verheij T, Pulcini C. How can we improve \
             antibiotic prescribing in primary care? Expert Rev Anti Infect Ther. \
             2016;14(4):403-413.",
            "How can we improve antibiotic prescribing in primary care?",
        ),
        (
            "Bickerdike L, Booth A, Wilson PM, Farley K, Wright K. Social prescribing: less \
             rhetoric and more reality. A systematic review of the evidence. BMJ Open. \
             2017;7:e013384.",
            "Social prescribing: less rhetoric and more reality. A systematic review of the \
             evidence",
        ),
        (
            "Mueller C, et al. Multicenter evaluation of a 0-hour/1-hour algorithm in the \
             diagnosis of myocardial infarction with high-sensitivity cardiac troponin T. Ann \
             Emerg Med. 2016;68(1):76-87.",
            "Multicenter evaluation of a 0-hour/1-hour algorithm in the diagnosis of \
             myocardial infarction with high-sensitivity cardiac troponin T",
        ),
        (
            "Fletcher R, Fernández-Götz M. Archaeological Perspectives on Urban \
             Sustainability: Issues and Reflections. Journal of Urban Archaeology. 2023;7:1-9.",
            "Archaeological Perspectives on Urban Sustainability: Issues and Reflections",
        ),
        (
            "14. van Hal ES et al. *Frontal plane roll-over analysis of prosthetic feet.* J \
             Biomech. 2021;3:4.",
            "Frontal plane roll-over analysis of prosthetic feet",
        ),
        (
            "Lin et al. 2011: Hearing loss and incident dementia. Arch Neurol. \
             68(2):214-220.",
            "Hearing loss and incident dementia",
        ),
    ] {
        assert_eq!(read(text, None).0.as_deref(), Some(title), "{text}");
    }
}

#[test]
fn a_quoted_title_is_one_only_right_after_the_byline() {
    let ieee = "A. Vaswani et al., \"Attention is all you need,\" in Proc. NeurIPS, 2017, \
                pp. 5998–6008.";
    assert_eq!(
        read(ieee, None),
        pair("Attention is all you need", &["A. Vaswani"])
    );
    // Words quoted inside the title aren't it.
    let inside = "Ball, J. E., Murrells, T., & Griffiths, P. (2014). \"Care left undone\" \
                  during nursing shifts: Associations with workload. BMJ Quality & Safety, \
                  23(2), 116–125.";
    assert_ne!(read(inside, None).0.as_deref(), Some("Care left undone"));
}

#[test]
fn the_cited_title_is_read_where_the_record_has_less_of_it() {
    // #168's claim: the record's title is the cited one's main title.
    let work = titled("The Third International Consensus Definitions for Sepsis and Septic Shock");
    let text = "Singer M, Deutschman CS, Seymour CW, et al. The Third International Consensus \
                Definitions for Sepsis and Septic Shock (Sepsis-3). JAMA. 2016;315(8):801-810.";
    assert_eq!(
        read(text, Some(&work)).0.as_deref(),
        Some(
            "The Third International Consensus Definitions for Sepsis and Septic Shock (Sepsis-3)"
        )
    );
    // A record whose title is the cited work's journal isn't read as the
    // title cited.
    let journal = titled("JAMA Network Open");
    let text = "Oduya-Quinlan, M. V., & Holmgren-Whitlock, W. (2020). The impact of wearable \
                sensors on falls prevention: a comprehensive review. JAMA Network Open, 41(11), \
                838–849.";
    assert_eq!(
        read(text, Some(&journal)).0.as_deref(),
        Some("The impact of wearable sensors on falls prevention: a comprehensive review")
    );
}

#[test]
fn a_bibtex_entry_gives_its_fields() {
    let text = "@article{seymour2016, author = {Seymour, Christopher W. and Liu, Vincent X.}, \
                title = {Assessment of Clinical Criteria for Sepsis}, journal = {JAMA}, year = \
                {2016}}";
    assert_eq!(
        read(text, None),
        pair(
            "Assessment of Clinical Criteria for Sepsis",
            &["Seymour, Christopher W.", "Liu, Vincent X."]
        )
    );
}

#[test]
fn fields_given_apart_are_kept() {
    let mut claim = Claim::from_text(SEYMOUR);
    claim.title = Some("A title given".into());
    claim.authors = vec!["Someone A".into()];
    let read = claim.as_read();
    assert_eq!(read.title.as_deref(), Some("A title given"));
    assert_eq!(read.authors, ["Someone A"]);
}

#[test]
fn bylines_read_as_names_in_one_style_or_not_at_all() {
    let cases: &[(&str, Option<&[&str]>)] = &[
        (
            "Seymour CW, Liu VX, Iwashyna TJ, et al.",
            Some(&["Seymour CW", "Liu VX", "Iwashyna TJ"]),
        ),
        (
            "Schepp SK, Longstreth WT Jr.",
            Some(&["Schepp SK", "Longstreth WT Jr"]),
        ),
        (
            "van der Poll T, Opal SM.",
            Some(&["van der Poll T", "Opal SM"]),
        ),
        (
            "Shahid, A. S. M. S. B., & Das, S. K. (2013).",
            Some(&["Shahid, A. S. M. S. B.", "Das, S. K."]),
        ),
        (
            "Smith, J. and Jones, K. (2016)",
            Some(&["Smith, J.", "Jones, K."]),
        ),
        ("S. Brin and L. Page,", Some(&["S. Brin", "L. Page"])),
        (
            "Deodato, Manuela, Caterina Qualizza, and Paolo Manganotti. 2024.",
            Some(&["Deodato, Manuela", "Caterina Qualizza", "Paolo Manganotti"]),
        ),
        ("Gilmer et al.,", Some(&["Gilmer"])),
        (
            "1. Hartwell J, Okafor N.",
            Some(&["Hartwell J", "Okafor N"]),
        ),
        // Not names, or not all of them.
        ("World Health Organization.", None),
        ("Javier Schloemann and R. Michael Buehrer. 2014.", None),
        ("Bentley, H. R. Triterpenoids", None),
        ("EMPEROR-Reduced trial investigators", None),
        ("Assessment of clinical criteria for sepsis", None),
    ];
    for (byline, expected) in cases {
        let expected = expected.map(|n| n.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        assert_eq!(names(byline), expected, "{byline}");
    }
}
