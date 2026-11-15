//! Tables that fit the terminal. Each column says how it takes space:
//! identifiers and numbers as wide as they are (never broken), names up to
//! a cap, titles whatever is left. When that's too little, low-value
//! columns go, or move under the title, in the order each says. Rows that
//! take more than one line are ruled apart.

use comfy_table::{
    Attribute, Cell, CellAlignment, Color, ColumnConstraint, ContentArrangement, LineStyle, Table,
    Width, presets,
};

use crate::term::{self, Level, visible_width};

/// The least a title column should get before others make way.
const FILL_MIN: usize = 30;

#[derive(Debug, Clone, Copy)]
enum Fit {
    Content,
    Cap(usize),
    Fill,
}

#[derive(Debug, Clone, Copy)]
enum How {
    Drop,
    /// Under the text of the column headed `into`; `join`ed to the line of
    /// the column folded before it.
    Fold {
        into: &'static str,
        join: bool,
    },
}

/// A column: its header, how it takes space, and what becomes of it when
/// the table is too wide.
#[derive(Debug, Clone)]
pub struct Col {
    header: &'static str,
    fit: Fit,
    right: bool,
    narrow: Option<(u8, How)>,
    folded_header: Option<&'static str>,
}

impl Col {
    fn new(header: &'static str, fit: Fit) -> Self {
        Col {
            header,
            fit,
            right: false,
            narrow: None,
            folded_header: None,
        }
    }

    /// As wide as its widest cell, never wrapped: identifiers, numbers,
    /// statuses.
    pub fn content(header: &'static str) -> Self {
        Col::new(header, Fit::Content)
    }

    /// At most `width` wide; longer cells wrap.
    pub fn cap(header: &'static str, width: usize) -> Self {
        Col::new(header, Fit::Cap(width))
    }

    /// The width left over: titles, prose.
    pub fn fill(header: &'static str) -> Self {
        Col::new(header, Fit::Fill)
    }

    pub fn right(mut self) -> Self {
        self.right = true;
        self
    }

    /// Left out when the table is too wide, at step `rank` (lowest first).
    pub fn drops(mut self, rank: u8) -> Self {
        self.narrow = Some((rank, How::Drop));
        self
    }

    /// Moved under the text of the column headed `into` when the table is
    /// too wide, at step `rank`.
    pub fn folds(mut self, rank: u8, into: &'static str) -> Self {
        self.narrow = Some((rank, How::Fold { into, join: false }));
        self
    }

    /// As [`Col::folds`], on one line with the column folded before it:
    /// "LeCun et al. · 2015".
    pub fn folds_beside(mut self, rank: u8, into: &'static str) -> Self {
        self.narrow = Some((rank, How::Fold { into, join: true }));
        self
    }

    /// The header once other columns are folded into this one.
    pub fn or_when_folded(mut self, header: &'static str) -> Self {
        self.folded_header = Some(header);
        self
    }
}

/// A cell's text and, for a status, how much attention it needs.
#[derive(Debug, Clone, Default)]
pub struct Text {
    text: String,
    level: Option<Level>,
}

impl From<String> for Text {
    fn from(text: String) -> Self {
        Text { text, level: None }
    }
}

impl From<&str> for Text {
    fn from(text: &str) -> Self {
        Text::from(text.to_owned())
    }
}

impl From<&String> for Text {
    fn from(text: &String) -> Self {
        Text::from(text.clone())
    }
}

/// A cell coloured by how much attention it needs.
pub fn status(text: impl Into<String>, level: Level) -> Text {
    Text {
        text: text.into(),
        level: Some(level),
    }
}

/// Rows under headers; `Display` fits it to stdout.
#[derive(Debug, Clone)]
pub struct Grid {
    cols: Vec<Col>,
    rows: Vec<Vec<Text>>,
}

/// Which columns show, and the cells as shown.
struct Layout {
    shown: Vec<usize>,
    headers: Vec<&'static str>,
    rows: Vec<Vec<Text>>,
    /// Per shown column, the width its folded-in lines need.
    lower: Vec<usize>,
}

