//! Article and web pages: what a page says the work is
//! (`claimFromPage`), and a web page to cite as itself.

use super::{Claim, Mention, Utf16Offsets};

/// What an article page says the work is (`claimFromPage`).
#[derive(Debug, Clone, uniffi::Record)]
pub struct PageClaim {
    /// Every identifier found, each once, in priority order: pass their
    /// `id`s to `lookup` for an exact answer. Each is located in what it
    /// was read from (a tag's value, or the address); `fromUrl` only when
    /// read from the page's address.
    pub ids: Vec<Mention>,
    /// The work as the page describes it, its identifiers included, ready
    /// for `verify`; `nil` with no identifier and no title, and for a web
    /// page (`kind` `webPage`), which has no work to verify.
    pub claim: Option<Claim>,
    /// What answered: the first vocabulary, in priority order, that gave
    /// an identifier or the title; `url` when nothing did.
    pub source: PageSource,
    /// Whether the page shows a scholarly work (verify `claim`) or is a
    /// web page to cite as itself (`webPage`). Always set by
    /// `claimFromPage`; optional only so a `PageClaim` made by hand needn't
    /// give it.
    #[uniffi(default = None)]
    pub kind: Option<PageKind>,
    /// The page as a web page, to cite as itself (`webPageWork`): always
    /// for `kind` `webPage`, and for a work's page with a title too, so
    /// the page can be cited when its work isn't found.
    #[uniffi(default = None)]
    pub web_page: Option<WebPage>,
}

impl From<&aspero_core::PageClaim> for PageClaim {
    fn from(p: &aspero_core::PageClaim) -> Self {
        PageClaim {
            ids: p
                .ids
                .iter()
                .map(|id| Mention::new(&id.text, &id.mention, &mut Utf16Offsets::new(&id.text)))
                .collect(),
            claim: p.claim.as_ref().map(|c| Claim::read(c, None)),
            source: p.source.into(),
            kind: Some(p.kind.into()),
            web_page: p.web_page.as_ref().map(Into::into),
        }
    }
}

/// Whether a page shows a scholarly work or is a web page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PageKind {
    /// A scholarly work's page: it gives an identifier, citation tags or
    /// a scholarly article in JSON-LD, or is on a publisher's site; or no
    /// markup was given to tell. Verify `claim`.
    Work,
    /// Any other page with a title: a web page, cited as itself
    /// (`webPage`).
    WebPage,
}

impl From<aspero_core::PageKind> for PageKind {
    fn from(kind: aspero_core::PageKind) -> Self {
        match kind {
            aspero_core::PageKind::WebPage => PageKind::WebPage,
            // `PageKind` is non-exhaustive: a work's page is the fallback.
            _ => PageKind::Work,
        }
    }
}

/// A web page, as a citation of it needs (`claimFromPage`'s `webPage`):
/// export or cite it through `webPageWork`. Dates ISO 8601 as precise as
/// the page gives them.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct WebPage {
    /// As the page gives it, but for its site's own name.
    pub title: Option<String>,
    /// The website's name ("Wikipedia", "BBC News"), else its host.
    pub site: Option<String>,
    /// The canonical address, normalised: without its fragment or
    /// tracking parameters (`utm_*`, `fbclid`, …).
    pub url: String,
    /// `url:` and `url`: the same page shared twice is the same key.
    pub key: String,
    /// People only, in page order, as written.
    pub authors: Vec<String>,
    pub published: Option<String>,
    pub updated: Option<String>,
    /// A language tag: `en`, `en-GB`.
    pub language: Option<String>,
}

impl From<&aspero_core::WebPage> for WebPage {
    fn from(p: &aspero_core::WebPage) -> Self {
        WebPage {
            title: p.title.clone(),
            site: p.site.clone(),
            url: p.url.clone(),
            key: p.key.clone(),
            authors: p.authors.clone(),
            published: p.published.map(|d| d.to_string()),
            updated: p.updated.map(|d| d.to_string()),
            language: p.language.clone(),
        }
    }
}

impl WebPage {
    /// The core page, dates that don't parse left out.
    pub(crate) fn to_core(&self) -> aspero_core::WebPage {
        let mut p = aspero_core::WebPage::default();
        p.title.clone_from(&self.title);
        p.site.clone_from(&self.site);
        p.url.clone_from(&self.url);
        p.key.clone_from(&self.key);
        p.authors.clone_from(&self.authors);
        p.published = self.published.as_deref().and_then(|d| d.parse().ok());
        p.updated = self.updated.as_deref().and_then(|d| d.parse().ok());
        p.language.clone_from(&self.language);
        p
    }
}

/// Where a page's claim came from, in priority order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PageSource {
    /// Highwire Press `citation_*` meta tags.
    Highwire,
    /// PRISM `prism.*` meta tags.
    Prism,
    /// Dublin Core `dc.*` / `dcterms.*` meta tags.
    DublinCore,
    /// schema.org JSON-LD.
    JsonLd,
    /// The page's `og:title` or `<title>`.
    Title,
    /// The title passed in (a dragged link's name).
    TitleHint,
    /// The page's address.
    Url,
}

impl From<aspero_core::PageSource> for PageSource {
    fn from(source: aspero_core::PageSource) -> Self {
        use aspero_core::PageSource as Core;
        match source {
            Core::Highwire => PageSource::Highwire,
            Core::Prism => PageSource::Prism,
            Core::DublinCore => PageSource::DublinCore,
            Core::JsonLd => PageSource::JsonLd,
            Core::Title => PageSource::Title,
            Core::TitleHint => PageSource::TitleHint,
            // `PageSource` is non-exhaustive: the address is the fallback.
            _ => PageSource::Url,
        }
    }
}
