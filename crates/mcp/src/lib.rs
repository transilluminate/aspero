//! aspero as a Model Context Protocol server: the engine's search, lookup,
//! verification and identification as tools an agent calls.
//!
//! Two layers, so transports and tools can grow apart:
//!
//! - [`Tools`]: the engine tools as plain functions. Each takes its
//!   arguments as JSON (described by a JSON Schema), checks the
//!   [`Session`]'s spend first, and answers with an [`Output`]: aspero's
//!   JSON shapes (`aspero_json`, as the CLI prints them) plus a few lines
//!   of text, or a [`ToolError`] to show the agent. Nothing here knows the
//!   protocol.
//! - [`serve_stdio`] (and [`serve`], over any byte stream): the protocol,
//!   via the official SDK (`rmcp`), on top of [`Tools`].
//!
//! - With the `http` feature, [`http::HttpServer`]: Streamable HTTP on
//!   loopback for apps that embed aspero, with bearer keys and their
//!   [`Scope`]s, budgets per session and per key per day, and tools the
//!   app provides ([`http::Host`]).
//!
//! A session is one connection: over stdio, the agent that launched the
//! server; over HTTP, an `initialize` and the requests after it. It carries
//! the [`Caller`] and the spend cap.

mod args;
#[cfg(feature = "http")]
pub mod http;
mod schema;
mod server;
mod session;
mod summary;
mod tools;

pub use server::{serve, serve_stdio};
pub use session::{Caller, DEFAULT_BUDGET_USD, Scope, Session};
pub use tools::{
    MAX_CITATIONS, MAX_IDS, MAX_LIMIT, Output, ToolError, ToolErrorKind, ToolSpec, Tools,
};
