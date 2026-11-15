//! `aspero styles`: the bundled citation styles, and their credits.

use aspero_bib::cite;
use serde_json::json;

use super::{Out, print};
use crate::args::Format;
use crate::exit::Exit;
use crate::grid::{Col, Grid};

// Like every command: main hands over what it parsed.
#[allow(clippy::needless_pass_by_value)]
pub fn run(credits: bool, out: Out) -> Exit {
    if credits {
        match out.format {
            Format::Json | Format::Jsonl => print(&json!({"credits": cite::credits()})),
            _ => print!("{}", cite::credits()),
        }
        return Exit::Ok;
    }
    match out.format {
        Format::Json | Format::Jsonl => print(&json!({
            "styles": cite::BUNDLED
                .iter()
                .map(|b| json!({"id": b.id, "title": b.title}))
                .collect::<Vec<_>>(),
        })),
        _ => {
            let mut grid = Grid::new([Col::content("Style"), Col::fill("Title")]);
            for b in cite::BUNDLED {
                grid.row([b.id, b.title]);
            }
            println!("{grid}");
        }
    }
    Exit::Ok
}
