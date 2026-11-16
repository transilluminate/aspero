//! Word documents as `check` reads them. The first byte picks the input:
//! even, the rest is the .docx file itself (the zip); odd, the rest is
//! `word/document.xml` (then, after a NUL byte, `word/footnotes.xml`),
//! zipped here, so mutations reach the XML and the citation fields' JSON
//! past the zip's checksums.

#![no_main]

use std::io::{Cursor, Write};

use libfuzzer_sys::fuzz_target;

fn zipped(document: &[u8], footnotes: Option<&[u8]>) -> Vec<u8> {
    let mut out = Vec::new();
    let mut zip = zip::ZipWriter::new(Cursor::new(&mut out));
    let stored =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    zip.start_file("word/document.xml", stored).unwrap();
    zip.write_all(document).unwrap();
    if let Some(notes) = footnotes {
        zip.start_file("word/footnotes.xml", stored).unwrap();
        zip.write_all(notes).unwrap();
    }
    zip.finish().unwrap();
    out
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let file = if mode & 1 == 0 {
        rest.to_vec()
    } else {
        match rest.iter().position(|&b| b == 0) {
            Some(at) => zipped(&rest[..at], Some(&rest[at + 1..])),
            None => zipped(rest, None),
        }
    };
    let Ok(doc) = aspero_bib::docx::read_docx(&file) else {
        return;
    };
    for entry in &doc.entries {
        let _ = entry.claim.as_read();
    }
    let _ = aspero_core::split_references(&doc.text);
});
