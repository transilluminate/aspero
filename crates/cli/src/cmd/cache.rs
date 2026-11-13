//! `aspero cache`: inspect or empty the answer cache.

use std::path::PathBuf;

use aspero_engine::SqliteCache;
use serde_json::json;

use super::{Failure, Out, print};
use crate::args::{CacheAction, Format};
use crate::exit::Exit;

pub fn run(path: PathBuf, action: CacheAction, out: Out) -> Result<Exit, Failure> {
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
    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    match out.format {
        Format::Text | Format::Table => println!(
            "{}\n  {} records ({} identifiers), {} not-found answers, {} searches, \
             {} works' citation links, {:.1} MB",
            path.display(),
            stats.records,
            stats.identifiers,
            stats.misses,
            stats.searches,
            stats.edges,
            bytes as f64 / 1_048_576.0
        ),
        _ => print(&json!({
            "path": path,
            "records": stats.records,
            "identifiers": stats.identifiers,
            "misses": stats.misses,
            "searches": stats.searches,
            "edges": stats.edges,
            "bytes": bytes,
        })),
    }
    Ok(Exit::Ok)
}
