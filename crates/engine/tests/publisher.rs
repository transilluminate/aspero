//! A verified book checked against its editions: the publisher its
//! citation names, against Open Library's editions (recorded by
//! `scripts/record-openlibrary.sh`). Cases from holdout-6's
//! `book-wrong-publisher`.
#![cfg(feature = "openlibrary")]

mod common;

use std::sync::{Arc, Mutex};

use aspero_core::testing::{fixture, response};
use aspero_core::{
    Claim, Error, HttpClient, HttpRequest, HttpResponse, Person, ResiliencePolicy, WorkType,
};
use aspero_engine::{CitedField, Engine, Reason, Verdict, Verification};
use aspero_openlibrary::{OpenLibrary, OpenLibraryConfig};
use async_trait::async_trait;
use common::*;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../providers/openlibrary/tests/fixtures"
);

/// Answers Open Library's requests with the fixture whose name its URL's
/// key words pick (`[(words, fixture)]`), else with a search that finds
/// nothing; or fails them all. Keeps the URLs asked.
struct Routes {
    fixtures: Vec<(&'static str, &'static str)>,
    failing: bool,
    urls: Mutex<Vec<String>>,
}

impl Routes {
    fn new(routes: &[(&'static str, &'static str)]) -> Arc<Self> {
        Arc::new(Routes {
            fixtures: routes.to_vec(),
            failing: false,
            urls: Mutex::default(),
        })
    }

    fn failing() -> Arc<Self> {
        Arc::new(Routes {
            fixtures: Vec::new(),
            failing: true,
            urls: Mutex::default(),
        })
    }

    /// The URLs the edition check asked: the book by title and author,
    /// and a work's editions (not the title searches verifying makes
    /// anyway).
    fn urls(&self) -> Vec<String> {
        let urls = self.urls.lock().unwrap();
        urls.iter()
            .filter(|u| u.contains("search.json?q=&") || u.contains("/editions.json"))
            .cloned()
            .collect()
    }
}

