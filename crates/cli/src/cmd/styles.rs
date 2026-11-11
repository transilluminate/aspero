//! `aspero styles`: the bundled citation styles, and their credits.

use aspero_bib::cite;
use serde_json::json;

use super::{Failure, Out, print};
use crate::args::Format;
use crate::exit::Exit;
use crate::output;

pub fn run(credits: bool, out: Out) -> Result<Exit, Failure> {
    if credits {
        match out.format {
            Format::Json | Format::Jsonl => print(&json!({"credits": cite::credits()})),
            _ => print!("{}", cite::credits()),
        }
        return Ok(Exit::Ok);
    }
    match out.format {
        Format::Json | Format::Jsonl => print(&json!({
            "styles": cite::BUNDLED
                .iter()
                .map(|b| json!({"id": b.id, "title": b.title}))
                .collect::<Vec<_>>(),
        })),
        _ => {
            let mut table = output::table(&["Style", "Title"]);
            for b in cite::BUNDLED {
                table.add_row(vec![b.id, b.title]);
            }
            println!("{table}");
        }
    }
    Ok(Exit::Ok)
}
