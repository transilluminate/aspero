//! Bibliography files and Word documents read, and works exported, offline.

use crate::Aspero;
use crate::records::{Bibliography, DocxReferences, ExportFormat, ReadError, Work};

#[uniffi::export]
impl Aspero {
    /// A BibTeX/BibLaTeX, RIS or CSL-JSON file's entries, each a claim to
    /// verify; the format is told from the contents. Offline.
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn read_references(&self, text: String) -> Result<Bibliography, ReadError> {
        let (format, entries) = aspero_bib::read(&text)?;
        Ok(Bibliography {
            format: format.into(),
            entries: entries.into_iter().map(Into::into).collect(),
        })
    }

    /// The citations Zotero or Mendeley keep in a Word document (.docx),
    /// each work once as a claim to verify, and the document's text.
    /// Throws `ReadError.Unrecognised` for anything but a .docx file.
    /// Offline.
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn read_docx(&self, data: Vec<u8>) -> Result<DocxReferences, ReadError> {
        Ok(aspero_bib::docx::read_docx(&data)?.into())
    }

    /// Works as BibTeX entries (keys kept distinct) or RIS records, one
    /// after another, or as a CSL-JSON array (keyed as BibTeX). Offline.
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn export(&self, works: Vec<Work>, format: ExportFormat) -> String {
        if format == ExportFormat::CslJson {
            let core: Vec<_> = works.iter().map(Work::to_core).collect();
            let pairs: Vec<_> = core.iter().map(|(w, ids)| (w, &ids[..])).collect();
            return aspero_bib::csl_json(&pairs);
        }
        let mut keys = aspero_bib::Keys::default();
        let entries: Vec<String> = works
            .iter()
            .map(|work| {
                let (w, ids) = work.to_core();
                match format {
                    ExportFormat::Ris => aspero_bib::ris(&w, &ids),
                    ExportFormat::Bibtex => aspero_bib::bibtex(&w, &ids, &keys.key(&w)),
                    ExportFormat::CslJson => unreachable!("written above"),
                }
            })
            .collect();
        entries.join("\n")
    }
}
