//! Tools the embedding app provides: its [`Host`], the tools it declares,
//! and what it's told after each call.

use std::sync::Arc;

use async_trait::async_trait;
use rmcp::model::{ContentBlock, Tool, ToolAnnotations};
use serde_json::{Map, Value};

use super::StartError;
use crate::{Caller, ToolErrorKind, scope};

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
    /// What a key needs to see and call it, named by the host
    /// ("history.read", "notes.write"): [`scope::check_host`] says which
    /// names it may take (not `sources`, aspero's own). A key needs exactly
    /// this scope; no scope implies another.
    pub scope: String,
    /// Whether it only reads: changes nothing in the host's data. Listed as
    /// `readOnlyHint`; a read-only tool is never listed as destructive.
    pub read_only: bool,
    /// Whether, unless read-only, it may take something away (remove,
    /// untag, delete, overwrite) rather than only add. Listed as
    /// `destructiveHint`.
    pub destructive: bool,
    /// Whether calling it again with the same arguments does nothing more.
    /// Listed as `idempotentHint`.
    pub idempotent: bool,
}

/// A host tool's answer.
#[derive(Debug, Clone, PartialEq)]
pub struct HostOutput {
    /// A JSON object: the result's `structuredContent`.
    pub structured: String,
    /// A few lines for the agent to read; the first is also the activity's
    /// summary.
    pub summary: String,
    /// Pictures for the agent to look at (a page as it's drawn, a crop of
    /// one figure), sent after the text as MCP `image` content, in order.
    /// Within [`MAX_IMAGES`], [`MAX_IMAGE_BYTES`] and
    /// [`MAX_RESULT_IMAGE_BYTES`], each starting with its type's signature,
    /// or the call fails (`failed`) saying which; never cut short. aspero
    /// doesn't decode, resize or recompress them.
    pub images: Vec<HostImage>,
}

/// The most images one host tool result may carry.
pub const MAX_IMAGES: usize = 4;
/// The most bytes one image may have: 3.75 MiB, so its base64 stays within
/// 5 MiB, the per-image cap agents commonly apply.
pub const MAX_IMAGE_BYTES: usize = 3_932_160;
/// The most bytes a host tool result's images may have together: 8 MiB.
pub const MAX_RESULT_IMAGE_BYTES: usize = 8_388_608;

/// A picture in a host tool's answer: its encoded bytes, as a file holds
/// them.
#[derive(Clone, PartialEq, Eq)]
pub struct HostImage {
    pub data: Vec<u8>,
    pub kind: ImageType,
}

/// Its size and type only: the bytes are never shown.
impl std::fmt::Debug for HostImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostImage")
            .field("bytes", &self.data.len())
            .field("kind", &self.kind)
            .finish()
    }
}

/// The image types a host tool may answer with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageType {
    Png,
    Jpeg,
}

impl ImageType {
    /// As MCP's `mimeType` says it.
    pub fn mime_type(self) -> &'static str {
        match self {
            ImageType::Png => "image/png",
            ImageType::Jpeg => "image/jpeg",
        }
    }

    /// The bytes a file of this type starts with: PNG's 8-byte header,
    /// JPEG's start-of-image marker and the next marker's `FF`.
    pub fn signature(self) -> &'static [u8] {
        match self {
            ImageType::Png => b"\x89PNG\r\n\x1a\n",
            ImageType::Jpeg => &[0xFF, 0xD8, 0xFF],
        }
    }

    fn name(self) -> &'static str {
        match self {
            ImageType::Png => "PNG",
            ImageType::Jpeg => "JPEG",
        }
    }
}

/// Why a host tool's images can't be sent, for the agent to read: the
/// first limit broken, or `None` when they're all within them.
pub(crate) fn image_problem(tool: &str, images: &[HostImage]) -> Option<String> {
    if images.len() > MAX_IMAGES {
        return Some(format!(
            "the app's {tool} answered with {} images, past the limit of {MAX_IMAGES} a result",
            images.len()
        ));
    }
    for (i, image) in images.iter().enumerate() {
        let n = i + 1;
        let kind = image.kind.name();
        if !image.data.starts_with(image.kind.signature()) {
            return Some(format!(
                "the app's {tool} answered with image {n} labelled {kind}, but its bytes \
                 don't start with {kind}'s signature"
            ));
        }
        if image.data.len() > MAX_IMAGE_BYTES {
            return Some(format!(
                "the app's {tool} answered with image {n} of {} bytes, past the limit of \
                 {MAX_IMAGE_BYTES} bytes (3.75 MiB) an image",
                image.data.len()
            ));
        }
    }
    let total = image_bytes(images);
    (total > MAX_RESULT_IMAGE_BYTES).then(|| {
        format!(
            "the app's {tool} answered with images of {total} bytes together, past the limit \
             of {MAX_RESULT_IMAGE_BYTES} bytes (8 MiB) a result"
        )
    })
}

/// The images' bytes together.
pub(crate) fn image_bytes(images: &[HostImage]) -> usize {
    images.iter().map(|i| i.data.len()).sum()
}