impl Grid {
    pub fn new(cols: impl IntoIterator<Item = Col>) -> Self {
        Grid {
            cols: cols.into_iter().collect(),
            rows: Vec::new(),
        }
    }

    /// A row: one cell per column, in order.
    pub fn row<T: Into<Text>>(&mut self, cells: impl IntoIterator<Item = T>) {
        let mut cells: Vec<Text> = cells.into_iter().map(Into::into).collect();
        cells.resize_with(self.cols.len(), Text::default);
        self.rows.push(cells);
    }

    #[cfg(test)]
    pub fn headers(&self) -> Vec<&'static str> {
        self.cols.iter().map(|c| c.header).collect()
    }

    /// The table in `width` columns (unbounded without one), coloured or
    /// not.
    pub fn render(&self, width: Option<usize>, colour: bool) -> String {
        let mut steps: Vec<(u8, usize)> = self
            .cols
            .iter()
            .enumerate()
            .filter_map(|(i, c)| Some((c.narrow?.0, i)))
            .collect();
        steps.sort_unstable();
        let mut gone = vec![false; self.cols.len()];
        let mut layout = self.layout(&gone);
        if let Some(width) = width {
            let mut steps = steps.as_slice();
            while self.needs(&layout) > width
                && let Some(&(rank, _)) = steps.first()
            {
                let taken = steps.iter().take_while(|(r, _)| *r == rank).count();
                for &(_, i) in &steps[..taken] {
                    gone[i] = true;
                }
                steps = &steps[taken..];
                layout = self.layout(&gone);
            }
        }
        // Rows of several lines run together without rules between them.
        // A rare one reads as a continuation (its first cell is blank), and
        // ruling a long list for it doubles its length.
        let widths = width.map(|w| self.widths(&layout, w));
        let tall = layout
            .rows
            .iter()
            .filter(|row| {
                row.iter().enumerate().any(|(k, cell)| {
                    cell.text.contains('\n')
                        || widths
                            .as_ref()
                            .is_some_and(|w| visible_width(&cell.text) > w[k])
                })
            })
            .count();
        let ruled = tall > 0 && tall * 10 > layout.rows.len();
        self.table(&layout, widths.as_deref(), colour, ruled)
            .to_string()
    }

    /// The columns shown when those marked `gone` are dropped or folded.
    fn layout(&self, gone: &[bool]) -> Layout {
        let shown: Vec<usize> = (0..self.cols.len()).filter(|&i| !gone[i]).collect();
        let position = |header: &str| shown.iter().position(|&i| self.cols[i].header == header);
        let mut headers: Vec<&'static str> = shown.iter().map(|&i| self.cols[i].header).collect();
        let mut lower = vec![0; shown.len()];
        let mut rows: Vec<Vec<Text>> = self
            .rows
            .iter()
            .map(|r| shown.iter().map(|&i| r[i].clone()).collect())
            .collect();
        // Per row, whether the column folded last added a line to join.
        let mut added = vec![false; self.rows.len()];
        for (i, col) in self.cols.iter().enumerate() {
            let Some((_, How::Fold { into, join })) = col.narrow else {
                continue;
            };
            let Some(at) = position(into).filter(|_| gone[i]) else {
                added.fill(false);
                continue;
            };
            if let Some(h) = self.cols[shown[at]].folded_header {
                headers[at] = h;
            }
            for ((row, source), added) in rows.iter_mut().zip(&self.rows).zip(&mut added) {
                let add = &source[i].text;
                let joins = join && *added;
                *added = !add.is_empty();
                if add.is_empty() {
                    continue;
                }
                let cell = &mut row[at].text;
                if joins {
                    cell.push_str(" · ");
                } else if !cell.is_empty() {
                    cell.push('\n');
                }
                cell.push_str(add);
                let last = cell.lines().last().unwrap_or("");
                lower[at] = lower[at].max(visible_width(last));
            }
        }
        // Nor is a word broken to fit: an identifier or a link stays whole.
        for (k, &i) in shown.iter().enumerate() {
            if matches!(self.cols[i].fit, Fit::Fill) {
                let longest = rows
                    .iter()
                    .flat_map(|r| r[k].text.split_whitespace())
                    .map(visible_width)
                    .max()
                    .unwrap_or(0);
                lower[k] = lower[k].max(longest);
            }
        }
        Layout {
            shown,
            headers,
            rows,
            lower,
        }
    }

    /// Each shown column's widest line, header included.
    fn content(layout: &Layout) -> Vec<usize> {
        (0..layout.shown.len())
            .map(|k| {
                layout
                    .rows
                    .iter()
                    .map(|r| widest(&r[k].text))
                    .chain([visible_width(layout.headers[k])])
                    .max()
                    .unwrap_or(0)
            })
            .collect()
    }

    /// The width `layout` needs: each column's, plus borders and padding.
    fn needs(&self, layout: &Layout) -> usize {
        let widths = Grid::content(layout)
            .into_iter()
            .enumerate()
            .map(|(k, content)| match self.cols[layout.shown[k]].fit {
                Fit::Content => content,
                Fit::Cap(cap) => content.min(cap),
                Fit::Fill => content.min(FILL_MIN).max(layout.lower[k]),
            });
        1 + widths.map(|w| w + 3).sum::<usize>()
    }

    /// Each shown column's width in `width` columns: the others as they
    /// need, the title columns sharing what's left (one that needs less
    /// leaves the rest to the others), each at least as wide as the lines
    /// folded into it.
    fn widths(&self, layout: &Layout, width: usize) -> Vec<usize> {
        let content = Grid::content(layout);
        let fit = |k: usize| self.cols[layout.shown[k]].fit;
        let mut widths: Vec<usize> = (0..content.len())
            .map(|k| match fit(k) {
                Fit::Content => content[k],
                Fit::Cap(cap) => content[k].min(cap),
                Fit::Fill => 0,
            })
            .collect();
        let used = 1 + widths.iter().map(|w| w + 3).sum::<usize>();
        let mut left = width.saturating_sub(used);
        let mut wanting: Vec<usize> = (0..content.len())
            .filter(|&k| matches!(fit(k), Fit::Fill))
            .collect();
        while !wanting.is_empty() {
            let share = left / wanting.len();
            let (met, unmet): (Vec<usize>, Vec<usize>) =
                wanting.iter().partition(|&&k| content[k] <= share);
            if met.is_empty() {
                let extra = left % wanting.len();
                for (j, &k) in wanting.iter().enumerate() {
                    widths[k] = share + usize::from(j < extra);
                }
                break;
            }
            for k in met {
                widths[k] = content[k];
                left -= content[k];
            }
            wanting = unmet;
        }
        for (k, w) in widths.iter_mut().enumerate() {
            *w = (*w).max(layout.lower[k]).max(1);
        }
        widths
    }

    /// The table in `widths` (unbounded without them).
    fn table(&self, layout: &Layout, widths: Option<&[usize]>, colour: bool, ruled: bool) -> Table {
        let mut style = presets::UTF8_FULL_CONDENSED.with_rounded_corners();
        if ruled {
            style = style.row_separator(LineStyle::new('├', '╌', '┼', '┤'));
        }
        let mut t = Table::new();
        t.load_style(style)
            .force_no_tty()
            .set_content_arrangement(ContentArrangement::Dynamic);
        if colour {
            t.enforce_styling();
        }
        if let Some(widths) = widths {
            let total = 1 + widths.iter().map(|w| w + 3).sum::<usize>();
            t.set_width(u16::try_from(total).unwrap_or(u16::MAX));
        }
        t.set_header(layout.headers.iter().map(|h| {
            let cell = Cell::new(h);
            if colour {
                cell.add_attribute(Attribute::Bold)
            } else {
                cell
            }
        }));
        for row in &layout.rows {
            t.add_row(row.iter().map(|c| {
                let cell = Cell::new(&c.text);
                match c.level {
                    Some(level) if colour => cell.fg(match level {
                        Level::Good => Color::Green,
                        Level::Info => Color::Blue,
                        Level::Warning => Color::Yellow,
                        Level::Error => Color::Red,
                    }),
                    _ => cell,
                }
            }));
        }
        for (k, &i) in layout.shown.iter().enumerate() {
            let Some(column) = t.column_mut(k) else {
                continue;
            };
            if let Some(widths) = widths {
                // Widths count the padding either side.
                let w = u16::try_from(widths[k] + 2).unwrap_or(u16::MAX);
                column.set_constraint(ColumnConstraint::Absolute(Width::Fixed(w)));
            }
            if self.cols[i].right {
                column.set_cell_alignment(CellAlignment::Right);
            }
        }
        t
    }
}

