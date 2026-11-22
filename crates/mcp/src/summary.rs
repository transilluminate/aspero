//! A few lines of text per result, from its JSON: for clients that show
//! text, and agents that skim before reading the structured content.

use std::fmt::Write as _;

use serde_json::Value;

/// Items listed before "… and N more".
const SHOWN: usize = 10;

pub fn of(tool: &str, v: &Value) -> String {
    match tool {
        "search" | "citing_works" | "references" => search(v),
        "search_supports" => match v["supported"].as_bool() {
            Some(true) => "Supported: the source can search this tree.".into(),
            _ => format!("Not supported: {}", str(&v["reason"])),
        },
        "lookup" => list(&v["results"], lookup_line),
        "verify" => {
            let mut text = list(&v["results"], verdict_lines);
            let verdicts = v["results"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            if verdicts.iter().any(|r| r["verdict"] == "not_found") {
                text.push_str(
                    "\nNot found means no source indexes a matching work: books, reports and \
                     grey literature are often missing, so it's not proof a citation is made up.",
                );
            }
            text
        }
        "analyse" => analysis(v),
        "identify" => format!(
            "IDENTIFIED {}\n{}",
            decided(str(&v["decided_by"])),
            verdict_lines(v)
        ),
        _ => String::new(),
    }
}

/// How `identify` decided, in words.
fn decided(by: &str) -> String {
    match by {
        "identifier_and_title" => "by an identifier on the page, confirmed by the title".into(),
        "identifier_and_page" => {
            "by an identifier on the page, confirmed by the title and first author on the page"
                .into()
        }
        "only_identifier" => {
            "by the only identifier on the page (no title to check it against)".into()
        }
        "title_search" => "by searching for the top of the page as a citation".into(),
        other => format!("by {}", other.replace('_', " ")),
    }
}

fn search(v: &Value) -> String {
    let hits = v["hits"].as_array().map(Vec::as_slice).unwrap_or_default();
    let total = v["total"]
        .as_u64()
        .map(|t| format!(" of {t}"))
        .unwrap_or_default();
    let served = match str(&v["served"]) {
        "live" => String::new(),
        s => format!(" ({s})"),
    };
    let works = match hits.len() {
        1 => "1 work".to_owned(),
        n => format!("{n} works"),
    };
    let mut lines = vec![format!(
        "{works}{total} via {}{served}:",
        str(&v["provider"])
    )];
    lines.extend(
        hits.iter()
            .take(SHOWN)
            .map(|h| format!("{}. {}{}", h["rank"], work_line(&h["work"]), held(h))),
    );
    if hits.len() > SHOWN {
        lines.push(format!("… and {} more", hits.len() - SHOWN));
    }
    if let Some(cursor) = v["next_cursor"].as_str() {
        lines.push(format!(
            "More: cursor {cursor:?} with source {:?}.",
            str(&v["provider"])
        ));
    }
    lines.join("\n")
}

/// What resolved, the estimate or each analysis' findings, and what
/// stopped any of it.
fn analysis(v: &Value) -> String {
    let seeds = v["seeds"].as_array().map(Vec::as_slice).unwrap_or_default();
    let resolved = seeds.iter().filter(|s| s["status"] == "resolved").count();
    let mut lines = vec![format!(
        "{resolved} of {} works resolved, {} with known references.",
        seeds.len(),
        v["with_references"]
    )];
    let unresolved: Vec<&Value> = seeds.iter().filter(|s| s["status"] != "resolved").collect();
    lines.extend(unresolved.iter().take(SHOWN).map(|s| {
        format!(
            "  seed {} {}: {}",
            s["index"],
            str(&s["input"]),
            str(&s["reason"])
        )
    }));
    if unresolved.len() > SHOWN {
        lines.push(format!("  … and {} more", unresolved.len() - SHOWN));
    }
    let e = &v["estimate"];
    if e.is_object() {
        let exact = if e["exact"] == true {
            "exact"
        } else {
            "approximate"
        };
        lines.push(format!(
            "Estimate ({exact}): {} paid requests, ${:.4}. Nothing was run.",
            e["requests"],
            e["cost_usd"].as_f64().unwrap_or(0.0)
        ));
    }
    let sections: [(&str, &str, Line); 4] = [
        ("retracted_references", "Retracted references", |i| {
            format!("seed {} cites {}", i["seed"], work_line(&i["cites"]))
        }),
        ("odd_one_out", "Odd ones out", |i| {
            format!(
                "seed {}: {}",
                i["seed"],
                str(&i["reason"]).replace('_', " ")
            )
        }),
        ("probably_missing", "Probably missing", |i| {
            format!(
                "{}, cited by seeds {}",
                found_line(i),
                joined(&i["cited_by"])
            )
        }),
        ("related", "Related", |i| {
            format!(
                "{}, score {}, via seeds {}",
                found_line(i),
                i["score"],
                joined(&i["seeds"])
            )
        }),
    ];
    for (key, name, line) in sections {
        let found = &v[key];
        if !found.is_object() {
            continue;
        }
        let items = found["items"]
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default();
        lines.push(match items.len() {
            0 => format!("{name}: none."),
            n => format!("{name} ({n}):"),
        });
        lines.extend(items.iter().take(SHOWN).map(|i| format!("  {}", line(i))));
        if items.len() > SHOWN {
            lines.push(format!("  … and {} more", items.len() - SHOWN));
        }
        if let Some(stops) = stopped(&found["incomplete"]) {
            lines.push(format!("  Incomplete: {stops}."));
        }
    }
    let seeds_stopped: Vec<Value> = v["incomplete"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["analysis"] == "seeds" || s["analysis"].is_null())
        .cloned()
        .collect();
    if let Some(stops) = stopped(&Value::Array(seeds_stopped)) {
        lines.push(format!("Resolving the seeds stopped: {stops}."));
    }
    if v["complete"] == false && !e.is_object() {
        let which: Vec<String> = v["incomplete"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| s["analysis"].as_str())
            .fold(Vec::new(), |mut names, n| {
                if !names.iter().any(|m| m == n) {
                    names.push(n.to_owned());
                }
                names
            });
        lines.push(if which.is_empty() {
            "Not complete: see each `incomplete`.".into()
        } else {
            format!("Not complete ({}): see `incomplete`.", which.join(", "))
        });
    }
    lines.join("\n")
}

/// An item's line.
type Line = fn(&Value) -> String;

/// "budget, 1200 of 4300; capped at citing, 200 of 900", or `None` when
/// nothing stopped.
fn stopped(incomplete: &Value) -> Option<String> {
    let stops = incomplete.as_array().filter(|s| !s.is_empty())?;
    let said: Vec<String> = stops
        .iter()
        .map(|s| {
            let stage = s["stage"]
                .as_str()
                .map(|st| format!(" at {st}"))
                .unwrap_or_default();
            format!(
                "{}{stage}, {} of {}",
                str(&s["kind"]),
                s["done"],
                s["total"]
            )
        })
        .collect();
    Some(said.join("; "))
}

/// A work an analysis found: its line, or its id when it wasn't looked up.
fn found_line(item: &Value) -> String {
    let found = match &item["work"] {
        Value::Object(_) => work_line(&item["work"]),
        _ => str(&item["id"]).to_owned(),
    };
    format!("{found}{}", held(item))
}

/// " (held)" for a work the app already holds.
fn held(item: &Value) -> &'static str {
    if item["held"] == true { " (held)" } else { "" }
}

