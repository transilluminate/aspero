//! The tools' output schemas: aspero's JSON shapes (`aspero_json`), as the
//! CLI prints them, plus each call's usage and the session's spend.
//!
//! Written by hand, since the shapes are built as JSON rather than typed.
//! They name the keys agents rely on and leave the rest open
//! (`additionalProperties`): `full` adds every known field. Self-contained
//! (no `$ref`), so any client's validator reads them.

use serde_json::{Map, Value, json};

fn object(properties: Value, required: &[&str]) -> Value {
    json!({
        "type": "object",
        "properties": properties,
        "required": required,
    })
}

fn nullable(kind: &str) -> Value {
    json!({"type": [kind, "null"]})
}

/// A work: lean by default, every known field with `full`.
fn work() -> Value {
    object(
        json!({
            "source": {"type": "string", "description": "the provider whose record this is"},
            "ids": {"type": "array", "items": {"type": "string"}, "description": "every identifier, prefixed: doi:…, arxiv:…, pmid:…"},
            "canonical": {
                "type": ["string", "null"],
                "description": "the work's key, the same whichever identifier found it: dedupe on it",
            },
            "title": {"type": "string"},
            "authors": {
                "type": "array",
                "items": object(json!({"name": {"type": "string"}, "orcid": {"type": "string"}}), &["name"]),
            },
            "issued": {"type": "string", "description": "the publication date: 2015, 2015-05 or 2015-05-27"},
            "type": {"type": "string", "description": "a CSL type"},
            "container": {"type": "string", "description": "the journal, or the book for a chapter"},
            "cited_by_count": {"type": "integer"},
            "url": {"type": "string"},
            "updates": {
                "type": "array",
                "description": "notices about the work: retraction, withdrawal, expression_of_concern, correction, new_version (superseded)",
                "items": {"type": "object"},
            },
            "update_to": {"type": "array", "description": "for a notice, the works it updates", "items": {"type": "object"}},
            "open_access": {"type": "object", "description": "whether a free, legal copy exists, and where"},
        }),
        &["source", "ids"],
    )
}

/// A scored candidate for a citation.
fn candidate() -> Value {
    object(
        json!({
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
        json!({
            "input": {"type": "string", "description": "the citation as given"},
            "key": {"description": "your key for the citation, echoed"},
            "claim": {"type": "object", "description": "the citation as understood"},
            "verdict": {
                "type": "string",
                "enum": ["verified", "plausible", "mismatch", "not_found", "unverifiable", "invalid"],
            },
            "reason": {"type": "string", "description": "one sentence"},
            "why": {"type": "object", "description": "the reason as data: code, and the fields that differ"},
            "read": {"type": "object"},
            "match": candidate(),
            "errors": {"type": "array", "items": {"type": "string"}, "description": "providers that failed"},
            "error_kinds": {"type": "array", "items": {"type": "string"}},
            "candidates": {"type": "array", "items": candidate(), "description": "with full: every candidate"},
        }),
        &["input", "verdict", "reason"],
    )
}

/// What each provider's calls cost: `{provider: {cost, remaining, resets_at}}`.
fn usage() -> Value {
    json!({
        "type": "object",
        "description": "this call's usage per provider: cost and what's left of its budget, in USD",
        "additionalProperties": object(
            json!({
                "cost": {"type": "number"},
                "remaining": {"type": "number"},
                "resets_at": {"type": "string"},
            }),
            &[],
        ),
    })
}

fn session() -> Value {
    let mut session = object(
        json!({
            "spent_usd": {"type": "number"},
            "budget_usd": {"type": "number"},
            "left_usd": {"type": "number"},
        }),
        &["spent_usd", "budget_usd", "left_usd"],
    );
    session["description"] = json!("this session's spend, this call included, against its cap");
    session
}

/// `properties` plus `usage` and `session`, as an output schema.
fn output(mut properties: Value, required: &[&str]) -> Map<String, Value> {
    properties["usage"] = usage();
    properties["session"] = session();
    let mut required = required.to_vec();
    required.extend(["usage", "session"]);
    match object(properties, &required) {
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
                    json!({
                        "rank": {"type": "integer"},
                        "score": {"type": ["number", "null"], "description": "the source's own; compare only within one search"},
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
        json!({
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
            "note": {"type": "string"},
            "latest": work(),
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
        &["input", "verdict", "reason", "identified", "decided_by"],
    )
}
