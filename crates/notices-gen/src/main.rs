//! `just notices`: writes the FFI's and CLI's third-party notices.
//! `--check` writes nothing, and fails if any would change.

use std::process::ExitCode;

fn main() -> ExitCode {
    let check = match std::env::args().nth(1).as_deref() {
        None => false,
        Some("--check") => true,
        Some(other) => {
            eprintln!("usage: notices-gen [--check]  (not {other:?})");
            return ExitCode::from(2);
        }
    };
    match run(check) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(check: bool) -> Result<ExitCode, notices_gen::Error> {
    let root = notices_gen::workspace_root();
    let outputs = notices_gen::generate(&root)?;
    let stale = notices_gen::stale(&root, &outputs);
    if check {
        if stale.is_empty() {
            return Ok(ExitCode::SUCCESS);
        }
        for path in &stale {
            eprintln!("out of date: {}", path.display());
        }
        eprintln!("third-party notices are out of date: {}", notices_gen::FIX);
        return Ok(ExitCode::FAILURE);
    }
    for output in &outputs {
        let path = root.join(&output.path);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&path, &output.contents)?;
    }
    for path in &stale {
        println!("wrote {}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}
