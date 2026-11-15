//! The terminal output goes to: how wide it is, whether to colour, text
//! wrapped to fit, and progress that overwrites itself. Set once in `main`;
//! until then (tests) output is plain and unwrapped, as it is in a pipe.

use std::io::{IsTerminal, Write as _};
use std::sync::OnceLock;

use unicode_width::UnicodeWidthChar;

#[derive(Debug, Default)]
struct Term {
    /// Stdout's width, when it's a terminal.
    width: Option<usize>,
    colour: bool,
    err_colour: bool,
    err_tty: bool,
}

static TERM: OnceLock<Term> = OnceLock::new();

fn term() -> &'static Term {
    TERM.get_or_init(Term::default)
}

/// Reads the terminal: stdout's width and, for each of stdout and stderr,
/// whether it's a terminal to colour (not with `NO_COLOR` set).
pub fn init() {
    let no_colour = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    let out_tty = std::io::stdout().is_terminal();
    let err_tty = std::io::stderr().is_terminal();
    let width = out_tty.then(|| {
        terminal_size::terminal_size_of(std::io::stdout()).map_or(80, |(w, _)| usize::from(w.0))
    });
    let _ = TERM.set(Term {
        width,
        colour: out_tty && !no_colour,
        err_colour: err_tty && !no_colour,
        err_tty,
    });
}

/// Stdout's width, when it's a terminal.
pub fn width() -> Option<usize> {
    term().width
}

/// Whether stdout is coloured.
pub fn colour() -> bool {
    term().colour
}

/// What a piece of text means, for its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Paint {
    Good,
    Warning,
    Error,
    /// Secondary: costs, paging, provenance.
    Dim,
    /// A heading.
    Bold,
    /// A verdict or status heading a result: bold, in its colour.
    Strong(Level),
}

/// How much attention something needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Good,
    Info,
    Warning,
    Error,
}

fn sgr(paint: Paint) -> &'static str {
    let level = |l: Level| match l {
        Level::Good => "32",
        Level::Info => "34",
        Level::Warning => "33",
        Level::Error => "31",
    };
    match paint {
        Paint::Good => level(Level::Good),
        Paint::Warning => level(Level::Warning),
        Paint::Error => level(Level::Error),
        Paint::Dim => "2",
        Paint::Bold => "1",
        Paint::Strong(Level::Good) => "1;32",
        Paint::Strong(Level::Info) => "1;34",
        Paint::Strong(Level::Warning) => "1;33",
        Paint::Strong(Level::Error) => "1;31",
    }
}

fn painted(on: bool, text: &str, paint: Paint) -> String {
    if on && !text.is_empty() {
        format!("\x1b[{}m{text}\x1b[0m", sgr(paint))
    } else {
        text.to_owned()
    }
}

/// `text` coloured for stdout, when it's coloured.
pub fn paint(text: &str, paint: Paint) -> String {
    painted(term().colour, text, paint)
}

/// `text` coloured for stderr, when it's coloured.
pub fn paint_err(text: &str, paint: Paint) -> String {
    painted(term().err_colour, text, paint)
}

/// `text` wrapped to stdout's width, when it's a terminal; as it is
/// otherwise, a record per line for `grep` and files.
pub fn fill(text: &str) -> String {
    match width() {
        Some(width) => wrap(text, width),
        None => text.to_owned(),
    }
}

/// Each line of `text` longer than `width` broken at spaces, never inside
/// a word (identifiers and links stay whole), and between the parts of a
/// `·` list rather than inside one ("· journal article" stays together)
/// where a part fits on a line. Continuations start under where the
/// line's text does: past a list marker (`- `, `[2] `, `3. `, `⚠ `), past
/// a short label and the gap after it (`VERIFIED  `, `doi:…  `), or two
/// columns in.
pub fn wrap(text: &str, width: usize) -> String {
    text.split('\n')
        .map(|line| wrap_line(line, width))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Widest label whose gap a continuation lines up after.
const LABEL: usize = 28;

fn wrap_line(line: &str, width: usize) -> String {
    if visible_width(line) <= width {
        return line.to_owned();
    }
    let hang = " ".repeat(hang(&strip(line)).min(width / 2));
    let indent = line.len() - line.trim_start_matches(' ').len();
    let words: Vec<(usize, &str)> = words(&line[indent..]).collect();
    // Where each `·` part starts, and how wide it is from there.
    let part = |i: usize| i == 0 || words[i - 1].1 == "·";
    let part_width = |i: usize| {
        let mut w = visible_width(words[i].1);
        for (gap, word) in words.iter().skip(i + 1) {
            if *word == "·" {
                break;
            }
            w += gap + visible_width(word);
        }
        w
    };
    let mut out = line[..indent].to_owned();
    let mut used = indent;
    for (i, &(gap, word)) in words.iter().enumerate() {
        let w = visible_width(word);
        let whole = part(i) && hang.len() + part_width(i) <= width;
        let fits = if whole {
            used + gap + part_width(i) <= width
        } else {
            used + gap + w <= width
        };
        if i > 0 && !fits {
            out.push('\n');
            out.push_str(&hang);
            used = hang.len();
        } else if i > 0 {
            out.push_str(&" ".repeat(gap));
            used += gap;
        }
        out.push_str(word);
        used += w;
    }
    out
}

/// The words of `text` and the spaces before each.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut rest = text;
    std::iter::from_fn(move || {
        let trimmed = rest.trim_start_matches(' ');
        let gap = rest.len() - trimmed.len();
        if trimmed.is_empty() {
            return None;
        }
        let end = trimmed.find(' ').unwrap_or(trimmed.len());
        let (word, after) = trimmed.split_at(end);
        rest = after;
        Some((gap, word))
    })
}

