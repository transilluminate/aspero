//! Citation styles, and works formatted as a reference list, offline.

use crate::Aspero;
use crate::records::{
    self, CitationStyle, CiteFormat, CitedEntry, StyleError, StyleProject, StyleSource, Work,
};

#[uniffi::export]
impl Aspero {
    /// The bundled citation styles: APA, Vancouver, Harvard, Chicago,
    /// IEEE, Nature, AMA, MLA.
    pub fn citation_styles(&self) -> Vec<CitationStyle> {
        aspero_bib::cite::BUNDLED
            .iter()
            .map(|b| {
                let credits = b.credits();
                CitationStyle {
                    id: b.id.into(),
                    title: b.title.into(),
                    authors: credits.authors,
                    link: credits.link,
                    license: credits.license.unwrap_or_default(),
                }
            })
            .collect()
    }

    /// The project the bundled styles and locales come from (the Citation
    /// Style Language), their licence, and the locales. Offline.
    pub fn style_project(&self) -> StyleProject {
        use aspero_bib::cite;
        StyleProject {
            name: cite::PROJECT.into(),
            link: cite::PROJECT_LINK.into(),
            license: cite::LICENSE.into(),
            license_link: cite::LICENSE_LINK.into(),
            locales: cite::locales(),
            locales_link: cite::LOCALES_LINK.into(),
        }
    }

    /// Works as a reference list in `style`, one entry each, in the
    /// style's order (alphabetical, or as given). `locale` (`en-GB`) sets
    /// the language of terms like "et al."; by default, the style's own.
    /// Offline.
    #[uniffi::method(default(locale = None))]
    // Exported: UniFFI hands the arguments over by value.
    #[allow(clippy::needless_pass_by_value)]
    pub fn cite(
        &self,
        works: Vec<Work>,
        style: StyleSource,
        format: CiteFormat,
        locale: Option<String>,
    ) -> Result<Vec<CitedEntry>, StyleError> {
        use aspero_bib::cite;
        let style = match style {
            StyleSource::Bundled { id } => cite::Style::bundled(&id)?,
            StyleSource::Csl { xml } => cite::Style::from_xml(&xml)?,
        };
        let core: Vec<_> = works.iter().map(Work::to_core).collect();
        let pairs: Vec<_> = core.iter().map(|(w, ids)| (w, &ids[..])).collect();
        let markup = match format {
            CiteFormat::Text => cite::Markup::Text,
            CiteFormat::Html => cite::Markup::Html,
        };
        Ok(cite::cite(&pairs, &style, locale.as_deref(), markup)
            .into_iter()
            .map(|c| CitedEntry {
                index: records::count(c.index),
                label: c.label,
                text: c.text,
                in_text: c.in_text,
            })
            .collect())
    }

    /// The bundled styles' and locales' license (CC BY-SA 3.0) and
    /// authors, to show with formatted references.
    pub fn style_credits(&self) -> String {
        aspero_bib::cite::credits()
    }
}
