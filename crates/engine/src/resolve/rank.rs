//! Ranking candidates: by confidence and tie-breaks, then the claim's
//! site's publisher's when it matches about as well.

use aspero_core::matching::{self, PLAUSIBLE, VERIFIED};
use aspero_core::{Agreement, Claim, IdKind, Notice, WorkId, site};

use super::Candidate;

/// How much less confident than the first a candidate from the claim's
/// site's publisher may be and still be preferred to it ([`prefer_site`]):
/// less than any whole field's disagreement costs (a year one off, 0.125
/// with title, authors and year given; a cap, 0.1; another journal, 0.06
/// and more), so only part of a title's or a byline's words separate
/// them. Within it the claim can't tell the two apart; the site can.
const SITE_MARGIN: f64 = 0.05;

/// Most confident first; on a tie the one confirming more of the journal,
/// volume and page the claim cites (the article over a copy that can't
/// check them), then the earliest-published (an original precedes its
/// reprints, abstracts and corrections), then the one more providers agree
/// on. A notice first, for a claim citing the work it's about, gives way
/// to that work ([`prefer_original`]). Then the first may give way to a
/// candidate from the claim's site's publisher ([`prefer_site`]); returns
/// whether it did.
pub(super) fn rank(candidates: &mut [Candidate], claim: &Claim) -> bool {
    let issued = |c: &Candidate| {
        c.record.work.issued.map_or(i64::MAX, |d| {
            i64::from(d.year()) * 10_000
                + i64::from(d.month().unwrap_or(12)) * 100
                + i64::from(d.day().unwrap_or(31))
        })
    };
    let confirmed = |c: &Candidate| {
        let a = &c.assessment;
        [a.container, a.volume, a.page]
            .iter()
            .filter(|f| f.agreement == Agreement::Match)
            .count()
    };
    candidates.sort_by(|a, b| {
        b.assessment
            .confidence
            .total_cmp(&a.assessment.confidence)
            .then(confirmed(b).cmp(&confirmed(a)))
            .then(issued(a).cmp(&issued(b)))
            .then(b.sightings.len().cmp(&a.sightings.len()))
    });
    prefer_original(candidates, claim);
    prefer_site(candidates, claim)
}

/// The band of verdicts a candidate's confidence is in: verified (2),
/// plausible (1) or neither (0).
fn band(c: &Candidate) -> u8 {
    match c.assessment.confidence {
        x if x >= VERIFIED => 2,
        x if x >= PLAUSIBLE => 1,
        _ => 0,
    }
}

/// A notice first that is titled as the work it's about (Taylor &
/// Francis's retraction notices are: holdout-1 #721), for a claim citing
/// that work, not a notice: the work itself goes first, when it's among
/// the candidates in the same band (verified or plausible). Else the
/// notice of a retraction or withdrawal carries it, naming itself, so the
/// verdict on the work cited says it's retracted.
fn prefer_original(candidates: &mut [Candidate], claim: &Claim) {
    let Some(first) = candidates.first() else {
        return;
    };
    let about = &first.record.work.notice_for;
    // Titled as a notice ("Retraction: …", "Notice of Retraction"), it is
    // the notice the claim cites.
    let titled_as_notice = first.record.work.title.as_deref().is_some_and(|t| {
        let head = t.trim_start().split([' ', ':']).next().unwrap_or("");
        [
            "retraction",
            "retracted",
            "withdrawal",
            "withdrawn",
            "notice",
        ]
        .iter()
        .any(|w| head.eq_ignore_ascii_case(w))
    });
    if about.is_empty()
        || band(first) == 0
        || titled_as_notice
        || matching::claim_mentions_derivative(claim)
    {
        return;
    }
    let original = candidates.iter().position(|c| {
        band(c) == band(first)
            && about
                .iter()
                .any(|t| c.ids.iter().any(|id| matching::same_work_id(id, &t.work)))
    });
    if let Some(i) = original {
        candidates[..=i].rotate_right(1);
        return;
    }
    let first = &mut candidates[0];
    let own = first
        .ids
        .iter()
        .find(|id| id.kind() == IdKind::Doi)
        .cloned();
    if first.retraction().is_none()
        && let Some(target) = first
            .record
            .work
            .notice_for
            .iter()
            .find(|t| t.kind.is_retraction())
    {
        let mut notice = Notice::new(target.kind.clone());
        notice.notice = own;
        notice.date = target.date;
        notice.source.clone_from(&target.source);
        first.notices.push(notice);
    }
}