#[async_trait]
impl HttpClient for Routes {
    async fn send(&self, request: HttpRequest) -> Result<HttpResponse, Error> {
        self.urls.lock().unwrap().push(request.url.clone());
        if self.failing {
            return Ok(response(503, &[], ""));
        }
        let found = self
            .fixtures
            .iter()
            .find(|(words, _)| request.url.contains(words));
        Ok(match found {
            Some((_, name)) => fixture(FIXTURES, name),
            None => response(200, &[], r#"{"numFound": 0, "docs": []}"#),
        })
    }
}

/// Crossref's record of a book (its matcher's only hit), then Open
/// Library behind `http`.
fn engine(book: aspero_core::SourceRecord, http: Arc<Routes>) -> Engine {
    let crossref = Arc::new(Scripted::new("crossref").matcher(vec![book]));
    let ol = OpenLibrary::new(OpenLibraryConfig::default(), http);
    // A failure fails at once.
    let mut once = ResiliencePolicy::default();
    once.max_retries = 0;
    Engine::builder()
        .provider(crossref, ResiliencePolicy::default(), 0)
        .provider(Arc::new(ol), once, 1)
        .build()
}

fn book(
    doi: &str,
    title: &str,
    authors: &[(&str, &str)],
    year: i16,
    container: Option<&str>,
) -> aspero_core::SourceRecord {
    let mut r = work("crossref", &[doi], title, &[], year);
    r.work.authors = authors
        .iter()
        .map(|(given, family)| {
            let mut p = Person::default();
            p.given = Some((*given).into());
            p.family = Some((*family).into());
            p
        })
        .collect();
    r.work.work_type = Some(WorkType::Book);
    r.work.container_title = container.map(str::to_owned);
    r
}

/// holdout-6 #408: Routledge's book, as Crossref and OpenAlex have it.
fn yapko(container: Option<&str>) -> aspero_core::SourceRecord {
    book(
        "10.4324/9780203717523",
        "Treating Depression With Hypnosis",
        &[("Michael D.", "Yapko")],
        2013,
        container,
    )
}

const YAPKO_ROUTES: &[(&str, &str)] = &[
    ("author=Yapko", "search_yapko"),
    ("OL21285529W/editions", "editions_yapko"),
];

fn publisher_differs(v: &Verification) -> (Option<&str>, Option<&str>) {
    let Reason::PartlyMatches { differs } = &v.reason else {
        panic!("{:?}: {}", v.reason, v.reason_text);
    };
    assert_eq!(differs.len(), 1, "{differs:?}");
    assert_eq!(differs[0].field, CitedField::Publisher);
    (differs[0].actual.as_deref(), differs[0].cited.as_deref())
}

#[tokio::test]
async fn a_book_cited_with_a_publisher_none_of_its_editions_names_is_plausible() {
    let http = Routes::new(YAPKO_ROUTES);
    let engine = engine(yapko(Some("Routledge eBooks")), http.clone());
    let text = "Yapko, M. D. (2013). *Treating Depression With Hypnosis*. Palgrave Macmillan.";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert_eq!(
        publisher_differs(&v),
        (Some("Taylor & Francis Group"), Some("Palgrave Macmillan"))
    );
    assert!(
        v.reason_text
            .ends_with("its publisher (\"Taylor & Francis Group\") differs"),
        "{}",
        v.reason_text
    );
    let best = &v.report.candidates[v.best_match.unwrap()];
    assert!(best.assessment.confidence < aspero_core::matching::VERIFIED);
    // The book found by its title and first author, then its work's
    // editions: two requests.
    let urls = http.urls();
    assert_eq!(urls.len(), 2, "{urls:?}");
    assert!(urls[0].contains("/search.json?q=&author=Yapko&title=Treating"));
    assert!(urls[1].ends_with("/works/OL21285529W/editions.json?limit=100"));
}

#[tokio::test]
async fn a_book_cited_with_its_publisher_asks_nothing_more() {
    // Its record names it (OpenAlex's "Routledge eBooks"), or its group's
    // imprint.
    for cited in ["Routledge", "CRC Press", "Taylor & Francis"] {
        let http = Routes::new(YAPKO_ROUTES);
        let engine = engine(yapko(Some("Routledge eBooks")), http.clone());
        let text = format!("Yapko, M. D. (2013). Treating Depression With Hypnosis. {cited}.");
        let v = engine.verify(&Claim::from_text(text), 5).await;
        assert_eq!(v.verdict, Verdict::Verified, "{cited}: {}", v.reason_text);
        assert!(http.urls().is_empty(), "{cited}: {:?}", http.urls());
    }
}

#[tokio::test]
async fn a_publisher_an_edition_names_verifies() {
    // No publisher in the record: the editions name Taylor & Francis.
    let http = Routes::new(YAPKO_ROUTES);
    let engine = engine(yapko(None), http.clone());
    let text = "Yapko, Michael D. 2013. Treating Depression With Hypnosis. New York: Taylor & \
                Francis Group.";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
    assert_eq!(http.urls().len(), 2);
}

#[tokio::test]
async fn a_book_open_library_lacks_or_cant_answer_for_stays_verified() {
    let text = "Yapko, M. D. (2013). *Treating Depression With Hypnosis*. Palgrave Macmillan.";
    for http in [Routes::new(&[]), Routes::failing()] {
        let engine = engine(yapko(Some("Routledge eBooks")), http.clone());
        let v = engine.verify(&Claim::from_text(text), 5).await;
        assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
        assert_eq!(http.urls().len(), 1, "the search, no editions");
    }
}

#[tokio::test]
async fn the_work_of_the_year_cited_gives_the_editions() {
    // holdout-6 #94: Routledge's 2013 reissue of a book Allen & Unwin
    // published from 1909; Open Library has both as works.
    let http = Routes::new(&[
        ("author=Jespersen", "search_jespersen"),
        ("OL34016458W/editions", "editions_jespersen"),
    ]);
    let record = book(
        "10.4324/9780203715987",
        "A Modern English Grammar on Historical Principles",
        &[("Otto", "Jespersen")],
        2013,
        None,
    );
    let engine = engine(record, http.clone());
    let text = "Jespersen, O. (2013). *A Modern English Grammar on Historical Principles: \
                Volume 4. Syntax (third volume)*. Palgrave Macmillan.";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert_eq!(
        publisher_differs(&v),
        (Some("Routledge"), Some("Palgrave Macmillan"))
    );
    // Allen & Unwin, an edition's publisher of the earlier work, verifies.
    let text = "Jespersen, O. (2013). *A Modern English Grammar on Historical Principles*. \
                Routledge.";
    let v = engine.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}

#[tokio::test]
async fn two_editions_and_the_record_agreeing_are_enough() {
    // holdout-6 #32: Palgrave's book; Open Library knows two editions.
    let http = Routes::new(&[
        ("author=Cronin", "search_cronin"),
        ("OL11943094W/editions", "editions_cronin"),
    ]);
    let record = book(
        "10.1007/978-1-349-09556-8",
        "Colour and Experience in Nineteenth-Century Poetry",
        &[("Richard", "Cronin")],
        1988,
        Some("Palgrave Macmillan UK eBooks"),
    );
    let palgrave = engine(record.clone(), http);
    let text = "Cronin, Richard. 1988. Colour and Experience in Nineteenth-Century Poetry. \
                Routledge.";
    let v = palgrave.verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Plausible, "{}", v.reason_text);
    assert_eq!(
        publisher_differs(&v),
        (Some("Palgrave Macmillan"), Some("Routledge"))
    );
    // Without the record's word, two editions are too few to tell.
    let mut bare = record;
    bare.work.container_title = None;
    let http = Routes::new(&[
        ("author=Cronin", "search_cronin"),
        ("OL11943094W/editions", "editions_cronin"),
    ]);
    let v = engine(bare, http).verify(&Claim::from_text(text), 5).await;
    assert_eq!(v.verdict, Verdict::Verified, "{}", v.reason_text);
}

