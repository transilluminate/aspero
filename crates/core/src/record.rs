//! What a provider returns about one work: normalised fields, every identifier
//! it asserts, and the untouched raw response.

use std::fmt;

use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use crate::id::WorkId;
use crate::provider::ProviderId;
use crate::work::Work;

/// One provider's view of one work. Providers never merge records; the
/// engine does, keeping each `SourceRecord` as the recomputable source of
/// truth.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct SourceRecord {
    pub source: ProviderId,
    /// Every identifier the provider asserts for this work, its own native
    /// ID first. These are the edges of the identifier graph.
    pub ids: Vec<WorkId>,
    pub work: Work,
    /// The fields this record is authoritative for. A `None` inside a covered
    /// field means "the provider has no value", not "not asked". The cache
    /// serves a request from this record iff the request's fields are a subset.
    pub fields: FieldSet,
    pub raw: Raw,
    pub fetched_at: Timestamp,
    /// The works this one cites, in the source's order and by the
    /// source's own identifiers (OpenAlex work ids). `None` means unknown:
    /// the source has no reference data for the work, or doesn't carry
    /// references at all. `Some(empty)` means the source says it cites
    /// nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub references: Option<Vec<WorkId>>,
}

impl SourceRecord {
    /// `fetched_at` is passed in rather than read here: `core` doesn't touch
    /// the clock, which keeps it deterministic in tests and portable to WASM.
    pub fn new(
        source: ProviderId,
        ids: Vec<WorkId>,
        work: Work,
        fields: FieldSet,
        raw: Raw,
        fetched_at: Timestamp,
    ) -> Self {
        SourceRecord {
            source,
            ids,
            work,
            fields,
            raw,
            fetched_at,
            references: None,
        }
    }

    /// This record with the works it cites (see
    /// [`references`](Self::references)).
    pub fn with_references(mut self, references: Option<Vec<WorkId>>) -> Self {
        self.references = references;
        self
    }

    /// The provider's own identifier for the work, if it gave any.
    pub fn native_id(&self) -> Option<&WorkId> {
        self.ids.first()
    }
}

/// The provider's response body for this work, verbatim, so normalisation
/// can be improved and rerun without refetching.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Raw {
    /// e.g. `application/json`, `application/x-bibtex`.
    pub media_type: String,
    pub body: String,
}

impl Raw {
    pub fn json(value: &serde_json::Value) -> Self {
        Raw {
            media_type: "application/json".into(),
            body: value.to_string(),
        }
    }
}

/// A selectable group of [`Work`] fields. Identifiers are always included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Field {
    Title,
    Authors,
    Issued,
    Type,
    /// Container title.
    Container,
    /// Volume, issue, page and publisher.
    Publication,
    Abstract,
    Url,
    CitedByCount,
    /// Retractions, corrections and other notices.
    Updates,
    /// Where a free, legal copy is.
    OpenAccess,
}

impl Field {
    /// Every field. A slice, so adding a field is not a breaking change.
    pub const ALL: &[Field] = &[
        Field::Title,
        Field::Authors,
        Field::Issued,
        Field::Type,
        Field::Container,
        Field::Publication,
        Field::Abstract,
        Field::Url,
        Field::CitedByCount,
        Field::Updates,
        Field::OpenAccess,
    ];

    const fn bit(self) -> u16 {
        1 << self as u16
    }
}

/// A set of [`Field`]s. Serialises as a list of field names.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct FieldSet(u16);

impl FieldSet {
    pub const EMPTY: FieldSet = FieldSet(0);
    pub const ALL: FieldSet = FieldSet::of(Field::ALL);
    /// The lean default: enough to identify and cite a work.
    pub const DEFAULT: FieldSet = FieldSet::of(&[
        Field::Title,
        Field::Authors,
        Field::Issued,
        Field::Type,
        Field::Container,
    ]);

    pub const fn of(fields: &[Field]) -> FieldSet {
        let mut bits = 0;
        let mut i = 0;
        while i < fields.len() {
            bits |= fields[i].bit();
            i += 1;
        }
        FieldSet(bits)
    }

    pub const fn contains(self, field: Field) -> bool {
        self.0 & field.bit() != 0
    }

    pub const fn is_subset(self, other: FieldSet) -> bool {
        self.0 & !other.0 == 0
    }

    pub const fn union(self, other: FieldSet) -> FieldSet {
        FieldSet(self.0 | other.0)
    }

    pub const fn difference(self, other: FieldSet) -> FieldSet {
        FieldSet(self.0 & !other.0)
    }

    pub const fn intersection(self, other: FieldSet) -> FieldSet {
        FieldSet(self.0 & other.0)
    }

    pub const fn with(self, field: Field) -> FieldSet {
        FieldSet(self.0 | field.bit())
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    pub fn iter(self) -> impl Iterator<Item = Field> {
        Field::ALL
            .iter()
            .copied()
            .filter(move |f| self.contains(*f))
    }
}

impl FromIterator<Field> for FieldSet {
    fn from_iter<I: IntoIterator<Item = Field>>(iter: I) -> Self {
        iter.into_iter().fold(FieldSet::EMPTY, FieldSet::with)
    }
}

impl fmt::Debug for FieldSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_set().entries(self.iter()).finish()
    }
}

impl Serialize for FieldSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_seq(self.iter())
    }
}

impl<'de> Deserialize<'de> for FieldSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Vec::<Field>::deserialize(d)?.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subset_logic_drives_cache_hits() {
        let cached = FieldSet::DEFAULT;
        assert!(FieldSet::of(&[Field::Title, Field::Issued]).is_subset(cached));
        assert!(!FieldSet::DEFAULT.with(Field::Abstract).is_subset(cached));
        assert_eq!(
            FieldSet::DEFAULT.with(Field::Abstract).difference(cached),
            FieldSet::of(&[Field::Abstract])
        );
        assert!(FieldSet::EMPTY.is_subset(FieldSet::EMPTY));
        assert!(FieldSet::ALL.iter().eq(Field::ALL.iter().copied()));
    }

    #[test]
    fn field_set_serialises_as_names() {
        let set = FieldSet::of(&[Field::Title, Field::CitedByCount]);
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["title","cited_by_count"]"#);
        assert_eq!(serde_json::from_str::<FieldSet>(&json).unwrap(), set);
    }
}
