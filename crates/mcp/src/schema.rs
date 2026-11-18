//! The tools' output schemas: aspero's JSON shapes (`aspero_json`), as the
//! CLI prints them, plus each call's usage and the session's spend.
//!
//! Written by hand, since the shapes are built as JSON rather than typed.
//! They name the keys agents rely on and leave the rest open
//! (`additionalProperties`): `full` adds every known field. Self-contained
//! (no `$ref`), so any client's validator reads them. Every tool's list
//! costs an agent's context, so a shape that recurs (a work, the session)
//! names its keys in a description, and a second copy points at the first.

use serde_json::{Map, Value, json};

fn object(properties: &Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn nullable(kind: &str) -> Value {
    json!({"type": [kind, "null"]})
}

/// A work: lean by default, every known field with `full`. Its keys are
/// named in the description rather than spelled out as properties: the
/// same shape recurs in nearly every tool, and agents read the keys off the
/// result.
fn work() -> Value {
    json!({
        "type": "object",
        "description": "a work: source (the provider), ids (doi:…, pmid:…, …), canonical_id \
            (dedupe on it), title, authors [{name, orcid}], authors_more (left out of a lean \
            list), issued, type (CSL), container (the journal, or the book for a chapter), \
            cited_by_count, url, notices (retraction, correction, …), notice_for (for a \
            notice, the works it updates), open_access",
        "required": ["source", "ids"],
    })
}

/// A scored candidate for a citation.
fn candidate() -> Value {
    object(
        &json!({
            "confidence": {"type": "number", "description": "0 to 1"},
            "checks": {"type": "object", "description": "per field: match, partial, mismatch or unknown"},
            "found_by": {"type": "array", "items": {"type": "object"}},
            "work": work(),
        }),
        &["confidence", "checks", "work"],
    )
}

/// One verification, after the claim's echo.
fn verification() -> Value {
    object(
        &json!({
            "input": {"type": "string", "description": "the citation as given"},
            "claim_key": {"description": "your key for the citation, echoed"},
            "claim": {"type": "object", "description": "the citation as understood"},
            "verdict": {
                "type": "string",
                "enum": ["verified", "plausible", "mismatch", "not_found", "unverifiable", "invalid"],
            },
            "reason_text": {"type": "string", "description": "the reason in one English sentence"},
            "reason": {"type": "object", "description": "the reason as data: code, and the fields that differ"},
            "read": {"type": "object"},
            "best_match": candidate(),
            "errors": {"type": "array", "items": {"type": "string"}, "description": "providers that failed"},
            "error_kinds": {"type": "array", "items": {"type": "string"}},
            "candidates": {
                "type": "array",
                "items": {"type": "object"},
                "description": "with full: every candidate, shaped as best_match",
            },
        }),
        &["input", "verdict", "reason_text"],
    )
}

/// What each provider's calls cost: `{provider: {cost_usd, remaining_usd,
/// resets_at}}`, named in the description rather than spelled out.
fn usage() -> Value {
    json!({
        "type": "object",
        "description": "per provider, in USD: {provider: {cost_usd, remaining_usd, resets_at}}",
    })
}

fn session() -> Value {
    json!({
        "type": "object",
        "description": "the session's spend, this call included: spent_usd, budget_usd, left_usd",
        "required": ["spent_usd", "budget_usd", "left_usd"],
    })
}

/// `properties` plus `usage` and `session`, as an output schema.
fn output(mut properties: Value, required: &[&str]) -> Map<String, Value> {
    properties["usage"] = usage();
    properties["session"] = session();
    let mut required = required.to_vec();
    required.extend(["usage", "session"]);
    match object(&properties, &required) {
        Value::Object(map) => map,
        _ => unreachable!("an object"),
    }
}

pub fn search() -> Map<String, Value> {
    output(
        json!({
            "provider": {"type": "string", "description": "the source that answered"},
            "total": nullable("integer"),
            "next_cursor": {
                "type": ["string", "null"],
                "description": "pass as cursor, with source set to provider, for the next page",
            },
            "ignored_filters": {"type": "array", "items": {"type": "string"}},
            "served": {"type": "string", "enum": ["live", "cache", "stale"]},
            "hits": {
                "type": "array",
                "items": object(
                    &json!({
                        "rank": {"type": "integer"},
                        "score": {"type": "number", "description": "the source's own, when it gives one; compare only within one search"},
                        "work": work(),
                    }),
                    &["rank", "work"],
                ),
            },
        }),
        &["provider", "served", "hits"],
    )
}

pub fn supports() -> Map<String, Value> {
    output(
        json!({
            "supported": {"type": "boolean"},
            "reason": {"type": "string", "description": "why not, when not"},
        }),
        &["supported"],
    )
}

pub fn lookup() -> Map<String, Value> {
    let item = object(
        &json!({
            "input": {"type": "string"},
            "id": {"type": "string", "description": "the identifier, canonical"},
            "status": {
                "type": "string",
                "enum": ["found", "not_found", "unsupported", "not_cached", "failed", "invalid"],
            },
            "served": {"type": "string", "enum": ["cache", "stale"]},
            "work": work(),
            "error": {"type": "string"},
            "attempts": {"type": "array", "items": {"type": "object"}, "description": "each provider asked, when not found (or with full)"},
            "found_with_trailing_punctuation": {
                "type": "boolean",
                "description": "the DOI missed with its trailing punctuation stripped, and was found with it kept",
            },
            "latest": {"type": "object", "description": "a superseded work's latest version, shaped as work"},
            "disagreements": {"type": "array", "items": {"type": "object"}},
        }),
        &["input", "status"],
    );
    output(
        json!({"results": {"type": "array", "items": item, "description": "one per identifier, in order"}}),
        &["results"],
    )
}

pub fn verify() -> Map<String, Value> {
    output(
        json!({"results": {"type": "array", "items": verification(), "description": "one per citation, in order"}}),
        &["results"],
    )
}

pub fn identify() -> Map<String, Value> {
    let mut properties = verification()["properties"].clone();
    properties["identified"] = json!({"const": true});
    properties["decided_by"] = json!({
        "type": "string",
        "enum": ["identifier_and_title", "identifier_and_page", "only_identifier", "title_search"],
    });
    output(
        properties,
        &[
            "input",
            "verdict",
            "reason_text",
            "identified",
            "decided_by",
        ],
    )
}
