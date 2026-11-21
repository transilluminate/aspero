//! Free-text claims scored against a record cut from the claim itself:
//! its title, authors and journal are pieces of the citation (shifted,
//! with words added or dropped), so the title-matching paths (main
//! title and subtitle, title runs, the layout reader) are reached far
//! more often than with an unrelated record (`claim`). The 0.38.3 panic
//! (`subtitle_of`) needed a record title that the text nearly repeats.
//!
//! The input: eight control bytes, then the citation as UTF-8.

#![no_main]

use aspero_core::matching::{
    assess, given_title, probable_title, search_terms, shared_title, title_similarity,
    whole_title_similarity,
};
use aspero_core::provider::ProviderId;
use aspero_core::record::{FieldSet, Raw};
use aspero_core::work::{PartialDate, Person, Work};
use aspero_core::{Claim, SourceRecord};
use libfuzzer_sys::fuzz_target;

/// The words of `text`, by byte range.
fn words(text: &str) -> Vec<&str> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .filter(|w| !w.is_empty())
        .collect()
}

/// Words `from..from + len` (wrapping), joined by spaces.
fn piece(words: &[&str], from: u8, len: u8) -> Option<String> {
    if words.is_empty() || len == 0 {
        return None;
    }
    let from = usize::from(from) % words.len();
    let len = usize::from(len % 16).max(1);
    Some(
        words[from..(from + len).min(words.len())]
            .join(" ")
            .to_string(),
    )
}

const ADDED: [&str; 8] = ["The", "A", "of", "and", ":", "—", "-", "in"];

fn record(text: &str, c: &[u8; 8]) -> SourceRecord {
    let words = words(text);
    let mut work = Work::default();
    let mut title = piece(&words, c[0], c[1]);
    // Change the title a little: a joining word or a separator before or
    // after it, or its first word dropped.
    if let Some(t) = &mut title {
        let added = ADDED[usize::from(c[2] >> 5)];
        match c[2] & 3 {
            1 => *t = format!("{added} {t}"),
            2 => t.push_str(&format!(
                " {added} {}",
                words[usize::from(c[3]) % words.len()]
            )),
            3 => {
                if let Some((_, rest)) = t.split_once(' ') {
                    *t = rest.to_string();
                }
            }
            _ => {}
        }
    }
    work.title = title;
    work.authors = (0..c[4] % 6)
        .filter_map(|k| {
            let w = words.get((usize::from(c[5]) + usize::from(k) * 3) % words.len().max(1))?;
            Some(if c[4] & 0x80 == 0 {
                Person::literal(*w)
            } else {
                Person::literal(format!("{}, {}", w, &w[..w.chars().next()?.len_utf8()]))
            })
        })
        .collect();
    work.container_title = piece(&words, c[6], c[7] % 5);
    work.issued = text
        .split(|ch: char| !ch.is_ascii_digit())
        .find(|d| d.len() == 4)
        .and_then(|d| d.parse().ok())
        .map(PartialDate::from_year);
    work.volume = piece(&words, c[3], 1);
    SourceRecord::new(
        ProviderId::from_static("fuzz"),
        Vec::new(),
        work,
        FieldSet::ALL,
        Raw {
            media_type: "application/json".into(),
            body: "{}".into(),
        },
        jiff::Timestamp::UNIX_EPOCH,
    )
}

fuzz_target!(|data: &[u8]| {
    let Some((control, text)) = data.split_first_chunk::<8>() else {
        return;
    };
    let Ok(text) = std::str::from_utf8(text) else {
        return;
    };
    let record = record(text, control);
    let claim = Claim::from_text(text);
    for read in [claim.as_read(), claim.as_read_beside(Some(&record.work))] {
        let assessed = assess(&read, &record);
        assert!(
            (0.0..=1.0).contains(&assessed.confidence),
            "confidence {} for {text:?} against {:?}",
            assessed.confidence,
            record.work.title
        );
    }
    let candidates = aspero_engine::compare(&claim, vec![record.clone()]);
    assert_eq!(candidates.len(), 1);
    let _ = given_title(text);
    let _ = probable_title(text);
    let _ = search_terms(text);
    if let Some(title) = &record.work.title {
        let _ = shared_title(title, text);
        for s in [
            title_similarity(title, text),
            whole_title_similarity(title, text),
        ] {
            assert!(
                (0.0..=1.0).contains(&s),
                "similarity {s} for {title:?}, {text:?}"
            );
        }
    }
});
