//! Article and web pages, offline: what a page says the work is, and a web
//! page as a work to cite.

use crate::Aspero;
use crate::records::{PageClaim, WebPage, Work};

#[uniffi::export]
impl Aspero {
    /// The work a publisher's article page shows, from its rendered markup
    /// (`html`: its `<head>` and JSON-LD scripts, or `nil` when the page
    /// wasn't loaded), its address, and the title the drag gave it (link
    /// text such as "Full Text" is ignored). Reads Highwire, PRISM, Dublin
    /// Core and JSON-LD metadata, then the page's title, then `title`, and
    /// the address's identifiers. Verify `claim`, or look up `ids`. A page
    /// that isn't a scholarly work's (`kind` `webPage`: no identifier,
    /// citation tags or scholarly JSON-LD) has no claim; cite its
    /// `webPage` instead (`webPageWork`). Offline.
    #[uniffi::method(default(title = None))]
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn claim_from_page(
        &self,
        html: Option<String>,
        url: String,
        title: Option<String>,
    ) -> PageClaim {
        (&aspero_core::claim_from_page(html.as_deref(), &url, title.as_deref())).into()
    }

    /// A web page as a work of type `webpage`, read on `accessed`
    /// (`2026-11-16`, the day the page was read; ignored unless a date),
    /// for `export` and `cite`: BibTeX `@misc` with `howpublished` and the
    /// access date in `note`, RIS `ELEC` with `Y2`, CSL-JSON `webpage`
    /// with `accessed` (Zotero's Web Page). Its `source` is `page`, its
    /// `ids` none. Offline.
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn web_page_work(&self, page: WebPage, accessed: String) -> Work {
        let work = page.to_core().to_work(accessed.trim().parse().ok());
        let record = aspero_core::SourceRecord::new(
            aspero_core::ProviderId::from_static("page"),
            Vec::new(),
            work,
            aspero_core::FieldSet::DEFAULT,
            aspero_core::Raw::json(&serde_json::Value::Null),
            jiff::Timestamp::now(),
        );
        Work::from_record(&record)
    }
}
