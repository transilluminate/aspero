//! A few lines of text per result, from its JSON: for clients that show
//! text, and agents that skim before reading the structured content.

use serde_json::Value;

/// Items listed before "… and N more".
const SHOWN: usize = 10;

pub fn of(tool: &str, v: &Value) -> String {
    match tool {
        "search" | "cites" | "cited_by" => search(v),
        "search_supports" => match v["supported"].as_bool() {
            Some(true) => "Supported: the source can search this tree.".into(),
            _ => format!("Not supported: {}", str(&v["reason"])),
        },
        "lookup" => list(&v["results"], lookup_line),
        "verify" => list(&v["results"], verdict_lines),
        "identify" => format!(
            "IDENTIFIED by {}\n{}",
            str(&v["decided_by"]).replace('_', " "),
            verdict_lines(v)
        ),
        _ => String::new(),
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
            .map(|h| format!("{}. {}", h["rank"], work_line(&h["work"]))),
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

/// "VERIFIED  <citation>", the reason, and the match.
fn verdict_lines(item: &Value) -> String {
    let mut lines = vec![
        format!(
            "{}  {}",
            str(&item["verdict"]).to_uppercase().replace('_', " "),
            clip(str(&item["input"]), 120)
        ),
        format!("  {}", str(&item["reason"])),
    ];
    if item["match"].is_object() {
        lines.push(format!(
            "  match ({}): {}",
            item["match"]["confidence"],
            work_line(&item["match"]["work"])
        ));
    }
    lines.join("\n")
}

/// `Title (2015), First Author et al., doi:…`
fn work_line(work: &Value) -> String {
    let title = work["title"].as_str().unwrap_or("(untitled)");
    let year = work["issued"].as_str().map(|d| &d[..d.len().min(4)]);
    let authors = work["authors"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let author = authors.first().and_then(|a| a["name"].as_str());
    let id = work["canonical"]
        .as_str()
        .or_else(|| work["ids"][0].as_str());
    let mut line = clip(title, 160);
    if let Some(year) = year {
        line.push_str(&format!(" ({year})"));
    }
    if let Some(author) = author {
        line.push_str(&format!(", {author}"));
        if authors.len() > 1 {
            line.push_str(" et al.");
        }
    }
    if let Some(id) = id {
        line.push_str(&format!(", {id}"));
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
