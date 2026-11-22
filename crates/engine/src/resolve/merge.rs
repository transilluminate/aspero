//! Merging records of one work into a candidate: identifiers, notices,
//! values sources give differently, years, and journal, volume and page.

use aspero_core::matching::{self, id_agreement, same_work_id};
use aspero_core::{
    Agreement, Claim, IdKind, Notice, NoticeKind, OpenAccess, PartialDate, Person, ProviderId,
    SourceRecord, Usage, WorkId, WorkType,
};

use super::rank::rank;
use super::{Candidate, ResolveReport, Sighting, Via};
use crate::registry::{carries_doi, drop_foreign, overrules};
use crate::{EngineError, SearchReport, add_usage};

/// Confidence at which `verify` stops gathering candidates early.
const DECISIVE: f64 = 0.95;

/// The source whose cited-by count a merged work shows when it has one:
/// the broadest index.
const COUNTS: &str = "openalex";

/// Whether a record's cited-by count is [`COUNTS`]'s.
fn from_counts(record: &SourceRecord) -> bool {
    record.source.as_str() == COUNTS
}

/// Scores a record against the claim with every identifier known for its
/// work (`ids`, merged from its other records), so a claimed identifier
/// counts whichever record of the work is kept: arXiv's DataCite record of
/// a paper cited by its journal DOI. And with every newer version its
/// records name (`notices`): OpenAlex's record of a Cochrane review's
/// version says nothing of the version superseding it, Crossref's does.
pub(super) fn assess(
    claim: &Claim,
    record: &SourceRecord,
    ids: &[WorkId],
    notices: &[Notice],
) -> matching::Assessment {
    let known = |id: &WorkId| record.ids.iter().any(|r| same_work_id(r, id));
    let newer: Vec<&Notice> = notices
        .iter()
        .filter(|n| n.kind == NoticeKind::NewVersion && !record.work.notices.contains(n))
        .collect();
    if ids.iter().all(known) && newer.is_empty() {
        return matching::assess(claim, record);
    }
    let mut record = record.clone();
    for id in ids {
        if !record.ids.iter().any(|r| same_work_id(r, id)) {
            record.ids.push(id.clone());
        }
    }
    record.work.notices.extend(newer.into_iter().cloned());
    matching::assess(claim, &record)
}

/// The claimed identifiers against every identifier known for a work
/// (`ids`): partly agreeing when one is an earlier version's, which its
/// record names ([`matching::earlier_id_claimed`]).
pub(super) fn judge_ids(claim: &Claim, ids: &[WorkId], record: &SourceRecord) -> Agreement {
    match id_agreement(&claim.ids, ids) {
        Agreement::Match if matching::earlier_id_claimed(claim, record) => Agreement::Partial,
        agreement => agreement,
    }
}

/// Shares between two records of one version (sharing a DOI or PMID) the
/// earlier versions each says it supersedes: Crossref's record of a
/// Cochrane review's version names the one before, Europe PMC's doesn't.
fn share_earlier_versions(a: &mut SourceRecord, b: &mut SourceRecord) {
    let shared = a.ids.iter().any(|x| {
        matches!(x.kind(), IdKind::Doi | IdKind::Pmid) && b.ids.iter().any(|y| same_work_id(x, y))
    });
    if !shared {
        return;
    }
    let give = |from: &SourceRecord, to: &mut SourceRecord| {
        for n in &from.work.notice_for {
            if n.kind == NoticeKind::NewVersion
                && !to.work.notice_for.contains(n)
                && !to.ids.iter().any(|id| same_work_id(id, &n.work))
            {
                to.work.notice_for.push(n.clone());
            }
        }
    };
    give(a, b);
    give(b, a);
}

/// Merges another record of the same work (found in `sightings`) into a
/// candidate: its identifiers, notices and sightings, every year reported,
/// and the record itself if it matches the claim better. Whichever record
/// wins, the work shows the same values whichever source answered first
/// ([`merged_values`]). `counts` says whether the candidate's and the
/// record's cited-by counts are OpenAlex's; returns whether the
/// candidate's then is.
fn absorb(
    claim: &Claim,
    c: &mut Candidate,
    counts: (bool, bool),
    ids: &[WorkId],
    notices: &[Notice],
    sightings: Vec<Sighting>,
    mut record: SourceRecord,
) -> bool {
    c.sightings.extend(sightings);
    for id in ids.iter().chain(&record.ids) {
        if !c.ids.iter().any(|known| same_work_id(known, id)) {
            c.ids.push(id.clone());
        }
    }
    merge_notices(&mut c.notices, notices);
    merge_notices(&mut c.notices, &record.work.notices);
    // Pool the years every source reports (online-first vs print,
    // editions), then keep the better-matching record.
    let pooled: Vec<i16> = years(&c.record).chain(years(&record)).collect();
    let counts = merged_values(&mut c.record, &mut record, counts);
    add_years(&mut c.record, &pooled);
    add_years(&mut record, &pooled);
    add_place(&mut c.record, &record);
    add_place(&mut record, &c.record);
    share_earlier_versions(&mut c.record, &mut record);
    c.assessment = assess(claim, &c.record, &c.ids, &c.notices);
    let assessment = assess(claim, &record, &c.ids, &c.notices);
    if assessment.confidence > c.assessment.confidence {
        c.record = record;
        c.assessment = assessment;
        return counts.1;
    }
    counts.0
}

