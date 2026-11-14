//! Tools the embedding app provides: its [`Host`], the tools it declares,
//! and what it's told after each call.

use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::{Tool, ToolAnnotations};
use serde_json::{Map, Value};

use super::StartError;
use crate::{Caller, Scope};

/// A tool the app serves, as it declares it.
#[derive(Debug, Clone, PartialEq)]
pub struct HostTool {
    /// Unique, and unlike every engine tool's.
    pub name: String,
    pub title: String,
    pub description: String,
    /// JSON Schema of the arguments: an object.
    pub input_schema: String,
    /// JSON Schema of the result's `structured`: an object.
    pub output_schema: Option<String>,
    /// What a key needs to see and call it.
    pub scope: Scope,
}

/// A host tool's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct HostOutput {
    /// A JSON object: the result's `structuredContent`.
    pub structured: String,
    /// A few lines for the agent to read; the first is also the activity's
    /// summary.
    pub summary: String,
}

/// Why a host tool failed, for the agent to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    InvalidArguments(String),
    NotFound(String),
    Refused(String),
    Failed(String),
}

impl HostError {
    pub fn kind(&self) -> &'static str {
        match self {
            HostError::InvalidArguments(_) => "invalid_arguments",
            HostError::NotFound(_) => "not_found",
            HostError::Refused(_) => "refused",
            HostError::Failed(_) => "failed",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            HostError::InvalidArguments(m)
            | HostError::NotFound(m)
            | HostError::Refused(m)
            | HostError::Failed(m) => m,
        }
    }
}

impl std::fmt::Display for HostError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.message(), self.kind())
    }
}

impl std::error::Error for HostError {}

/// A tool call, done: for the app to show who used what.
#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub key_name: String,
    pub tool: String,
    /// A few words: "searched ‘lactate clearance’", "looked up 3 works",
    /// the first line of a host tool's summary, or "error: …".
    pub summary: String,
    pub is_error: bool,
    /// What the call spent, in USD: host tools cost nothing.
    pub cost_usd: f64,
}

/// The embedding app: its tools, served through aspero's protocol, auth and
/// timeouts, and a callback after every tool call.
#[async_trait]
pub trait Host: Send + Sync {
    /// The app's tools: asked at start and when the app says they changed.
    fn tools(&self) -> Vec<HostTool>;

    /// Calls the tool `name` with `arguments` (a JSON object whose schema's
    /// required fields are present) for `caller`, whose key has the
    /// tool's scope. Abandoned after the server's host timeout.
    async fn call(
        &self,
        name: String,
        arguments: String,
        caller: Caller,
    ) -> Result<HostOutput, HostError>;

    /// After every tool call, engine or host, success or error: from a
    /// background thread, in order, never holding up a request.
    fn activity(&self, activity: Activity);
}

/// A host tool, checked and ready to list.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    pub scope: Scope,
    pub tool: Tool,
    /// Its input schema's required fields.
    pub required: Vec<String>,
}

/// The host's tools, checked: names unique and unlike the engine's,
/// schemas JSON objects.
pub(crate) fn catalog(host: &dyn Host, engine: &[&str]) -> Result<Vec<Listed>, StartError> {
    let mut listed: Vec<Listed> = Vec::new();
    for spec in host.tools() {
        let name = spec.name.trim();
        if name.is_empty() {
            return Err(StartError::Setup("a host tool has no name".into()));
        }
        if engine.contains(&name) || listed.iter().any(|l| l.tool.name == name) {
            return Err(StartError::ToolNameClash { name: name.into() });
        }
        let schema = |what: &str, json: &str| -> Result<Map<String, Value>, StartError> {
            match serde_json::from_str(json) {
                Ok(Value::Object(map)) => Ok(map),
                _ => Err(StartError::Setup(format!(
                    "host tool {name:?}: its {what} schema isn't a JSON object"
                ))),
            }
        };
        let input = schema("input", &spec.input_schema)?;
        let required = match input.get("required") {
            Some(Value::Array(fields)) => fields
                .iter()
                .filter_map(|f| f.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };
        let writes = spec.scope == Scope::WriteHistory;
        let mut tool = Tool::new(name.to_owned(), spec.description.clone(), input)
            .with_annotations(
                ToolAnnotations::with_title(spec.title.clone())
                    .read_only(!writes)
                    .destructive(false)
                    .idempotent(!writes)
                    .open_world(false),
            );
        if !spec.title.is_empty() {
            tool = tool.with_title(spec.title.clone());
        }
        if let Some(output) = &spec.output_schema {
            tool = tool.with_raw_output_schema(Arc::new(schema("output", output)?));
        }
        listed.push(Listed {
            scope: spec.scope,
            tool,
            required,
        });
    }
    Ok(listed)
}