/// holdout-6's twelve `book-wrong-publisher` rows, each with its record
/// as Crossref (and OpenAlex's container) gives it, against Open
/// Library's recorded answers: `None` where it lacks the book.
#[tokio::test]
async fn holdout_6s_books_with_another_publisher() {
    struct Row {
        n: u32,
        claim: &'static str,
        title: &'static str,
        author: (&'static str, &'static str),
        year: i16,
        container: Option<&'static str>,
        chapter: bool,
        open_library: Option<(&'static str, &'static str, &'static str)>,
        plausible: bool,
    }
    let rows = [
        Row {
            n: 32,
            claim: "Cronin, Richard. 1988. Colour and Experience in Nineteenth-Century Poetry. \
                    Routledge.",
            title: "Colour and Experience in Nineteenth-Century Poetry",
            author: ("Richard", "Cronin"),
            year: 1988,
            container: Some("Palgrave Macmillan UK eBooks"),
            chapter: false,
            open_library: Some(("search_cronin", "OL11943094W", "editions_cronin")),
            plausible: true,
        },
        // Its record is a chapter's: no book to check.
        Row {
            n: 85,
            claim: "Petersen, Thomas Søbirk. 2024. Ethics and Situational Crime Prevention. \
                    Palgrave Macmillan.",
            title: "Ethics and situational crime prevention",
            author: ("Thomas Søbirk", "Petersen"),
            year: 2024,
            container: Some("Ethics and Situational Crime Prevention"),
            chapter: true,
            open_library: None,
            plausible: false,
        },
        Row {
            n: 94,
            claim: "Jespersen, O. (2013). *A Modern English Grammar on Historical Principles: \
                    Volume 4. Syntax (third volume)*. Palgrave Macmillan.",
            title: "A Modern English Grammar on Historical Principles",
            author: ("Otto", "Jespersen"),
            year: 2013,
            container: None,
            chapter: false,
            open_library: Some(("search_jespersen", "OL34016458W", "editions_jespersen")),
            plausible: true,
        },
        Row {
            n: 188,
            claim: "Barker, R. (2007). *Early Modern Tragedy, Gender and Performance, \
                    1984–2000*. Routledge.",
            title: "Early Modern Tragedy, Gender and Performance, 1984–2000",
            author: ("Roberta", "Barker"),
            year: 2007,
            container: Some("Palgrave Macmillan UK eBooks"),
            chapter: false,
            open_library: Some(("search_barker", "OL8008683W", "editions_barker")),
            plausible: true,
        },
        Row {
            n: 262,
            claim: "Scolnicov, S. (2013) Plato 's Metaphysics of Education (RLE: Plato). \
                    Palgrave Macmillan.",
            title: "Plato 's Metaphysics of Education (RLE: Plato)",
            author: ("Samuel", "Scolnicov"),
            year: 2013,
            container: Some("Routledge eBooks"),
            chapter: false,
            open_library: Some(("search_scolnicov", "OL21269086W", "editions_scolnicov")),
            plausible: true,
        },
        Row {
            n: 408,
            claim: "Yapko, M. D. (2013). *Treating Depression With Hypnosis*. Palgrave \
                    Macmillan.",
            title: "Treating Depression With Hypnosis",
            author: ("Michael D.", "Yapko"),
            year: 2013,
            container: Some("Routledge eBooks"),
            chapter: false,
            open_library: Some(("search_yapko", "OL21285529W", "editions_yapko")),
            plausible: true,
        },
        // One edition, and a record naming no publisher: too few to tell.
        Row {
            n: 445,
            claim: "Anjum, R.L. and Rocca, E. (2024) Philosophy of Science. Elsevier.",
            title: "Philosophy of Science",
            author: ("Rani Lill", "Anjum"),
            year: 2024,
            container: Some("Palgrave Philosophy Today"),
            chapter: false,
            open_library: Some(("search_anjum", "OL37926341W", "editions_anjum")),
            plausible: false,
        },
        Row {
            n: 575,
            claim: "Kolkowicz, R. (2019). *The Soviet Military and the Communist Party*. \
                    Palgrave Macmillan.",
            title: "The Soviet Military and the Communist Party",
            author: ("Roman", "Kolkowicz"),
            year: 2019,
            container: Some("Routledge eBooks"),
            chapter: false,
            open_library: Some(("search_kolkowicz", "OL18401623W", "editions_kolkowicz")),
            plausible: true,
        },
        // Open Library lacks it.
        Row {
            n: 640,
            claim: "Oakley, A. (2014). *Helpers In Childbirth: Midwifery Today*. Palgrave \
                    Macmillan.",
            title: "Helpers In Childbirth: Midwifery Today",
            author: ("Ann", "Oakley"),
            year: 2014,
            container: Some("Routledge eBooks"),
            chapter: false,
            open_library: Some(("search_oakley", "", "")),
            plausible: false,
        },
        Row {
            n: 659,
            claim: "Laughland, J. (2009). *A History of Political Trials*. Brill.",
            title: "A History of Political Trials",
            author: ("John", "Laughland"),
            year: 2009,
            container: None,
            chapter: false,
            open_library: Some(("search_laughland", "OL17526122W", "editions_laughland")),
            plausible: false,
        },
        Row {
            n: 857,
            claim: "Ghosal, A. (2021) Engineering Aspects of Howrah Bridge at Kolkata (1943). \
                    Wiley.",
            title: "Engineering Aspects of Howrah Bridge at Kolkata (1943)",
            author: ("Amitabha", "Ghosal"),
            year: 2021,
            container: Some("CRC Press eBooks"),
            chapter: false,
            open_library: Some(("search_ghosal", "", "")),
            plausible: false,
        },
        Row {
            n: 937,
            claim: "Rayees, Sheikh and Inshah Din. 2021. Asthma: Pathophysiology, Herbal and \
                    Modern Therapeutic Interventions. Elsevier.",
            title: "Asthma: Pathophysiology, Herbal and Modern Therapeutic Interventions",
            author: ("Sheikh", "Rayees"),
            year: 2021,
            container: Some("SpringerBriefs in Immunology"),
            chapter: false,
            open_library: Some(("search_rayees", "", "")),
            plausible: false,
        },
    ];
    for row in rows {
        let mut record = book(
            "10.4324/0000000000",
            row.title,
            &[row.author],
            row.year,
            row.container,
        );
        if row.chapter {
            record.work.work_type = Some(WorkType::Chapter);
        }
        let author: &'static str = format!("author={}", row.author.1).leak();
        let routes: Vec<(&'static str, &'static str)> = match row.open_library {
            Some((search, key, editions)) if !key.is_empty() => {
                vec![
                    (author, search),
                    (format!("{key}/editions").leak(), editions),
                ]
            }
            Some((search, _, _)) => vec![(author, search)],
            None => Vec::new(),
        };
        let http = Routes::new(&routes);
        let v = engine(record, http.clone())
            .verify(&Claim::from_text(row.claim), 5)
            .await;
        let expected = if row.plausible {
            Verdict::Plausible
        } else {
            Verdict::Verified
        };
        assert_eq!(v.verdict, expected, "#{}: {}", row.n, v.reason_text);
        if row.plausible {
            publisher_differs(&v);
        }
        if row.chapter {
            assert!(http.urls().is_empty(), "#{}", row.n);
        }
    }
}