/// Values sources give differently for one work, made the same on both
/// records so the work doesn't change with the source that won the merge
/// (#81). `counts` says whether each side's cited-by count is OpenAlex's;
/// returns whether each side's then is.
///
/// - Open access: the more informative, whatever the version (a free
///   preprint is a free copy): free with a PDF, then free with a page,
///   then free, then not free (with a licence first), then unknown.
/// - Between records of one version (sharing a DOI or PMID, so not a
///   preprint and its article):
///   - cited-by count: OpenAlex's when either has one, else the larger;
///   - issued: the earliest (online first, then in print: OpenAlex's
///     2020-12-01 over Crossref's 2021-01), and of dates that overlap
///     (2021 and 2021-01-05) the more precise. The other date's year stays
///     in `other_years`, where verification's year matching finds it.
fn merged_values(a: &mut SourceRecord, b: &mut SourceRecord, counts: (bool, bool)) -> (bool, bool) {
    let oa = best_open_access(a.work.open_access.as_ref(), b.work.open_access.as_ref()).cloned();
    a.work.open_access.clone_from(&oa);
    b.work.open_access = oa;

    let shared = a.ids.iter().any(|x| {
        matches!(x.kind(), IdKind::Doi | IdKind::Pmid) && b.ids.iter().any(|y| same_work_id(x, y))
    });
    if !shared {
        return counts;
    }
    let (count, counted) = match (a.work.cited_by_count, b.work.cited_by_count, counts) {
        (Some(n), _, (true, _)) | (_, Some(n), (_, true)) => (Some(n), true),
        (x, y, _) => (x.max(y), false),
    };
    a.work.cited_by_count = count;
    b.work.cited_by_count = count;
    let issued = match (a.work.issued, b.work.issued) {
        (Some(x), Some(y)) => Some(earliest(x, y)),
        (x, y) => x.or(y),
    };
    a.work.issued = issued;
    b.work.issued = issued;
    share_author_ids(&mut a.work.authors, &b.work.authors);
    share_author_ids(&mut b.work.authors, &a.work.authors);
    (counted, counted)
}

/// Gives `to`'s authors the OpenAlex ids `from`'s have (#174), so a work
/// led by another source's record still names them. Two authors are the
/// same one when their family names agree and their first initials don't
/// differ, at the same place in both lists, or as the only such pair in
/// either list ("Wei Zhang" and "Li Zhang" are told apart wherever they
/// stand).
fn share_author_ids(to: &mut [Person], from: &[Person]) {
    let key = |p: &Person| {
        let (family, given) = p.parts()?;
        let family = matching::normalise(&family);
        let initial = given.and_then(|g| matching::normalise(&g).chars().next());
        (!family.is_empty()).then_some((family, initial))
    };
    let alike = |a: &(String, Option<char>), b: &(String, Option<char>)| {
        a.0 == b.0 && (a.1.is_none() || b.1.is_none() || a.1 == b.1)
    };
    let theirs: Vec<_> = from.iter().map(key).collect();
    let mine: Vec<_> = to.iter().map(key).collect();
    for (i, person) in to.iter_mut().enumerate() {
        let Some(k) = &mine[i] else { continue };
        if person.openalex_id.is_some() {
            continue;
        }
        let matches = |list: &[Option<(String, Option<char>)>]| {
            list.iter()
                .enumerate()
                .filter(|(_, o)| o.as_ref().is_some_and(|o| alike(k, o)))
                .map(|(j, _)| j)
                .collect::<Vec<_>>()
        };
        let same = if theirs
            .get(i)
            .and_then(Option::as_ref)
            .is_some_and(|o| alike(k, o))
        {
            Some(i)
        } else {
            match (matches(&theirs).as_slice(), matches(&mine).len()) {
                (&[j], 1) => Some(j),
                _ => None,
            }
        };
        if let Some(j) = same {
            person.openalex_id.clone_from(&from[j].openalex_id);
        }
    }
}

/// The more informative of two open-access records; on a tie, the first.
fn best_open_access<'a>(
    a: Option<&'a OpenAccess>,
    b: Option<&'a OpenAccess>,
) -> Option<&'a OpenAccess> {
    let rank = |oa: Option<&OpenAccess>| {
        oa.map(|oa| {
            (
                oa.is_oa,
                oa.pdf_url.is_some(),
                oa.url.is_some(),
                oa.license.is_some(),
            )
        })
    };
    if rank(b) > rank(a) { b } else { a }
}