/// A host tool's answer as MCP content: its text, then each image, in
/// order, base64.
pub(crate) fn content(summary: String, images: &[HostImage]) -> Vec<ContentBlock> {
    use base64::Engine as _;
    let mut content = vec![ContentBlock::text(summary)];
    content.extend(images.iter().map(|i| {
        ContentBlock::image(
            base64::engine::general_purpose::STANDARD.encode(&i.data),
            i.kind.mime_type(),
        )
    }));
    content
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

/// A tool call, done: for the app to show who used what. Built only by
/// the server, so fields can be added without breaking embedders.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Activity {
    pub key_name: String,
    pub tool: String,
    /// A few words: "searched ‘lactate clearance’", "looked up 3 works",
    /// the first line of a host tool's summary, or "error: …".
    pub summary: String,
    pub is_error: bool,
    /// What the call spent, in USD: host tools cost nothing.
    pub cost_usd: f64,
    /// The error's kind, when `is_error`: a host tool's error's
    /// ([`HostError`]: invalid arguments, not found, refused, failed), or
    /// `Forbidden`, `InvalidArguments` (a required field missing),
    /// `Timeout` or `Failed` (not a JSON object).
    pub error: Option<ToolErrorKind>,
    /// The error's message, whole: the summary's without "error: ".
    pub error_message: Option<String>,
    /// What an engine tool was asked, as given, at most 500 characters:
    /// `search`'s keywords or its query tree in a line (`title:sepsis AND
    /// years:2018–`), `search_supports`' tree, `lookup`'s identifiers and
    /// `verify`'s citations joined with "; ", `citing_works`' and
    /// `references`' identifier, `identify`'s title hint, title or first
    /// line. `None` for host tools: the app words its own.
    pub query: Option<String>,
    /// How many came back: the works on a search's page (`search`,
    /// `citing_works`, `references`), the works looked up, the citations
    /// verified, or the one work identified. `None` for errors, host tools
    /// and `search_supports`.
    pub count: Option<u64>,
    /// Whether the call only estimated: `analyse` with `estimate_only`,
    /// which ran nothing and spent nothing (its `count` is the works it
    /// was given). False for every other call.
    pub estimate_only: bool,
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

    /// Which works the app already holds (a reference manager's library),
    /// as [`Holdings::held`](crate::Holdings::held) says: asked once per
    /// `search`, `citing_works` or `references` page and `analyse` result,
    /// which mark them `held: true`; abandoned after
    /// [`HELD_TIMEOUT`](super::HELD_TIMEOUT), when nothing is marked.
    /// `None`, unless the app overrides it: nothing is marked.
    async fn held(&self, works: Vec<Vec<String>>) -> Option<Vec<bool>> {
        let _ = works;
        None
    }
}

/// A host tool, checked and ready to list.
#[derive(Debug, Clone)]
pub(crate) struct Listed {
    pub scope: String,
    pub tool: Tool,
    /// Its input schema's required fields.
    pub required: Vec<String>,
}

/// The host's tools, checked: names unique and unlike the engine's,
/// scopes valid ([`scope::check_host`]), schemas JSON objects. Annotated
/// with the hints the host declared.
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
        if let Err(problem) = scope::check_host(&spec.scope) {
            return Err(StartError::Setup(format!(
                "host tool {name:?}: its scope {:?} {problem}",
                spec.scope
            )));
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
        let mut tool = Tool::new(name.to_owned(), spec.description.clone(), input)
            .with_annotations(
                ToolAnnotations::with_title(spec.title.clone())
                    .read_only(spec.read_only)
                    .destructive(!spec.read_only && spec.destructive)
                    .idempotent(spec.idempotent)
                    .open_world(false),
            );
        if !spec.title.is_empty() {
            tool = tool.with_title(spec.title.clone());
        }
        if let Some(output) = &spec.output_schema {
            tool = tool.with_raw_output_schema(Arc::new(schema("output", output)?));
        }
        listed.push(Listed {
            scope: spec.scope.clone(),
            tool,
            required,
        });
    }
    Ok(listed)
}

#[cfg(test)]
mod tests {
    use rmcp::model::CallToolResult;
    use serde_json::json;

    use super::*;

    /// An image of `kind` with `len` bytes, its signature first.
    fn image(kind: ImageType, len: usize) -> HostImage {
        let mut data = kind.signature().to_vec();
        data.resize(len, 0);
        HostImage { data, kind }
    }

    fn problem(images: &[HostImage]) -> String {
        image_problem("page", images).expect("a limit broken")
    }

    #[test]
    fn the_signatures_are_pngs_header_and_jpegs_start_of_image() {
        assert_eq!(
            ImageType::Png.signature(),
            [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]
        );
        assert_eq!(ImageType::Jpeg.signature(), [0xFF, 0xD8, 0xFF]);
        assert_eq!(ImageType::Png.mime_type(), "image/png");
        assert_eq!(ImageType::Jpeg.mime_type(), "image/jpeg");
    }