/// Numbers in a list, comma-separated.
fn joined(items: &Value) -> String {
    let items = items.as_array().map(Vec::as_slice).unwrap_or_default();
    items
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

/// Each item's lines, the first [`SHOWN`] of them.
fn list(items: &Value, line: fn(&Value) -> String) -> String {
    let items = items.as_array().map(Vec::as_slice).unwrap_or_default();
    let mut lines: Vec<String> = items.iter().take(SHOWN).map(line).collect();
    if items.len() > SHOWN {
        lines.push(format!("… and {} more", items.len() - SHOWN));
    }
    lines.join("\n")
}

fn lookup_line(item: &Value) -> String {
    let status = str(&item["status"]);
    let detail = match status {
        "found" => work_line(&item["work"]),
        _ => item["error"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_default(),
    };
    format!("{}: {status} {detail}", str(&item["input"]))
        .trim_end()
        .to_owned()
}

/// "VERIFIED  <citation>", the reason, and the best match.
fn verdict_lines(item: &Value) -> String {
    let mut lines = vec![
        format!(
            "{}  {}",
            str(&item["verdict"]).to_uppercase().replace('_', " "),
            clip(str(&item["input"]), 120)
        ),
        format!("  {}", str(&item["reason_text"])),
    ];
    if item["best_match"].is_object() {
        lines.push(format!(
            "  match ({}): {}",
            item["best_match"]["confidence"],
            work_line(&item["best_match"]["work"])
        ));
    }
    lines.join("\n")
}

/// `Title (2015), First Author et al., doi:…`: authors as objects or, in
/// a hit, names; the year from `issued`, or `year` (an analysis' works).
fn work_line(work: &Value) -> String {
    let title = work["title"].as_str().unwrap_or("(untitled)");
    let year = match (work["issued"].as_str(), work["year"].as_i64()) {
        (Some(d), _) => Some(d[..d.len().min(4)].to_owned()),
        (None, year) => year.map(|y| y.to_string()),
    };
    let authors = work["authors"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let author = authors
        .first()
        .and_then(|a| a.as_str().or_else(|| a["name"].as_str()));
    let id = work["canonical_id"]
        .as_str()
        .or_else(|| work["ids"][0].as_str());
    let mut line = clip(title, 160);
    if let Some(year) = year {
        let _ = write!(line, " ({year})");
    }
    if let Some(author) = author {
        let _ = write!(line, ", {author}");
        if authors.len() > 1 || work["authors_more"].is_u64() {
            line.push_str(" et al.");
        }
    }
    if let Some(id) = id {
        let _ = write!(line, ", {id}");
    }
    line
}

fn str(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// At most `max` characters, with an ellipsis if cut.
fn clip(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(max) {
        Some((at, _)) => format!("{}…", &line[..at]),
        None => line,
    }
}
