//! Citation links: a work's references, or a page of the works citing it.

use aspero_core::WorkId;

/// Citation links, added beside the versioned schema so that adding them
/// keeps the rest of the cache (a rebuild drops and recreates them too).
/// One row per work and direction: the ids it cites (`references`), or a
/// page of those citing it (`cited_by`).
pub(crate) const EDGES: &str = "
CREATE TABLE IF NOT EXISTS edges (
    work       TEXT NOT NULL,      -- the work's id (an OpenAlex work id)
    direction  TEXT NOT NULL,      -- 'references' or 'cited_by'
    ids        TEXT NOT NULL,      -- JSON list of work ids, in the source's order
    complete   INTEGER NOT NULL,   -- 1 if `ids` is every link the source has
    stored_at  INTEGER NOT NULL,   -- unix seconds
    PRIMARY KEY (work, direction)
);
";

/// Which way a citation link points from a work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Direction {
    /// The works it cites.
    References,
    /// The works citing it.
    CitedBy,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Direction::References => "references",
            Direction::CitedBy => "cited_by",
        }
    }
}

/// A work's links in one direction.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Edges {
    /// In the source's order.
    pub ids: Vec<WorkId>,
    /// Whether these are every link the source has (not a page of them).
    pub complete: bool,
}

impl Edges {
    pub fn new(ids: Vec<WorkId>, complete: bool) -> Self {
        Edges { ids, complete }
    }
}