    #[test]
    fn images_within_the_limits_pass() {
        assert_eq!(image_problem("page", &[]), None);
        let png = image(ImageType::Png, 100);
        let jpeg = image(ImageType::Jpeg, 3);
        assert_eq!(image_problem("page", &[png, jpeg]), None);
        // Each at its limit, and together at theirs.
        let biggest = image(ImageType::Png, MAX_IMAGE_BYTES);
        assert_eq!(image_problem("page", &[biggest]), None);
        let half = MAX_RESULT_IMAGE_BYTES / 4;
        let four = vec![image(ImageType::Jpeg, half); MAX_IMAGES];
        assert_eq!(image_problem("page", &four), None);
    }

    #[test]
    fn a_mislabelled_or_empty_image_fails_naming_its_type() {
        let mut jpeg_as_png = image(ImageType::Jpeg, 100);
        jpeg_as_png.kind = ImageType::Png;
        let ok = image(ImageType::Png, 10);
        assert_eq!(
            problem(&[ok, jpeg_as_png]),
            "the app's page answered with image 2 labelled PNG, but its bytes don't start \
             with PNG's signature"
        );
        let empty = HostImage {
            data: Vec::new(),
            kind: ImageType::Jpeg,
        };
        assert!(problem(&[empty]).contains("image 1 labelled JPEG"));
        // PNG's header cut short isn't PNG's.
        let short = HostImage {
            data: ImageType::Png.signature()[..7].to_vec(),
            kind: ImageType::Png,
        };
        assert!(problem(&[short]).contains("PNG's signature"));
    }

    #[test]
    fn an_image_past_its_limit_fails_naming_it() {
        let big = image(ImageType::Png, MAX_IMAGE_BYTES + 1);
        assert_eq!(
            problem(&[big]),
            "the app's page answered with image 1 of 3932161 bytes, past the limit of \
             3932160 bytes (3.75 MiB) an image"
        );
    }

    #[test]
    fn more_images_than_a_result_may_carry_fail_naming_the_limit() {
        let five = vec![image(ImageType::Png, 10); MAX_IMAGES + 1];
        assert_eq!(
            problem(&five),
            "the app's page answered with 5 images, past the limit of 4 a result"
        );
    }

    #[test]
    fn images_past_their_limit_together_fail_naming_it() {
        // Three just within each image's limit: 11.25 MiB together.
        let three = vec![image(ImageType::Jpeg, MAX_IMAGE_BYTES); 3];
        assert_eq!(
            problem(&three),
            "the app's page answered with images of 11796480 bytes together, past the limit \
             of 8388608 bytes (8 MiB) a result"
        );
        let over = MAX_RESULT_IMAGE_BYTES / 4 + 1;
        let four = vec![image(ImageType::Png, over); MAX_IMAGES];
        assert!(problem(&four).contains("(8 MiB) a result"));
        assert_eq!(image_bytes(&four), MAX_RESULT_IMAGE_BYTES + 4);
    }

    /// The limits as the issue agreed them, and the docs say: a change is
    /// deliberate.
    #[test]
    fn the_limits_are_as_documented() {
        assert_eq!(MAX_IMAGES, 4);
        assert_eq!(MAX_IMAGE_BYTES, 3 * 1024 * 1024 + 768 * 1024);
        assert_eq!(MAX_RESULT_IMAGE_BYTES, 8 * 1024 * 1024);
        // An image at its limit stays within 5 MiB as base64.
        assert!(MAX_IMAGE_BYTES.div_ceil(3) * 4 <= 5 * 1024 * 1024);
    }

    /// The text first, then each image as MCP's `image` content, base64,
    /// in order.
    #[test]
    fn images_follow_the_text_in_order_as_base64() {
        let png = HostImage {
            data: b"\x89PNG\r\n\x1a\nab".to_vec(),
            kind: ImageType::Png,
        };
        let jpeg = HostImage {
            data: vec![0xFF, 0xD8, 0xFF, 0xE0],
            kind: ImageType::Jpeg,
        };
        let result = CallToolResult::success(content("a page".into(), &[png, jpeg]));
        let json = serde_json::to_value(&result).unwrap();
        assert_eq!(
            json["content"],
            json!([
                {"type": "text", "text": "a page"},
                {"type": "image", "data": "iVBORw0KGgphYg==", "mimeType": "image/png"},
                {"type": "image", "data": "/9j/4A==", "mimeType": "image/jpeg"},
            ])
        );
    }

    /// Without images, a result is as it was: its JSON byte for byte.
    #[test]
    fn without_images_a_result_is_unchanged() {
        let now = CallToolResult::success(content("found 2\nfirst: X".into(), &[]));
        let before = CallToolResult::success(vec![ContentBlock::text("found 2\nfirst: X")]);
        assert_eq!(
            serde_json::to_string(&now).unwrap(),
            serde_json::to_string(&before).unwrap()
        );
    }

    /// Debug output says an image's size, never its bytes.
    #[test]
    fn an_image_debugs_as_its_size() {
        let png = image(ImageType::Png, 4096);
        assert_eq!(format!("{png:?}"), "HostImage { bytes: 4096, kind: Png }");
    }
}
