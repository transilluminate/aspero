//! `aspero licences`: what this binary is built from, and with `--full`,
//! the notices its licences ask for. Both come from `notices/`, which
//! `just notices` writes at release.

use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{Out, print, print_line};
use crate::args::Format;
use crate::exit::Exit;
use crate::grid::{Col, Grid};

const NOTICES: &str = include_str!("../../notices/notices.txt");
const DEPENDENCIES: &str = include_str!("../../notices/dependencies.json");

#[derive(Debug, Deserialize, Serialize)]
struct Dependency {
    name: String,
    version: String,
    license: String,
    repository: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Dependencies {
    dependencies: Vec<Dependency>,
}

// Like every command: main hands over what it parsed.
#[allow(clippy::needless_pass_by_value)]
pub fn run(out: Out) -> Exit {
    if out.full {
        match out.format {
            Format::Json | Format::Jsonl => print(&json!({"notices": NOTICES})),
            _ => print!("{NOTICES}"),
        }
        return Exit::Ok;
    }
    let file: Dependencies =
        serde_json::from_str(DEPENDENCIES).expect("notices/dependencies.json is valid");
    match out.format {
        Format::Json => print(&json!({"dependencies": file.dependencies})),
        Format::Jsonl => {
            for d in &file.dependencies {
                print_line(&json!(d));
            }
        }
        _ => {
            let mut grid = Grid::new([
                Col::content("Name"),
                Col::content("Version"),
                Col::fill("Licence"),
            ]);
            for d in &file.dependencies {
                grid.row([&d.name, &d.version, &d.license]);
            }
            println!("{grid}");
        }
    }
    Exit::Ok
}