/// Of two dates of one work, the earlier; of two that overlap ("2021"
/// and "2021-01-05"), the more precise; on a tie, the first.
fn earliest(a: PartialDate, b: PartialDate) -> PartialDate {
    let bounds = |d: PartialDate| {
        let start = (d.year(), d.month().unwrap_or(1), d.day().unwrap_or(1));
        let end = (d.year(), d.month().unwrap_or(12), d.day().unwrap_or(31));
        (start, end)
    };
    let precision =
        |d: PartialDate| usize::from(d.month().is_some()) + usize::from(d.day().is_some());
    let ((a_start, a_end), (b_start, b_end)) = (bounds(a), bounds(b));
    if b_end < a_start {
        b
    } else if a_end < b_start || precision(a) >= precision(b) {
        a
    } else {
        b
    }
}

/// Adds notices not already known. A detailed notice (with the notice's
/// identifier) replaces a bare one of the same kind, such as OpenAlex's
/// `is_retracted` flag.
pub(crate) fn merge_notices(known: &mut Vec<Notice>, new: &[Notice]) {
    for update in new {
        let same_kind = |k: &Notice| k.kind == update.kind;
        if known
            .iter()
            .any(|k| same_kind(k) && k.notice == update.notice)
        {
            continue; // already known
        }
        if update.notice.is_none() {
            if !known.iter().any(same_kind) {
                known.push(update.clone());
            }
            continue; // a bare flag adds nothing to a known notice
        }
        match known
            .iter_mut()
            .find(|k| same_kind(k) && k.notice.is_none())
        {
            Some(bare) => *bare = update.clone(),
            None => known.push(update.clone()),
        }
    }
}

pub(super) struct Accumulator<'a> {
    pub(super) claim: &'a Claim,
    pub(super) report: ResolveReport,
    /// Whether each candidate's cited-by count is OpenAlex's.
    pub(super) counted: Vec<bool>,
    /// The DOI registry (Crossref), if it's here.
    pub(super) registry: Option<ProviderId>,
    /// The registry's records seen so far, one per DOI.
    pub(super) registered: Vec<SourceRecord>,
}

impl Accumulator<'_> {
    /// Adds a record, merging it into an existing candidate that shares an
    /// identifier, and then any other candidate the record links it to.
    /// The registry's record of a DOI wins for it first ([`Self::defer`]).
    pub(super) fn add(&mut self, mut record: SourceRecord, sighting: Sighting) {
        self.defer(&mut record);
        let claim = self.claim;
        let candidates = &mut self.report.candidates;
        let shares = |c: &Candidate, ids: &[WorkId]| {
            c.ids.iter().any(|a| ids.iter().any(|b| same_work_id(a, b)))
        };
        let mut at = if let Some(i) = candidates.iter().position(|c| shares(c, &record.ids)) {
            let counts = (self.counted[i], from_counts(&record));
            self.counted[i] = absorb(
                claim,
                &mut candidates[i],
                counts,
                &[],
                &[],
                vec![sighting],
                record,
            );
            i
        } else {
            self.counted.push(from_counts(&record));
            let mut notices = Vec::new();
            merge_notices(&mut notices, &record.work.notices);
            candidates.push(Candidate {
                ids: record.ids.clone(),
                notices,
                sightings: vec![sighting],
                assessment: matching::assess(claim, &record),
                record,
            });
            candidates.len() - 1
        };
        // A record carrying two candidates' identifiers (a DOI one source
        // knows, a PMID another does) makes them one work.
        while let Some(j) =
            (0..candidates.len()).find(|&j| j != at && shares(&candidates[j], &candidates[at].ids))
        {
            let (keep, gone) = (at.min(j), at.max(j));
            let counts = (self.counted[keep], self.counted.remove(gone));
            let gone = candidates.remove(gone);
            let c = &mut candidates[keep];
            self.counted[keep] = absorb(
                claim,
                c,
                counts,
                &gone.ids,
                &gone.notices,
                gone.sightings,
                gone.record,
            );
            at = keep;
        }
        // Judge claimed identifiers against everything known about the work.
        let candidate = &mut candidates[at];
        candidate.assessment.ids = judge_ids(claim, &candidate.ids, &candidate.record);
    }

    /// The registry's record of a DOI wins for that DOI (#183): the
    /// aggregator's record, where it contradicts the registry's
    /// ([`overrules`]: two works it merged into one), doesn't carry that
    /// DOI, nor the PMIDs and PMCIDs the registry's record doesn't confirm
    /// ([`drop_foreign`]), so it isn't merged with the registry's record
    /// and lends it none of its values. A registry record takes them from
    /// the candidates already found; the aggregator's loses them as it's
    /// added. A record that agrees merges as before.
    fn defer(&mut self, record: &mut SourceRecord) {
        if self.registry.as_ref() != Some(&record.source) {
            for registered in &self.registered {
                if carries_doi(&record.ids, registered) && overrules(registered, record) {
                    drop_foreign(&mut record.ids, registered);
                }
            }
            return;
        }
        let claim = self.claim;
        for c in &mut self.report.candidates {
            if carries_doi(&c.ids, record) && overrules(record, &c.record) {
                drop_foreign(&mut c.ids, record);
                drop_foreign(&mut c.record.ids, record);
                c.assessment = assess(claim, &c.record, &c.ids, &c.notices);
                c.assessment.ids = judge_ids(claim, &c.ids, &c.record);
            }
        }
        if !self.registered.iter().any(|r| carries_doi(&r.ids, record)) {
            self.registered.push(record.clone());
        }
    }

    /// Adds one search's hits; returns the hits' titles.
    pub(super) fn add_search(
        &mut self,
        provider: &ProviderId,
        via: Via,
        result: Result<SearchReport, EngineError>,
    ) -> Vec<String> {
        match result {
            Ok(report) => {
                if let Some(u) = report.usage {
                    add_usage(&mut self.report.usage, provider, u);
                }
                let mut titles = Vec::new();
                for (i, hit) in report.page.hits.into_iter().enumerate() {
                    // An untitled hit (a book chapter registered under the
                    // book) points to the cited title via its container.
                    let w = &hit.record.work;
                    titles.extend(w.title.clone().or_else(|| w.container_title.clone()));
                    let sighting = Sighting {
                        provider: provider.clone(),
                        via,
                        rank: i + 1,
                        score: hit.score,
                    };
                    self.add(hit.record, sighting);
                }
                titles
            }
            Err(e) => {
                self.report.errors.push(e);
                Vec::new()
            }
        }
    }

    pub(super) fn usage(&mut self, usage: Vec<(ProviderId, Usage)>) {
        for (provider, u) in usage {
            add_usage(&mut self.report.usage, &provider, u);
        }
    }

    /// A candidate so clearly right that searching further can't improve
    /// on it: decisive, and either the identifiers the claim asserted or two
    /// providers vouch for it. A digest or reprint of a paper can match the
    /// claim exactly while only a later search finds the original. Two
    /// providers also settle a claim whose identifier names another work,
    /// but not one whose identifiers the candidate can't yet be compared
    /// with: a later provider may add the claimed PMID.
    pub(super) fn verified(&self) -> bool {
        self.report.candidates.iter().any(|c| {
            let vouched = || {
                c.sightings
                    .iter()
                    .any(|s| s.provider != c.sightings[0].provider)
            };
            let corroborated = match c.assessment.ids {
                Agreement::Match => true,
                Agreement::Mismatch => vouched(),
                _ => self.claim.ids.is_empty() && vouched(),
            };
            c.assessment.confidence >= DECISIVE && corroborated
        })
    }

    pub(super) fn finish(mut self) -> ResolveReport {
        contest(&mut self.report.candidates, self.claim);
        self.report.chosen_by_site = rank(&mut self.report.candidates, self.claim);
        self.report
    }
}

