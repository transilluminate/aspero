//! `aspero search`: keyword search against one provider.

use aspero_core::{Cursor, SearchQuery, Sort, SourceRecord, WorkType, YearRange};
use aspero_engine::Engine;
use serde_json::{Value, json};

use super::{Failure, Out, print, print_line, shell_quote};
use crate::args::{Format, SearchOptions, SortBy};
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
            aspero_json::hit(i + 1, h.score, work)
        })
        .collect();

    match out.format {
        Format::Jsonl => hits.iter().for_each(print_line),
        Format::Table => {
            println!("{}", hits_table(&page.hits));
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
        Format::Bibtex | Format::Ris | Format::CslJson | Format::Cite => {
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
/// The hits as a table: a Score column when any hit has one (scores are
/// the provider's, and some give none), and Cited (`cited_by_count`) when
/// any work says how often it's cited.
fn hits_table(hits: &[aspero_core::Hit]) -> comfy_table::Table {
    use comfy_table::CellAlignment;
    let scored = hits.iter().any(|h| h.score.is_some());
    let counted = hits.iter().any(|h| h.record.work.cited_by_count.is_some());
    let mut headers = vec!["#"];
    if scored {
        headers.push("Score");
    }
    headers.push("Year");
    if counted {
        headers.push("Cited");
    }
    headers.extend(["Author", "Title", "ID"]);
    let mut table = output::table(&headers);
    for (i, hit) in hits.iter().enumerate() {
        let w = &hit.record.work;
        let mut row = vec![(i + 1).to_string()];
        if scored {
            row.push(hit.score.map(|s| format!("{s:.1}")).unwrap_or_default());
        }
        row.push(output::year(w));
        if counted {
            row.push(w.cited_by_count.map(output::thousands).unwrap_or_default());
        }
        row.extend([
            output::first_author(w),
            w.title.clone().unwrap_or_default(),
            output::main_id(&hit.record.ids),
        ]);
        table.add_row(row);
    }
    for (i, header) in headers.iter().enumerate() {
        if matches!(*header, "#" | "Score" | "Cited")
            && let Some(column) = table.column_mut(i)
        {
            column.set_cell_alignment(CellAlignment::Right);
        }
    }
    table
}

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
    let mut args = vec!["aspero".to_owned(), "search".to_owned()];
    // Fields alone need no keywords.
    if !text.is_empty() {
        args.push(shell_quote(text));
    }
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
    for (name, value) in [
        ("--author", &o.author),
        ("--title", &o.title),
        ("--container", &o.container),
    ] {
        if let Some(v) = value {
            flag(name, Some(v.clone()));
        }
    }
    // As given, so the next page asks the same.
    if let Some(tree) = &o.tree {
        flag("--query", Some(tree.clone()));
    }
    if o.sort != SortBy::Relevance {
        flag("--sort", Some(sort(o.sort).as_str().replace('_', "-")));
    }
    if let Some(since) = &o.since {
        flag("--since", Some(since.clone()));
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
    q.author = o.author.clone();
    q.title = o.title.clone();
    q.container = o.container.clone();
    q.tree = o.tree.as_deref().map(parse_tree).transpose()?;
    q.sort = sort(o.sort);
    q.since = o
        .since
        .as_deref()
        .map(SearchQuery::parse_since)
        .transpose()
        .map_err(|e| Failure::usage(format!("--since: {e}")))?;
    q.page.limit = o.limit;
    q.page.cursor = o.cursor.clone().map(Cursor);
    Ok(q)
}

/// `--query`'s JSON tree.
fn parse_tree(json: &str) -> Result<aspero_core::QueryNode, Failure> {
    let value: serde_json::Value = serde_json::from_str(json)
        .map_err(|e| Failure::usage(format!("--query is not JSON: {e}")))?;
    aspero_json::parse_query(&value).map_err(|e| Failure::usage(format!("--query: {e}")))
}

fn sort(by: SortBy) -> Sort {
    match by {
        SortBy::Relevance => Sort::Relevance,
        SortBy::Newest => Sort::Newest,
        SortBy::MostCited => Sort::MostCited,
    }
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
    fn tables_show_scores_only_when_given_and_citations_when_known() {
        use aspero_core::Hit;
        let mut cited = crate::cmd::testing::work(&["W1"], "Deep learning", &["Yann LeCun"], 2015);
        cited.work.cited_by_count = Some(91_234);
        let other = crate::cmd::testing::work(&["W2"], "Backprop", &["David Rumelhart"], 1986);
        // Sorted by most cited, OpenAlex gives no scores.
        let unscored = [Hit::new(cited.clone(), None), Hit::new(other.clone(), None)];
        let table = hits_table(&unscored).to_string();
        assert!(!table.contains("Score"), "{table}");
        assert!(
            table.contains("Cited") && table.contains("91,234"),
            "{table}"
        );
        let header = |t: &comfy_table::Table| -> Vec<String> {
            t.header()
                .unwrap()
                .cell_iter()
                .map(|c| c.content())
                .collect()
        };
        assert_eq!(
            header(&hits_table(&unscored)),
            ["#", "Year", "Cited", "Author", "Title", "ID"]
        );
        // Scored, without counts.
        let scored = [Hit::new(other, Some(12.5))];
        assert_eq!(
            header(&hits_table(&scored)),
            ["#", "Score", "Year", "Author", "Title", "ID"]
        );
    }

    #[test]
    fn next_page_commands_reuse_the_search_and_pin_the_provider() {
        let options = SearchOptions {
            source: None,
            year: Some("2020..".into()),
            work_type: None,
            oa: true,
            author: None,
            title: None,
            container: None,
            tree: None,
            sort: SortBy::Relevance,
            since: None,
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
        let by_fields = SearchOptions {
            year: None,
            oa: false,
            author: Some("LeCun".into()),
            container: Some("Nature".into()),
            sort: SortBy::MostCited,
            ..options
        };
        assert_eq!(
            next_command("", &by_fields, "crossref", "c", false),
            "aspero search --source crossref --author LeCun --container Nature --sort most-cited --limit 5 --cursor c"
        );
        let by_tree = SearchOptions {
            author: None,
            container: None,
            tree: Some(r#"{"title": "Bishop's"}"#.into()),
            ..by_fields
        };
        assert_eq!(
            next_command("", &by_tree, "openalex", "c", false),
            r#"aspero search --source openalex --query '{"title": "Bishop'\''s"}' --sort most-cited --limit 5 --cursor c"#
        );
        let watching = SearchOptions {
            tree: None,
            sort: SortBy::Relevance,
            since: Some("2026-11-01".into()),
            ..by_tree
        };
        assert_eq!(
            next_command("sepsis", &watching, "crossref", "c", false),
            "aspero search sepsis --source crossref --since 2026-11-01 --limit 5 --cursor c"
        );
        assert_eq!(
            query("sepsis".into(), &watching).unwrap().since,
            Some(jiff::civil::date(2026, 11, 1))
        );
        let bad = SearchOptions {
            since: Some("2026-11".into()),
            ..watching
        };
        assert_eq!(
            query("sepsis".into(), &bad).unwrap_err().message,
            "--since: invalid date \"2026-11\": expected YYYY-MM-DD"
        );
    }

    #[test]
    fn query_trees_parse_or_say_why() {
        let tree = parse_tree(r#"{"any": [{"title": "a"}, {"title": "b"}]}"#).unwrap();
        assert!(matches!(tree, aspero_core::QueryNode::Any(_)));
        let bad = |s| parse_tree(s).unwrap_err().message;
        assert!(bad("{").starts_with("--query is not JSON"));
        assert!(bad(r#"{"titel": "a"}"#).starts_with("--query: unknown query key"));
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