fn widest(text: &str) -> usize {
    text.lines().map(visible_width).max().unwrap_or(0)
}

impl std::fmt::Display for Grid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.render(term::width(), term::colour()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn works() -> Grid {
        let mut g = Grid::new([
            Col::content("#").right(),
            Col::cap("Author", 18).folds(2, "Title"),
            Col::content("Year").folds_beside(2, "Title"),
            Col::fill("Title").or_when_folded("Work"),
            Col::content("ID").folds(1, "Title"),
            Col::content("Via").drops(0),
        ]);
        g.row([
            "1",
            "Krizhevsky et al.",
            "2017",
            "ImageNet classification with deep convolutional neural networks",
            "doi:10.1145/3065386",
            "openalex",
        ]);
        g.row([
            "2",
            "LeCun et al.",
            "2015",
            "Deep learning",
            "doi:10.1038/nature14539",
            "crossref",
        ]);
        g
    }

    #[test]
    fn wide_tables_keep_every_column_on_one_line_per_row() {
        let wide = works().render(Some(160), false);
        assert!(wide.contains("Via"), "{wide}");
        assert!(!wide.contains('╌'), "{wide}");
        assert_eq!(wide.lines().count(), 6, "{wide}");
    }

    #[test]
    fn rows_are_ruled_apart_only_when_many_wrap() {
        let list = |wrapped: usize| {
            let mut g = Grid::new([Col::content("Name"), Col::fill("Licence")]);
            for i in 0..20 {
                let licence = if i < wrapped {
                    "MIT OR Apache-2.0 OR Unicode-3.0 OR BSD-3-Clause"
                } else {
                    "MIT"
                };
                g.row([format!("crate{i}"), licence.to_owned()]);
            }
            g.render(Some(40), false)
        };
        // One in twenty reads as a continuation; five in twenty don't.
        assert!(!list(1).contains('╌'), "{}", list(1));
        assert!(list(5).contains('╌'), "{}", list(5));
    }

    #[test]
    fn narrow_tables_drop_then_fold_and_never_break_an_identifier() {
        let at = |w| works().render(Some(w), false);
        let mid = at(100);
        assert!(!mid.contains("Via") && mid.contains("ID"), "{mid}");
        let narrow = at(80);
        assert!(!narrow.contains(" ID "), "{narrow}");
        assert!(narrow.contains("doi:10.1038/nature14539"), "{narrow}");
        assert!(
            narrow.contains("Author") && narrow.contains("Work"),
            "{narrow}"
        );
        // Wrapped rows are ruled apart.
        assert!(narrow.contains('╌'), "{narrow}");
        let tight = at(50);
        assert!(
            tight.contains("Work") && !tight.contains("Author"),
            "{tight}"
        );
        assert!(tight.contains("LeCun et al. · 2015"), "{tight}");
        for table in [&mid, &narrow, &tight] {
            assert!(table.contains("doi:10.1145/3065386"), "{table}");
        }
        for (table, width) in [(&mid, 100), (&narrow, 80), (&tight, 50)] {
            for line in table.lines() {
                assert!(visible_width(line) <= width, "{line}");
            }
        }
    }
}