/// Whether a candidate's DOI is under one of a site's prefixes.
fn from_site(c: &Candidate, prefixes: &[&str]) -> bool {
    c.ids.iter().any(|id| match id {
        WorkId::Doi(doi) => site::has_prefix(doi, prefixes),
        _ => false,
    })
}

/// Puts first, of the ranked `candidates`, the one from the claim's site's
/// publisher (a DOI under [`Claim::site_prefixes`]) that best fits the
/// claim, when it matches about as well as the first: as confident within
/// [`SITE_MARGIN`], its title agreeing as well, and of the same verdict, at
/// least plausible. A tie-break, not a score: confidences stay their own,
/// so the verdict is the chosen work's own assessment, and the one the
/// first would have had. It never turns a weak match into a verified one,
/// never outranks a clearly better match, and with no candidate from the
/// publisher changes nothing. Of the publisher's, the most confident, then
/// the best title, then the best whole title (subtitles count: of two
/// works titled as cited, the one whose subtitle is cited too), then in
/// rank order. Returns whether another candidate became first.
fn prefer_site(candidates: &mut [Candidate], claim: &Claim) -> bool {
    let prefixes = claim.site_prefixes();
    let Some(first) = candidates.first() else {
        return false;
    };
    if prefixes.is_empty() || band(first) == 0 {
        return false;
    }
    let title_rank = |c: &Candidate| match c.assessment.title.agreement {
        Agreement::Match => 2,
        Agreement::Partial => 1,
        _ => 0,
    };
    let eligible = |c: &Candidate| {
        from_site(c, prefixes)
            && band(c) == band(first)
            && first.assessment.confidence - c.assessment.confidence <= SITE_MARGIN
            && title_rank(c) >= title_rank(first)
    };
    let fit = |c: &Candidate| {
        let whole = match (&claim.title, &c.record.work.title) {
            (Some(claimed), Some(actual)) => matching::whole_title_similarity(claimed, actual),
            _ => 0.0,
        };
        [
            c.assessment.confidence,
            c.assessment.title.score.unwrap_or(0.0),
            whole,
        ]
    };
    let better = |a: [f64; 3], b: [f64; 3]| {
        a.iter()
            .zip(&b)
            .map(|(x, y)| x.total_cmp(y))
            .find(|o| o.is_ne())
            .is_some_and(std::cmp::Ordering::is_gt)
    };
    let mut best: Option<(usize, [f64; 3])> = None;
    for (i, c) in candidates.iter().enumerate().filter(|(_, c)| eligible(c)) {
        let f = fit(c);
        if best.is_none_or(|(_, b)| better(f, b)) {
            best = Some((i, f));
        }
    }
    match best {
        Some((i, _)) if i > 0 => {
            candidates[..=i].rotate_right(1);
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use aspero_core::{SourceRecord, WorkType};

    use super::*;
    use crate::resolve::tests::titled;

    /// The candidates' DOIs, ranked for `claim`, and whether the site
    /// decided the first.
    fn ranked(claim: &Claim, records: &[SourceRecord]) -> (Vec<String>, bool) {
        let mut candidates: Vec<Candidate> = records
            .iter()
            .map(|r| Candidate {
                record: r.clone(),
                ids: r.ids.clone(),
                sightings: Vec::new(),
                assessment: matching::assess(claim, r),
                notices: Vec::new(),
            })
            .collect();
        let chosen = rank(&mut candidates, claim);
        let dois = candidates.iter().map(|c| c.ids[0].to_string()).collect();
        (dois, chosen)
    }

    fn claim(title: &str, authors: &[&str], year: Option<i16>, site: Option<&str>) -> Claim {
        let mut claim = Claim::default();
        claim.title = Some(title.into());
        claim.authors = authors.iter().map(|a| (*a).to_owned()).collect();
        claim.year = year;
        claim.site = site.map(str::to_owned);
        claim
    }

    const STROKE: &str = "Treatment of Acute Ischemic Stroke: A Review";

    #[test]
    fn the_sites_publisher_wins_a_tie() {
        let records = [
            titled("10.7897/2230-8407.08567", STROKE, &[], 2017),
            titled("10.1001/jama.2026.19482", STROKE, &[], 2026),
        ];
        let (dois, chosen) = ranked(&claim(STROKE, &[], None, None), &records);
        assert_eq!(dois[0], "doi:10.7897/2230-8407.08567");
        assert!(!chosen);
        for site in ["jamanetwork.com", "www.jamanetwork.com"] {
            let (dois, chosen) = ranked(&claim(STROKE, &[], None, Some(site)), &records);
            assert_eq!(
                dois,
                ["doi:10.1001/jama.2026.19482", "doi:10.7897/2230-8407.08567"]
            );
            assert!(chosen);
        }
        // No candidate is the publisher's: as without the site.
        for site in ["nature.com", "notjamanetwork.com"] {
            let (dois, chosen) = ranked(&claim(STROKE, &[], None, Some(site)), &records);
            assert_eq!(dois[0], "doi:10.7897/2230-8407.08567", "{site}");
            assert!(!chosen);
        }
        // With identifiers, the site says nothing.
        let mut with_id = claim(STROKE, &[], None, Some("jamanetwork.com"));
        with_id.ids = vec!["pmid:1".parse().unwrap()];
        assert!(!ranked(&with_id, &records).1);
    }

    #[test]
    fn the_sites_publisher_does_not_beat_a_clearly_better_match() {
        let site = Some("jamanetwork.com");
        let smith = claim(STROKE, &["Smith"], Some(2020), site);
        // Its year is off by one: both verify, the other clearly better.
        let records = [
            titled("10.7897/x", STROKE, &["Smith"], 2020),
            titled("10.1001/y", STROKE, &["Smith"], 2019),
        ];
        let (dois, chosen) = ranked(&smith, &records);
        assert_eq!(dois[0], "doi:10.7897/x");
        assert!(!chosen);
        // Its title only partly agrees.
        let records = [
            titled("10.7897/x", STROKE, &["Smith"], 2020),
            titled(
                "10.1001/y",
                "Treatment of acute ischemic stroke in older adults: outcomes",
                &["Smith"],
                2020,
            ),
        ];
        let (dois, chosen) = ranked(&smith, &records);
        assert_eq!(dois[0], "doi:10.7897/x");
        assert!(!chosen);
        // Nothing is even plausible: no work to choose.
        let records = [
            titled("10.7897/x", "Stroke units", &["Lee"], 2020),
            titled("10.1001/y", "Stroke units", &["Lee"], 2020),
        ];
        let (dois, chosen) = ranked(&claim("Stroke care", &["Jones"], None, site), &records);
        assert_eq!(dois[0], "doi:10.7897/x");
        assert!(!chosen);
    }

    #[test]
    fn of_the_publishers_namesakes_the_one_with_the_cited_subtitle() {
        // As Crossref gives it, without its subtitle ("Open Trial With …"):
        // its main title is the one cited, so its title matches as well.
        let records = [
            titled("10.7897/x", STROKE, &[], 2017),
            titled(
                "10.1001/archneur.1986.00520040066022",
                "Treatment of Acute Ischemic Stroke",
                &[],
                1986,
            ),
            titled("10.1001/jama.2026.19482", STROKE, &[], 2026),
        ];
        let (dois, chosen) = ranked(&claim(STROKE, &[], None, Some("jamanetwork.com")), &records);
        assert_eq!(dois[0], "doi:10.1001/jama.2026.19482");
        assert!(chosen);
    }

    #[test]
    fn a_retraction_notice_titled_as_its_original_gives_way_to_it() {
        // holdout-1 #721: Taylor & Francis's notice is titled as the
        // article it retracts, and matches its citation as well.
        let title = "Further Evidence of Critical Thinking and Final Examination Performance in \
                     Advanced Financial Accounting";
        let original_doi: WorkId = "doi:10.1080/09639280903180774".parse().unwrap();
        let mut notice = titled(
            "10.1080/09639284.2011.560642",
            title,
            &["Indra Abeysekera"],
            2011,
        );
        notice.work.notice_for.push(aspero_core::NoticeFor::new(
            aspero_core::NoticeKind::Retraction,
            original_doi.clone(),
        ));
        notice.work.type_as_notice();
        let original = titled(
            "10.1080/09639280903180774",
            &format!("RETRACTED ARTICLE: {title}"),
            &["Indra Abeysekera"],
            2011,
        );
        let claim = Claim::from_text(format!(
            "Abeysekera I. {title}. Accounting Education. 2011;20(1):i-xviii."
        ));
        let (dois, _) = ranked(&claim, &[notice.clone(), original]);
        assert_eq!(dois[0], original_doi.to_string());

        // Without the original among the candidates, the notice says what
        // it notifies: a retraction, by itself.
        let mut candidates = vec![Candidate {
            record: notice.clone(),
            ids: notice.ids.clone(),
            sightings: Vec::new(),
            assessment: matching::assess(&claim, &notice),
            notices: Vec::new(),
        }];
        rank(&mut candidates, &claim);
        let flag = candidates[0].retraction().expect("a retraction notice");
        assert_eq!(flag.notice.as_ref(), Some(&notice.ids[0]));
        // Once only, however often ranked.
        rank(&mut candidates, &claim);
        assert_eq!(candidates[0].notices.len(), 1);

        // A claim citing the notice cites no retracted work.
        let cited = Claim::from_text(format!(
            "Retraction of: {title}. Accounting Education. 2011;20(1):1."
        ));
        candidates[0].notices.clear();
        rank(&mut candidates, &cited);
        assert!(candidates[0].notices.is_empty());
        // Nor does one matching a notice titled as one.
        let notice_title = format!("Retraction: {title}");
        candidates[0].record.work.title = Some(notice_title.clone());
        let cited = Claim::from_text(format!(
            "Abeysekera I. {notice_title}. Accounting Education. 2011;20(1):1."
        ));
        rank(&mut candidates, &cited);
        assert!(candidates[0].notices.is_empty());
    }

    #[test]
    fn the_article_outranks_a_copy_that_cant_check_its_place() {
        // Red team (#102): Tversky & Kahneman's Science article, cited with
        // a wrong page, against the earlier technical report.
        let title = "Judgment under Uncertainty: Heuristics and Biases";
        let authors = ["Amos Tversky", "Daniel Kahneman"];
        let mut report = titled("10.21236/ad0767426", title, &authors, 1973);
        report.work.work_type = Some(WorkType::Report);
        let mut article = titled("10.1126/science.185.4157.1124", title, &authors, 1974);
        article.work.container_title = Some("Science".into());
        article.work.volume = Some("185".into());
        article.work.page = Some("1124-1131".into());
        let claim = Claim::from_text(
            "Tversky A, Kahneman D. Judgment under uncertainty: heuristics and biases. Science. \
             1974;185:1353.",
        );
        let (dois, _) = ranked(&claim, &[report, article]);
        assert_eq!(dois[0], "doi:10.1126/science.185.4157.1124");
    }
}
