//! `aspero cache`: inspect or empty the answer cache.

use std::path::PathBuf;

use aspero_engine::SqliteCache;
use serde_json::json;

use super::{Failure, Out, print};
use crate::args::{CacheAction, Format};
use crate::exit::Exit;

/// `max_bytes`: the size limit `--cache-max-mb` sets, shown beside the
/// size (stats doesn't evict: the next write does).
pub fn run(
    path: PathBuf,
    action: CacheAction,
    max_bytes: Option<u64>,
    out: Out,
) -> Result<Exit, Failure> {
    if let CacheAction::Path = action {
        match out.format {
            Format::Text | Format::Table => println!("{}", path.display()),
            _ => print(&json!({"path": path})),
        }
        return Ok(Exit::Ok);
    }
    let cache = SqliteCache::open(&path).map_err(|e| Failure {
        exit: Exit::Internal,
        message: format!("could not open cache {}: {e}", path.display()),
    })?;
    let failed = |e: aspero_engine::StoreError| Failure {
        exit: Exit::Internal,
        message: e.to_string(),
    };
    if let CacheAction::Clear = action {
        cache.clear().map_err(failed)?;
    }
    let stats = cache.stats().map_err(failed)?;
    let mb = |bytes: u64| bytes as f64 / 1_000_000.0;
    let limit = max_bytes.map_or(String::new(), |m| format!(" of {:.0} MB", mb(m)));
    match out.format {
        Format::Text | Format::Table => println!(
            "{}\n  {} records ({} identifiers), {} not-found answers, {} searches, \
             {} works' citation links, {:.1} MB{limit}",
            path.display(),
            stats.records,
            stats.identifiers,
            stats.misses,
            stats.searches,
            stats.edges,
            mb(stats.bytes)
        ),
        _ => print(&json!({
            "path": path,
            "records": stats.records,
            "identifiers": stats.identifiers,
            "misses": stats.misses,
            "searches": stats.searches,
            "edges": stats.edges,
            "bytes": stats.bytes,
            "max_bytes": max_bytes,
        })),
    }
    Ok(Exit::Ok)
}
