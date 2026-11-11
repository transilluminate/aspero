//! `aspero search`: keyword search against one provider.

use aspero_core::{Cursor, SearchQuery, SourceRecord, WorkType, YearRange};
use aspero_engine::Engine;
use serde_json::{Value, json};

use super::{Failure, Out, print, print_line, shell_quote};
use crate::args::{Format, SearchOptions};
use crate::exit::Exit;
use crate::output;

pub async fn run(
    engine: &Engine,
    text: String,
    options: SearchOptions,
    out: Out,
) -> Result<Exit, Failure> {
    let query = query(text.clone(), &options)?;
    let report = engine.search(options.source.as_deref(), &query).await?;
    let page = &report.page;
    let next = page.next.as_ref().map(|c| {
        next_command(
            &text,
            &options,
            report.provider.as_str(),
            &c.0,
            out.abstracts,
        )
    });
    let abstracts = if out.abstracts {
        let records: Vec<&SourceRecord> = page.hits.iter().map(|h| &h.record).collect();
        Some(engine.abstracts(&records).await)
    } else {
        None
    };
    let found = |i: usize| abstracts.as_ref().map(|a| a.found[i].as_ref());
    let hits: Vec<Value> = page
        .hits
        .iter()
        .enumerate()
        .map(|(i, h)| {
            let mut work = output::work(&h.record, out.full);
            if let Some(a) = found(i) {
                output::set_abstract(&mut work, a);
            }
            json!({"rank": i + 1, "score": h.score, "work": work})
        })
        .collect();

    match out.format {
        Format::Jsonl => hits.iter().for_each(print_line),
        Format::Table => {
            let mut table = output::table(&["#", "Score", "Year", "Author", "Title", "ID"]);
            for (i, hit) in page.hits.iter().enumerate() {
                let w = &hit.record.work;
                table.add_row(vec![
                    (i + 1).to_string(),
                    hit.score.map(|s| format!("{s:.1}")).unwrap_or_default(),
                    output::year(w),
                    output::first_author(w),
                    w.title.clone().unwrap_or_default(),
                    output::main_id(&hit.record.ids),
                ]);
            }
            println!("{table}");
            for (i, hit) in page.hits.iter().enumerate() {
                if let Some(a) = found(i) {
                    println!(
                        "\n{}. {}\n{}",
                        i + 1,
                        output::main_id(&hit.record.ids),
                        output::abstract_text(a, "  ")
                    );
                }
            }
            footer(&report, next.as_deref());
        }
        Format::Text => {
            for (i, hit) in page.hits.iter().enumerate() {
                print!("{}", output::hit_text(i + 1, hit));
                if let Some(a) = found(i) {
                    println!("{}", output::abstract_text(a, "    "));
                }
                println!();
            }
            footer(&report, next.as_deref());
        }
        Format::Bibtex | Format::Ris | Format::Cite => {
            let mut export = output::Exporter::new(&out)?;
            for hit in &page.hits {
                let entry = export.entry(&hit.record.work, &hit.record.ids);
                if !entry.is_empty() {
                    println!("{entry}");
                }
            }
            print!("{}", export.finish());
        }
        Format::Json | Format::Auto => {
            let mut doc = json!({
                "provider": report.provider,
                "total": page.total,
                "next_cursor": page.next.as_ref().map(|c| &c.0),
                "next_command": next,
                "ignored_filters": report.ignored_filters,
                "served": report.served.as_str(),
                "hits": hits,
            });
            if let Some(u) = &report.usage {
                doc["usage"] = json!({ report.provider.to_string(): aspero_json::usage_one(u) });
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

/// Totals, ignored filters, paging and spend, on stderr.
fn footer(report: &aspero_engine::SearchReport, next: Option<&str>) {
    let page = &report.page;
    let total = page.total.map(|t| format!(" of {t}")).unwrap_or_default();
    let served = match report.served {
        aspero_engine::Served::Live => String::new(),
        s => format!(" ({})", s.as_str()),
    };
    eprintln!(
        "{} results{total} via {}{served}",
        page.hits.len(),
        report.provider
    );
    if !report.ignored_filters.is_empty() {
        eprintln!(
            "ignored filters (unsupported by {}): {:?}",
            report.provider, report.ignored_filters
        );
    }
    if let Some(next) = next {
        eprintln!("next page: {next}");
    }
    if let Some(u) = &report.usage
        && let Some(text) = output::usage_text(&[(report.provider.clone(), *u)])
    {
        eprintln!("{text}");
    }
}

/// The command that fetches the next page: the same search, pinned to the
/// provider that issued the cursor (cursors mean nothing elsewhere).
fn next_command(
    text: &str,
    o: &SearchOptions,
    provider: &str,
    cursor: &str,
    abstracts: bool,
) -> String {
    let mut args = vec!["aspero".to_owned(), "search".to_owned(), shell_quote(text)];
    let mut flag = |name: &str, value: Option<String>| {
        args.push(name.to_owned());
        if let Some(v) = value {
            args.push(shell_quote(&v));
        }
    };
    flag("--source", Some(provider.to_owned()));
    if let Some(year) = &o.year {
        flag("--year", Some(year.clone()));
    }
    if let Some(t) = &o.work_type {
        flag("--type", Some(t.clone()));
    }
    if o.oa {
        flag("--oa", None);
    }
    if abstracts {
        flag("--abstract", None);
    }
    flag("--limit", Some(o.limit.to_string()));
    flag("--cursor", Some(cursor.to_owned()));
    args.join(" ")
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
    fn next_page_commands_reuse_the_search_and_pin_the_provider() {
        let options = SearchOptions {
            source: None,
            year: Some("2020..".into()),
            work_type: None,
            oa: true,
            limit: 5,
            cursor: Some("old".into()),
        };
        assert_eq!(
            next_command(
                "protein folding",
                &options,
                "europepmc",
                "AoII+Y5/T0Sg==",
                false
            ),
            "aspero search 'protein folding' --source europepmc --year 2020.. --oa --limit 5 --cursor AoII+Y5/T0Sg=="
        );
        assert_eq!(
            next_command("Bishop's book", &options, "openalex", "c", true)
                .split(" --")
                .next(),
            Some(r#"aspero search 'Bishop'\''s book'"#)
        );
    }

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
