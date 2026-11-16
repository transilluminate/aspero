//! Reference files as `check` reads them: detected (BibTeX, RIS,
//! CSL-JSON), and each reader on its own.

#![no_main]

use aspero_bib::BibEntry;
use libfuzzer_sys::fuzz_target;

fn use_entries(entries: &[BibEntry]) {
    for entry in entries {
        let _ = entry.is_checkable();
        let read = entry.claim.as_read();
        let _ = read.reference();
        let _ = read.place();
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(src) = std::str::from_utf8(data) else {
        return;
    };
    let _ = aspero_bib::detect(src);
    if let Ok((_, entries)) = aspero_bib::read(src) {
        use_entries(&entries);
    }
    if let Ok(entries) = aspero_bib::parse(src) {
        use_entries(&entries);
    }
    if let Ok(entries) = aspero_bib::parse_ris(src) {
        use_entries(&entries);
    }
    if let Ok(entries) = aspero_bib::parse_csl_json(src) {
        use_entries(&entries);
    }
});
