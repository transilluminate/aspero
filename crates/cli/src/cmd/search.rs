//! `aspero search`: keyword search against one provider.

use aspero_core::{Cursor, SearchQuery, WorkType, YearRange};
use aspero_engine::Engine;
use serde_json::{Value, json};

use super::{Failure, Out, print, print_line};
use crate::args::{Format, SearchOptions};
use crate::exit::Exit;
use crate::output;

pub async fn run(
    engine: &Engine,
    text: String,
    options: SearchOptions,
    out: Out,
) -> Result<Exit, Failure> {
    let query = query(text, &options)?;
    let report = engine.search(options.source.as_deref(), &query).await?;
    let page = &report.page;
    let hits: Vec<Value> = page
        .hits
        .iter()
        .enumerate()
        .map(|(i, h)| {
            json!({"rank": i + 1, "score": h.score, "work": output::work(&h.record, out.full)})
        })
        .collect();

    match out.format {
        Format::Jsonl => hits.iter().for_each(print_line),
        Format::Table => {
            for (i, hit) in page.hits.iter().enumerate() {
                println!("{}", output::hit_text(i + 1, hit));
            }
            let total = page.total.map(|t| format!(" of {t}")).unwrap_or_default();
            eprintln!("{} results{total} via {}", page.hits.len(), report.provider);
            if !report.ignored_filters.is_empty() {
                eprintln!(
                    "ignored filters (unsupported by {}): {:?}",
                    report.provider, report.ignored_filters
                );
            }
            if let Some(next) = &page.next {
                eprintln!("more: --cursor {}", next.0);
            }
            if let Some(u) = &report.usage
                && let Some(text) = output::usage_text(&[(report.provider.clone(), *u)])
            {
                eprintln!("{text}");
            }
        }
        Format::Json | Format::Auto => {
            let mut doc = json!({
                "provider": report.provider,
                "total": page.total,
                "next_cursor": page.next.as_ref().map(|c| &c.0),
                "ignored_filters": report.ignored_filters,
                "hits": hits,
            });
            if let Some(u) = &report.usage {
                doc["usage"] = json!({ report.provider.to_string(): output::usage_one(u) });
            }
            print(&doc);
        }
    }
    Ok(if page.hits.is_empty() {
        Exit::NotFound
    } else {
        Exit::Ok
    })
}

fn query(text: String, o: &SearchOptions) -> Result<SearchQuery, Failure> {
    let mut q = SearchQuery::new(text);
    q.years = o.year.as_deref().map(parse_years).transpose()?;
    q.work_type = o.work_type.as_deref().map(WorkType::from_csl);
    q.open_access = o.oa.then_some(true);
    q.page.limit = o.limit;
    q.page.cursor = o.cursor.clone().map(Cursor);
    Ok(q)
}

/// `2017`, `2017..2020`, `2017..` or `..2020`.
fn parse_years(s: &str) -> Result<YearRange, Failure> {
    let year = |t: &str| -> Result<Option<i16>, Failure> {
        let t = t.trim();
        if t.is_empty() {
            return Ok(None);
        }
        t.parse()
            .map(Some)
            .map_err(|_| Failure::usage(format!("invalid year {t:?} in --year {s:?}")))
    };
    let (from, to) = match s.split_once("..") {
        Some((from, to)) => (year(from)?, year(to)?),
        None => {
            let y = year(s)?;
            (y, y)
        }
    };
    if from.is_none() && to.is_none() {
        return Err(Failure::usage(format!("--year {s:?} names no year")));
    }
    Ok(YearRange { from, to })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn year_ranges() {
        let r = |s| {
            parse_years(s)
                .map(|y| (y.from, y.to))
                .map_err(|f| f.message)
        };
        assert_eq!(r("2017"), Ok((Some(2017), Some(2017))));
        assert_eq!(r("2017..2020"), Ok((Some(2017), Some(2020))));
        assert_eq!(r("2017.."), Ok((Some(2017), None)));
        assert_eq!(r("..2020"), Ok((None, Some(2020))));
        assert!(r("..").is_err());
        assert!(r("twenty").is_err());
    }
}