/// Where a line's continuations start (see [`wrap`]), in columns.
fn hang(line: &str) -> usize {
    let indent = line.len() - line.trim_start_matches(' ').len();
    let rest = &line[indent..];
    let marker = ["- ", "• ", "⚠ "]
        .iter()
        .find(|m| rest.starts_with(**m))
        .map(|m| m.chars().count())
        .or_else(|| {
            // "[12] " and "12. "
            let digits = rest
                .trim_start_matches('[')
                .chars()
                .take_while(char::is_ascii_digit)
                .count();
            let open = usize::from(rest.starts_with('['));
            let after = rest.get(open + digits..)?;
            (digits > 0 && (after.starts_with("] ") || after.starts_with(". ")))
                .then_some(open + digits + 2)
        });
    if let Some(m) = marker {
        return indent + m;
    }
    let label = rest.find("  ").filter(|&at| at > 0).and_then(|at| {
        let gap = rest[at..].len() - rest[at..].trim_start_matches(' ').len();
        let to = visible_width(&rest[..at]) + gap;
        (to <= LABEL).then_some(to)
    });
    indent + label.unwrap_or(2)
}

/// `text` without colour codes.
fn strip(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // CSI: ESC [ … final byte in @–~.
            for c in chars.by_ref() {
                if ('@'..='~').contains(&c) && c != '[' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Columns `text` takes on a terminal, colour codes aside.
pub fn visible_width(text: &str) -> usize {
    strip(text)
        .chars()
        .filter_map(UnicodeWidthChar::width)
        .sum()
}

/// "checked 2 of 5 entries…" on stderr, overwritten in place, when stderr
/// is a terminal and there's more than one; nothing otherwise.
pub struct Progress {
    what: &'static str,
    total: usize,
    shown: bool,
    on: bool,
}

impl Progress {
    pub fn new(what: &'static str, total: usize) -> Self {
        Progress {
            what,
            total,
            shown: false,
            on: total > 1 && term().err_tty,
        }
    }

    pub fn show(&mut self, done: usize) {
        if self.on && done < self.total {
            let line = format!("checked {done} of {} {}…", self.total, self.what);
            eprint!("\r\x1b[K{}", paint_err(&line, Paint::Dim));
            let _ = std::io::stderr().flush();
            self.shown = true;
        }
    }

    /// Clears the line, before results are printed.
    pub fn clear(&mut self) {
        if self.shown {
            eprint!("\r\x1b[K");
            let _ = std::io::stderr().flush();
            self.shown = false;
        }
    }
}

impl Drop for Progress {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_wrap_under_their_text_and_never_inside_a_word() {
        assert_eq!(wrap("short", 10), "short");
        assert_eq!(
            wrap("  - add doi = {10.1038/nature14539} and more", 24),
            "  - add doi =\n    {10.1038/nature14539}\n    and more"
        );
        assert_eq!(
            wrap("VERIFIED  LeCun Y, Bengio Y. Deep learning.", 24),
            "VERIFIED  LeCun Y,\n          Bengio Y. Deep\n          learning."
        );
        assert_eq!(
            wrap("  [12] key  a seed with a long title", 20),
            "  [12] key  a seed\n       with a long\n       title"
        );
        // Parts of a `·` list break between them where they can.
        assert_eq!(
            wrap("  Yann LeCun · 2015 · Nature · journal article", 36),
            "  Yann LeCun · 2015 · Nature ·\n    journal article"
        );
        assert_eq!(
            wrap("  a · one part too long for any line", 16),
            "  a · one part\n    too long for\n    any line"
        );
        // A word wider than the line is left whole.
        assert_eq!(
            wrap("  see https://example.org/a/very/long/path", 16),
            "  see\n    https://example.org/a/very/long/path"
        );
        // Colour codes take no room.
        let red = "\x1b[31mERROR\x1b[0m";
        assert_eq!(visible_width(red), 5);
        assert_eq!(wrap(&format!("{red}  a b c"), 12), format!("{red}  a b c"));
        assert_eq!(
            wrap(&format!("{red}  a b c d e f"), 16),
            format!("{red}  a b c d e\n       f")
        );
    }
}