/// Scores works a caller already has against a claim, on `verify`'s terms
/// for its own candidates, offline: each field's agreement and the
/// confidence, the identifiers judged against all the work's own, and a
/// work whose claimed identifier another given work carries capped as
/// `verify` caps it (#148). In the order given, unmerged and unranked;
/// `sightings` empty.
pub fn compare(claim: &Claim, records: Vec<SourceRecord>) -> Vec<Candidate> {
    let mut candidates: Vec<Candidate> = records
        .into_iter()
        .map(|record| {
            let mut notices = Vec::new();
            merge_notices(&mut notices, &record.work.notices);
            let mut assessment = matching::assess(claim, &record);
            assessment.ids = judge_ids(claim, &record.ids, &record);
            Candidate {
                ids: record.ids.clone(),
                notices,
                sightings: Vec::new(),
                assessment,
                record,
            }
        })
        .collect();
    contest(&mut candidates, claim);
    candidates
}

/// A candidate one claimed identifier names while another is carried by a
/// different work's record (wild-3: a paper's PMID beside another paper's
/// DOI, which its lookup found): the identifiers name two works, so they
/// don't confirm this one. Its identifiers partly agree, and it is
/// plausible at most, the identifier to check (#148). Two records of one
/// work that weren't merged ([`matching::same_work`]) aren't two works.
fn contest(candidates: &mut [Candidate], claim: &Claim) {
    let carries = |c: &Candidate, id: &WorkId| c.ids.iter().any(|k| same_work_id(k, id));
    let contested: Vec<bool> = candidates
        .iter()
        .enumerate()
        .map(|(i, c)| {
            c.assessment.ids == Agreement::Match
                && claim.ids.iter().any(|id| {
                    !carries(c, id)
                        && candidates.iter().enumerate().any(|(j, other)| {
                            j != i
                                && carries(other, id)
                                && !matching::same_work(&other.record, &c.record)
                        })
                })
        })
        .collect();
    for (c, contested) in candidates.iter_mut().zip(contested) {
        if contested {
            c.assessment.ids = Agreement::Partial;
            c.assessment.confidence = c.assessment.confidence.min(matching::ID_CONFLICT_CAP);
        }
    }
}

