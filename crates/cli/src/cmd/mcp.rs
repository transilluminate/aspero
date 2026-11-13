//! `aspero mcp`: the engine's tools for agents, over the Model Context
//! Protocol on stdin and stdout.

use std::io::IsTerminal;
use std::sync::Arc;

use aspero_engine::Engine;
use aspero_mcp::{Caller, Session, Tools};

use super::Failure;
use crate::exit::Exit;

/// Serves one session until stdin closes. stdout carries the protocol and
/// nothing else.
pub async fn run(engine: Engine, budget: f64) -> Result<Exit, Failure> {
    if std::io::stdin().is_terminal() {
        eprintln!(
            "aspero mcp speaks the Model Context Protocol on stdin and stdout, for an \
             agent to launch:\n  claude mcp add aspero -- aspero mcp\nWaiting for a \
             client; Ctrl-D quits."
        );
    }
    let tools = Arc::new(Tools::new(engine));
    let session = Arc::new(Session::new(Caller::default(), budget));
    aspero_mcp::serve_stdio(tools, session)
        .await
        .map_err(|e| Failure {
            exit: Exit::Internal,
            message: format!("MCP session failed: {e}"),
        })?;
    Ok(Exit::Ok)
}
