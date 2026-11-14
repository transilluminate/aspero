//! The Streamable HTTP server on its own, for trying it with an agent or
//! `curl`: the standard engine (`OPENALEX_API_KEY`, `ASPERO_MAILTO`), a
//! cache in a directory of your choosing, and one key with the `sources`
//! scope for the token in `ASPERO_MCP_TOKEN`. Serves until Ctrl-C.
//!
//! ```sh
//! ASPERO_MCP_TOKEN=… cargo run -p aspero-mcp --features http --example http_server -- 23130 /tmp/aspero-mcp
//! ```

use std::sync::Arc;

use aspero_engine::{SecretString, SqliteCache, StandardConfig, standard};
use aspero_mcp::Scope;
use aspero_mcp::Tools;
use aspero_mcp::http::{HttpConfig, HttpServer, Key, sha256_hex};

#[tokio::main]
async fn main() {
    let env = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
    let mut args = std::env::args().skip(1);
    let port: u16 = args.next().map_or(23130, |p| p.parse().expect("a port"));
    let dir = args.next().unwrap_or_else(|| "target/mcp-example".into());
    let token = env("ASPERO_MCP_TOKEN").expect("ASPERO_MCP_TOKEN: the bearer token to accept");

    let cache = Arc::new(
        SqliteCache::open(&std::path::Path::new(&dir).join("cache.sqlite")).expect("cache"),
    );
    let mut config = StandardConfig::all(env("OPENALEX_API_KEY").map(SecretString::from));
    config.mailto = env("ASPERO_MAILTO");
    config.cache = Some(cache.clone());
    config.pacing = Some(cache.clone());
    let engine = standard(config).expect("engine");

    let mut http = HttpConfig::default();
    http.port = port;
    http.keys = vec![Key {
        id: "example".into(),
        name: "example agent".into(),
        scopes: vec![Scope::Sources],
        sha256: sha256_hex(&token),
        daily_budget_usd: 0.05,
    }];
    let server = HttpServer::start(Arc::new(Tools::new(engine)), http, None, cache)
        .await
        .unwrap_or_else(|e| panic!("{e}"));
    eprintln!(
        "serving http://127.0.0.1:{}/mcp; Ctrl-C stops",
        server.port()
    );
    tokio::signal::ctrl_c().await.expect("ctrl-c");
    server.stop().await;
    eprintln!("stopped: {:?}", server.status());
}