/// The journal, volume and page of another record of the work, for a
/// record that gives none of them and isn't a preprint (arXiv's DataCite
/// record of a paper published in Physical Review Letters): the work
/// appeared there, whichever record of it is kept. And the volume or page
/// a record in the same journal leaves out (Crossref's BMC records give no
/// article number; Europe PMC's do), so a wrong one cited is seen.
fn add_place(record: &mut SourceRecord, other: &SourceRecord) {
    let same_version = record.ids.iter().any(|x| {
        matches!(x.kind(), IdKind::Doi | IdKind::Pmid)
            && other.ids.iter().any(|y| same_work_id(x, y))
    });
    let (w, o) = (&mut record.work, &other.work);
    if w.work_type == Some(WorkType::Preprint) {
        return;
    }
    // OpenAlex puts a book's chapters in their publisher's collection
    // ("BRILL eBooks", "Routledge eBooks"), not their book: a record of
    // the same version naming the book places it (holdout-6 #89), so the
    // venue cited is checked whichever record is kept.
    let collection = |c: &Option<String>| {
        c.as_deref().is_some_and(|c| {
            c.len() > 7
                && c.get(c.len() - 7..)
                    .is_some_and(|end| end.eq_ignore_ascii_case(" ebooks"))
        })
    };
    if same_version
        && collection(&w.container_title)
        && o.container_title.is_some()
        && !collection(&o.container_title)
    {
        w.container_title.clone_from(&o.container_title);
    }
    if w.container_title.is_none() && w.volume.is_none() && w.page.is_none() {
        w.container_title.clone_from(&o.container_title);
        w.volume.clone_from(&o.volume);
        w.page.clone_from(&o.page);
        return;
    }
    // A record of the same version (a DOI or PMID shared) that gives its
    // pages but not its venue (OpenAlex's of an ACM paper, "37–42" in no
    // proceedings) is in the other's, where its numbers don't contradict:
    // a venue cited is checked whichever record is kept.
    let agree = |a: &Option<String>, b: &Option<String>| match (a, b) {
        (Some(a), Some(b)) => aspero_core::publication::first_page(a)
            .eq_ignore_ascii_case(aspero_core::publication::first_page(b)),
        _ => true,
    };
    if w.container_title.is_none()
        && o.container_title.is_some()
        && same_version
        && agree(&w.volume, &o.volume)
        && agree(&w.page, &o.page)
    {
        w.container_title.clone_from(&o.container_title);
        if w.volume.is_none() {
            w.volume.clone_from(&o.volume);
        }
        return;
    }
    let same_journal = match (&w.container_title, &o.container_title) {
        (Some(a), Some(b)) => aspero_core::publication::same_journal(a, b),
        _ => false,
    };
    let same_volume = match (&w.volume, &o.volume) {
        (Some(a), Some(b)) => a.trim().eq_ignore_ascii_case(b.trim()),
        _ => true,
    };
    if same_journal && same_volume {
        if w.volume.is_none() {
            w.volume.clone_from(&o.volume);
        }
        if w.page.is_none() {
            w.page.clone_from(&o.page);
        }
    }
}

/// Every year a record reports: issued, then other editions or versions.
fn years(record: &SourceRecord) -> impl Iterator<Item = i16> + '_ {
    let w = &record.work;
    w.issued
        .map(|d| d.year())
        .into_iter()
        .chain(w.other_years.iter().copied())
}

/// Adds `pooled` years the record doesn't already report to its
/// `other_years`.
fn add_years(record: &mut SourceRecord, pooled: &[i16]) {
    let issued = record.work.issued.map(|d| d.year());
    for &y in pooled {
        if Some(y) != issued && !record.work.other_years.contains(&y) {
            record.work.other_years.push(y);
        }
    }
    record.work.other_years.sort_unstable();
}

#[cfg(test)]
mod tests {
    use aspero_core::{FieldSet, NoticeKind, Raw, Work};
    use jiff::Timestamp;

    use super::*;
    use crate::resolve::tests::titled;

    #[test]
    fn given_works_compare_on_verifys_terms() {
        // A paper's DOI beside another paper's PMID (#148): the work the
        // DOI names doesn't carry the PMID, which another given work does.
        let deep = titled(
            "doi:10.1038/nature14539",
            "Deep learning",
            &["Yann LeCun"],
            2015,
        );
        let mut other = titled("doi:10.1000/other", "Something else", &["A. Author"], 2015);
        other.ids.push("pmid:12345678".parse().unwrap());
        let mut claim = Claim::from_text("LeCun Y. Deep learning. 2015");
        claim.ids = vec![
            "doi:10.1038/nature14539".parse().unwrap(),
            "pmid:12345678".parse().unwrap(),
        ];
        let alone = compare(&claim, vec![deep.clone()]);
        assert_eq!(alone[0].assessment.ids, Agreement::Match);
        let both = compare(&claim, vec![other, deep]);
        // In the order given.
        assert_eq!(both[1].ids[0].to_string(), "doi:10.1038/nature14539");
        assert_eq!(both[1].assessment.ids, Agreement::Partial);
        assert!(both[1].assessment.confidence <= matching::ID_CONFLICT_CAP);
        assert!(both.iter().all(|c| c.sightings.is_empty()));
    }

