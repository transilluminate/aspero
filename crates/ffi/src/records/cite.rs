//! Citation styles: the bundled ones and their credits, and works
//! formatted as a reference list (`cite`).

/// A bundled citation style.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CitationStyle {
    /// How `StyleSource.bundled` names it: `apa`, `vancouver`, ….
    pub id: String,
    pub title: String,
    /// Who wrote it: its authors, then its contributors.
    #[uniffi(default = [])]
    pub authors: Vec<String>,
    /// Its page in the Zotero Style Repository.
    #[uniffi(default = None)]
    pub link: Option<String>,
    /// Its licence, as an SPDX id: `CC-BY-SA-3.0`.
    #[uniffi(default = "CC-BY-SA-3.0")]
    pub license: String,
}

/// Where the bundled styles and locales come from, and their licence,
/// to credit them (`CitationStyle` has each style's authors).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct StyleProject {
    /// `Citation Style Language`.
    pub name: String,
    pub link: String,
    /// An SPDX id: `CC-BY-SA-3.0`.
    pub license: String,
    pub license_link: String,
    /// The bundled locales' codes: `en-US`, `en-GB`.
    pub locales: Vec<String>,
    pub locales_link: String,
}

/// Which citation style to format in.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Enum)]
pub enum StyleSource {
    /// A bundled style, by `CitationStyle.id`.
    Bundled { id: String },
    /// An independent style's `.csl` file, as text.
    Csl { xml: String },
}

/// Plain text, or HTML marking italics, bold and small caps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum CiteFormat {
    Text,
    Html,
}

/// One work in a formatted reference list.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CitedEntry {
    /// The work's position in the input.
    pub index: u32,
    /// The style's number or label for it ("1.", "\[1\]"), when the style
    /// sets it apart from the reference.
    pub label: Option<String>,
    pub text: String,
    /// How the style cites it in running text: "(LeCun et al., 2015)",
    /// "(1)".
    pub in_text: String,
}

/// Why a style couldn't be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum StyleError {
    /// No bundled style has this id.
    #[error("no bundled style {id:?}")]
    UnknownStyle { id: String },
    /// A dependent style (a journal's alias): use its `parent` style, by
    /// its id in the CSL repository (`nlm-citation-sequence`).
    #[error("a dependent style; use its parent, {parent}")]
    Dependent { parent: String },
    /// Not a CSL style, or a broken one.
    #[error("not a CSL style: {message}")]
    Invalid { message: String },
}

impl From<aspero_bib::cite::StyleError> for StyleError {
    fn from(e: aspero_bib::cite::StyleError) -> Self {
        use aspero_bib::cite::StyleError as E;
        match e {
            E::Unknown { id } => StyleError::UnknownStyle { id },
            E::Dependent { parent } => StyleError::Dependent { parent },
            E::Invalid { message } => StyleError::Invalid { message },
            _ => StyleError::Invalid {
                message: e.to_string(),
            },
        }
    }
}
