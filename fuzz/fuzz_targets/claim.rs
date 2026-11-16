//! Claims read from free text, as `verify` reads a citation: the cited
//! journal, locator, years and identifiers, and scoring against a record.
//! The input is the citation, then (after a NUL byte, if any) a record
//! read with `arbitrary`. Each line of the citation is also read as a
//! `verify` stdin line (a JSON object or text).

#![no_main]

use arbitrary::{Arbitrary, Unstructured};
use aspero_core::provider::ProviderId;
use aspero_core::record::{FieldSet, Raw};
use aspero_core::work::{PartialDate, Person, Work};
use aspero_core::{Claim, SourceRecord, WorkId};
use libfuzzer_sys::fuzz_target;

#[path = "common.rs"]
mod common;

#[derive(Arbitrary, Debug)]
struct Record<'a> {
    title: Option<&'a str>,
    authors: Vec<&'a str>,
    year: Option<i16>,
    container: Option<&'a str>,
    volume: Option<&'a str>,
    page: Option<&'a str>,
    ids: Vec<&'a str>,
}

impl Record<'_> {
    fn source_record(&self) -> SourceRecord {
        let mut work = Work::default();
        work.title = self.title.map(Into::into);
        work.authors = self.authors.iter().map(|a| Person::literal(*a)).collect();
        work.issued = self.year.map(PartialDate::from_year);
        work.container_title = self.container.map(Into::into);
        work.volume = self.volume.map(Into::into);
        work.page = self.page.map(Into::into);
        let raw = Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        };
        SourceRecord::new(
            ProviderId::from_static("fuzz"),
            self.ids
                .iter()
                .filter_map(|i| i.parse::<WorkId>().ok())
                .collect(),
            work,
            FieldSet::ALL,
            raw,
            jiff::Timestamp::UNIX_EPOCH,
        )
    }
}

fn read(claim: &Claim) {
    for id in &claim.ids {
        common::assert_round_trips(id);
    }
    let read = claim.as_read();
    let _ = read.place();
    let _ = read.reference();
    let _ = claim.cited_years();
    let _ = claim.cited_container();
    let _ = claim.site_prefixes();
}

fuzz_target!(|data: &[u8]| {
    let (text, rest) = match data.iter().position(|&b| b == 0) {
        Some(at) => (&data[..at], &data[at + 1..]),
        None => (data, &[][..]),
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let claim = Claim::from_input(text);
    read(&claim);
    let from_text = Claim::from_text(text);
    read(&from_text);
    for line in text.lines() {
        if let Ok(claim) = aspero_json::InputClaim::from_line(line).claim {
            read(&claim);
        }
    }
    if let Ok(record) = Record::arbitrary_take_rest(Unstructured::new(rest)) {
        let record = record.source_record();
        let assessed = aspero_core::matching::assess(&claim.as_read(), &record);
        assert!(
            (0.0..=1.0).contains(&assessed.confidence),
            "confidence {} for {text:?}",
            assessed.confidence
        );
        let _ = aspero_core::matching::assess(&from_text, &record);
    }
});