    /// Plasticenta as a source has it (#81).
    fn plasticenta(
        source: &'static str,
        ids: &[&str],
        issued: &str,
        cited_by: Option<u64>,
        oa: Option<OpenAccess>,
    ) -> SourceRecord {
        let mut work = Work::default();
        work.title = Some("Plasticenta: First evidence of microplastics in human placenta".into());
        work.authors = vec![aspero_core::Person::literal("Antonio Ragusa")];
        work.issued = Some(issued.parse().unwrap());
        work.work_type = Some(WorkType::JournalArticle);
        work.cited_by_count = cited_by;
        work.open_access = oa;
        SourceRecord::new(
            ProviderId::from_static(source),
            ids.iter().map(|i| i.parse().unwrap()).collect(),
            work,
            FieldSet::ALL,
            Raw {
                media_type: "application/json".into(),
                body: "{}".into(),
            },
            Timestamp::UNIX_EPOCH,
        )
    }

    fn open_access(is_oa: bool, pdf: Option<&str>, license: Option<&str>) -> OpenAccess {
        let mut oa = OpenAccess::default();
        oa.is_oa = is_oa;
        oa.pdf_url = pdf.map(str::to_owned);
        oa.license = license.map(str::to_owned);
        oa
    }

    const DOI: &str = "doi:10.1016/j.envint.2020.106274";

    /// Sources answering in either order give one work the same values.
    #[test]
    fn a_merged_work_shows_the_same_values_whichever_source_wins() {
        let records = || {
            [
                plasticenta(
                    "crossref",
                    &[DOI],
                    "2021-01",
                    Some(3339),
                    Some(open_access(false, None, Some("cc-by-nc-nd"))),
                ),
                plasticenta(
                    "openalex",
                    &["openalex:W3109968655", DOI],
                    "2020-12-01",
                    Some(3794),
                    Some(open_access(
                        true,
                        Some("https://x/pdf"),
                        Some("cc-by-nc-nd"),
                    )),
                ),
                plasticenta(
                    "europepmc",
                    &["pmid:33395930", DOI],
                    "2021-01-01",
                    Some(4000),
                    None,
                ),
            ]
        };
        let claims = [
            Claim::from_text("Ragusa A. Plasticenta. Environ Int. 2021;146:106274."),
            Claim::from_text("Ragusa A. Plasticenta. 2020."),
        ];
        for claim in &claims {
            for order in [[0, 1, 2], [2, 1, 0], [1, 0, 2], [2, 0, 1]] {
                let mut acc = Accumulator {
                    claim,
                    report: ResolveReport::default(),
                    counted: Vec::new(),
                    registry: None,
                    registered: Vec::new(),
                };
                let all = records();
                for i in order {
                    let record = all[i].clone();
                    let sighting = Sighting {
                        provider: record.source.clone(),
                        via: Via::TitleSearch,
                        rank: 1,
                        score: None,
                    };
                    acc.add(record, sighting);
                }
                let [c] = &acc.report.candidates[..] else {
                    panic!("one work: {order:?}");
                };
                let w = &c.record.work;
                assert_eq!(w.cited_by_count, Some(3794), "OpenAlex's: {order:?}");
                let oa = w.open_access.as_ref().unwrap();
                assert!(oa.is_oa && oa.pdf_url.is_some(), "{order:?}");
                assert_eq!(w.issued.unwrap().to_string(), "2020-12-01", "{order:?}");
                assert!(w.other_years.contains(&2021), "{order:?}");
                assert_eq!(c.sightings.len(), 3);
            }
        }
    }

    /// An OpenAlex preprint merged in by its arXiv ID doesn't make the
    /// article's count OpenAlex's: either order shows the article's.
    #[test]
    fn a_preprints_source_does_not_decide_its_articles_count() {
        let claim = Claim::from_text("Ragusa A. Plasticenta. Environ Int. 2021;146:106274.");
        let records = [
            plasticenta(
                "crossref",
                &[DOI, "arxiv:2001.00001"],
                "2021",
                Some(10),
                None,
            ),
            plasticenta(
                "openalex",
                &["doi:10.1101/2020.07.15.198325", "arxiv:2001.00001"],
                "2019-07-15",
                Some(9),
                None,
            ),
            plasticenta(
                "europepmc",
                &[DOI, "pmid:33395930"],
                "2021",
                Some(3400),
                None,
            ),
        ];
        for order in [[0, 1, 2], [0, 2, 1]] {
            let mut acc = Accumulator {
                claim: &claim,
                report: ResolveReport::default(),
                counted: Vec::new(),
                registry: None,
                registered: Vec::new(),
            };
            for i in order {
                let record = records[i].clone();
                let sighting = Sighting {
                    provider: record.source.clone(),
                    via: Via::TitleSearch,
                    rank: 1,
                    score: None,
                };
                acc.add(record, sighting);
            }
            let [c] = &acc.report.candidates[..] else {
                panic!("one work: {order:?}");
            };
            assert_eq!(c.record.source.as_str(), "crossref", "{order:?}");
            assert_eq!(c.record.work.cited_by_count, Some(3400), "{order:?}");
        }
    }

    #[test]
    fn without_openalex_the_larger_count_and_versions_keep_their_own() {
        let mut a = plasticenta("crossref", &[DOI], "2021-01", Some(3339), None);
        let mut b = plasticenta("europepmc", &[DOI], "2021-01-04", Some(3400), None);
        merged_values(&mut a, &mut b, (false, false));
        assert_eq!(
            (a.work.cited_by_count, b.work.cited_by_count),
            (Some(3400), Some(3400))
        );
        assert_eq!(a.work.issued.unwrap().to_string(), "2021-01-04");

        // A preprint and its article (no shared DOI or PMID) keep their
        // own count and date; a free copy is still shared.
        let mut article = plasticenta(
            "crossref",
            &[DOI, "arxiv:2001.00001"],
            "2021",
            Some(10),
            None,
        );
        let mut preprint = plasticenta(
            "openalex",
            &["doi:10.1101/2020.07.15.198325", "arxiv:2001.00001"],
            "2020-07-15",
            Some(9),
            Some(open_access(true, None, None)),
        );
        merged_values(&mut article, &mut preprint, (false, true));
        assert_eq!(article.work.cited_by_count, Some(10));
        assert_eq!(article.work.issued.unwrap().to_string(), "2021");
        assert!(article.work.open_access.as_ref().is_some_and(|oa| oa.is_oa));
    }

    #[test]
    fn authors_get_the_openalex_ids_of_another_record_of_their_version() {
        // #174: a Crossref-led work still names its authors' OpenAlex ids.
        let person = |given: &str, family: &str| aspero_core::Person::new(given, family);
        let with_id = |name: &str, id: &str| {
            let mut p = aspero_core::Person::literal(name);
            p.openalex_id = Some(id.into());
            p
        };
        let mut crossref = plasticenta("crossref", &[DOI], "2021", None, None);
        crossref.work.authors = vec![
            person("Antonio", "Ragusa"),
            person("Wei", "Zhang"),
            person("Li", "Zhang"),
            person("Alessandro", "Svelato"),
        ];
        let mut openalex = plasticenta("openalex", &[DOI], "2021", None, None);
        openalex.work.authors = vec![
            with_id("Antonio Ragusa", "A1"),
            with_id("Li Zhang", "A2"),
            with_id("Wei Zhang", "A3"),
        ];
        merged_values(&mut crossref, &mut openalex, (false, true));
        let ids: Vec<_> = crossref
            .work
            .authors
            .iter()
            .map(|p| p.openalex_id.as_deref())
            .collect();
        // By name and place; the two Zhangs, out of place, by their
        // initials; Svelato isn't on the other.
        assert_eq!(ids, [Some("A1"), Some("A3"), Some("A2"), None]);

        // Records of another version (no shared DOI or PMID) keep their own.
        let mut preprint = plasticenta("openalex", &["arxiv:2001.00001"], "2020", None, None);
        preprint.work.authors = vec![with_id("Antonio Ragusa", "A1")];
        let mut article = plasticenta("crossref", &[DOI], "2021", None, None);
        article.work.authors = vec![person("Antonio", "Ragusa")];
        merged_values(&mut article, &mut preprint, (false, true));
        assert_eq!(article.work.authors[0].openalex_id, None);
    }

    #[test]
    fn the_more_informative_open_access_wins() {
        let free_pdf = Some(open_access(true, Some("https://x/pdf"), None));
        let free = Some(open_access(true, None, None));
        let licensed = Some(open_access(false, None, Some("cc-by")));
        let closed = Some(open_access(false, None, None));
        let ladder = [&free_pdf, &free, &licensed, &closed, &None].map(Option::as_ref);
        for (i, &better) in ladder.iter().enumerate() {
            for &worse in &ladder[i..] {
                assert_eq!(best_open_access(better, worse), better);
                assert_eq!(best_open_access(worse, better), better);
            }
        }
    }

    #[test]
    fn the_earliest_date_then_the_most_precise() {
        let date = |s: &str| s.parse::<PartialDate>().unwrap();
        for (a, b, want) in [
            ("2020-12-01", "2021-01", "2020-12-01"),
            ("2021-01", "2020-12-01", "2020-12-01"),
            ("2021", "2021-01-05", "2021-01-05"),
            ("2021-01-05", "2021", "2021-01-05"),
            ("2020", "2021-03-05", "2020"),
            ("2021-01", "2021-01", "2021-01"),
        ] {
            assert_eq!(earliest(date(a), date(b)).to_string(), want, "{a} {b}");
        }
    }

    fn update(kind: NoticeKind, notice: Option<&str>) -> Notice {
        let mut u = Notice::new(kind);
        u.notice = notice.map(|n| n.parse().unwrap());
        u
    }

    #[test]
    fn detailed_notices_replace_bare_flags_without_duplicates() {
        let mut known = vec![update(NoticeKind::Retraction, None)];
        let crossref = [
            update(NoticeKind::Correction, Some("10.1000/c1")),
            update(NoticeKind::Retraction, Some("10.1000/r")),
            update(NoticeKind::Correction, Some("10.1000/c2")),
        ];
        merge_notices(&mut known, &crossref);
        merge_notices(&mut known, &crossref); // again: nothing new
        merge_notices(&mut known, &[update(NoticeKind::Retraction, None)]);
        let summary: Vec<(String, Option<String>)> = known
            .iter()
            .map(|u| {
                (
                    u.kind.as_str().to_owned(),
                    u.notice.as_ref().map(ToString::to_string),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("retraction".into(), Some("doi:10.1000/r".into())),
                ("correction".into(), Some("doi:10.1000/c1".into())),
                ("correction".into(), Some("doi:10.1000/c2".into())),
            ]
        );
    }

    #[test]
    fn a_record_without_a_place_takes_its_works() {
        // arXiv's DataCite record of the LIGO paper, merged with OpenAlex's.
        let title = "Observation of Gravitational Waves from a Binary Black Hole Merger";
        let mut datacite = titled("10.48550/arxiv.1602.03837", title, &[], 2016);
        let mut openalex = titled("10.1103/physrevlett.116.061102", title, &[], 2016);
        openalex.work.container_title = Some("Physical Review Letters".into());
        openalex.work.volume = Some("116".into());
        openalex.work.page = Some("061102".into());
        let mut preprint = datacite.clone();
        preprint.work.work_type = Some(WorkType::Preprint);
        add_place(&mut datacite, &openalex);
        assert_eq!(
            datacite.work.container_title.as_deref(),
            Some("Physical Review Letters")
        );
        assert_eq!(datacite.work.volume.as_deref(), Some("116"));
        // A preprint is another version, still in no journal.
        add_place(&mut preprint, &openalex);
        assert_eq!(preprint.work.container_title, None);
        // Crossref's BMC record without its article number, Europe PMC's
        // with it (holdout-1 #508: cited at another page, verified).
        let title = "Views of health care professionals and policy-makers on the use of \
                     surveillance data to combat antimicrobial resistance";
        let mut crossref = titled("10.1186/s12889-020-8383-8", title, &[], 2020);
        crossref.work.container_title = Some("BMC Public Health".into());
        crossref.work.volume = Some("20".into());
        let mut europepmc = crossref.clone();
        europepmc.work.container_title = Some("BMC public health".into());
        europepmc.work.page = Some("279".into());
        let mut elsewhere = europepmc.clone();
        elsewhere.work.container_title = Some("Public Health Reviews".into());
        let mut kept = crossref.clone();
        add_place(&mut kept, &elsewhere);
        assert_eq!(kept.work.page, None, "another journal's page");
        add_place(&mut crossref, &europepmc);
        assert_eq!(crossref.work.page.as_deref(), Some("279"));
        // OpenAlex's record of an ACM paper gives its pages, not its
        // proceedings; Crossref's gives them (holdout-3 #894: cited at
        // another meeting, verified by OpenAlex's).
        let title = "Mobile Agents for Mobile Games";
        let mut openalex = titled("10.1145/2751496.2751500", title, &[], 2015);
        openalex.work.page = Some("37-42".into());
        let mut crossref = openalex.clone();
        crossref.work.page = None;
        crossref.work.container_title =
            Some("Proceedings of the 2nd Workshop on Mobile Gaming".into());
        let mut elsewhere = openalex.clone();
        add_place(&mut openalex, &crossref);
        assert_eq!(
            openalex.work.container_title.as_deref(),
            Some("Proceedings of the 2nd Workshop on Mobile Gaming")
        );
        // Not another version's, nor at other pages.
        let mut other_version = crossref.clone();
        other_version.ids = vec!["doi:10.1145/9999999.9999999".parse().unwrap()];
        add_place(&mut elsewhere, &other_version);
        assert_eq!(elsewhere.work.container_title, None);
        let mut moved = crossref.clone();
        moved.work.page = Some("101".into());
        add_place(&mut elsewhere, &moved);
        assert_eq!(elsewhere.work.container_title, None);
    }

    #[test]
    fn a_chapter_in_its_publishers_collection_is_in_its_book() {
        // holdout-6 #89: OpenAlex's record in "BRILL eBooks", Crossref's
        // in the book; whichever is kept, the book is the place.
        let title = "6 Life after Migration: Japanese Immigrants’ Experience of Migration";
        let mut openalex = titled("10.1163/9789004283008_007", title, &[], 2014);
        openalex.work.work_type = Some(WorkType::Chapter);
        openalex.work.container_title = Some("BRILL eBooks".into());
        openalex.work.page = Some("128-178".into());
        let mut crossref = openalex.clone();
        crossref.work.container_title = Some("Migration as Transnational Leisure".into());
        let mut other_version = openalex.clone();
        other_version.ids = vec!["doi:10.1163/9789004283008_008".parse().unwrap()];
        add_place(&mut other_version, &crossref);
        assert_eq!(
            other_version.work.container_title.as_deref(),
            Some("BRILL eBooks")
        );
        let mut kept = crossref.clone();
        add_place(&mut kept, &openalex);
        add_place(&mut openalex, &crossref);
        for r in [&openalex, &kept] {
            assert_eq!(
                r.work.container_title.as_deref(),
                Some("Migration as Transnational Leisure")
            );
        }
    }
}
