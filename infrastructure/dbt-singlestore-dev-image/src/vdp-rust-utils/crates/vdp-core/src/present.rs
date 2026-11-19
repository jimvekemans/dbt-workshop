//! Plain-text tables and 88-column Unicode callout panels for terminal output.

use std::fmt::Write as _;

/// Default outer width in columns for a rendered [`Panel`].
pub const PANEL_WIDTH: usize = 88;

const MIN_PANEL_WIDTH: usize = 6;

/// Rounded Unicode callout box wrapped to a fixed outer width (default: 88 columns).
///
/// # Examples
///
/// ```
/// use vdp_core::present::Panel;
///
/// let rendered = Panel::new("Notice", "ready").width(14).render();
/// assert_eq!(
///     rendered,
///     "╭─ Notice ───╮\n│ ready      │\n╰────────────╯\n"
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    title: String,
    body: String,
    width: usize,
    preformatted: bool,
    preformatted_tail: Option<String>,
}

impl Panel {
    /// Creates a titled panel with outer width [`PANEL_WIDTH`].
    #[must_use]
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            body: body.into(),
            width: PANEL_WIDTH,
            preformatted: false,
            preformatted_tail: None,
        }
    }

    /// Creates an untitled panel with outer width [`PANEL_WIDTH`].
    #[must_use]
    pub fn untitled(body: impl Into<String>) -> Self {
        Self::new("", body)
    }

    /// Sets the total outer width in terminal columns (clamped to at least 6).
    #[must_use]
    pub const fn width(mut self, width: usize) -> Self {
        self.width = if width < MIN_PANEL_WIDTH {
            MIN_PANEL_WIDTH
        } else {
            width
        };
        self
    }

    /// Preserves all whitespace and indentation, expanding tabs to 4-column stops and hard-wrapping lines only at the inner box width (`width - 4`).
    #[must_use]
    pub const fn preformatted(mut self) -> Self {
        self.preformatted = true;
        self
    }

    /// Appends a preformatted block after the word-wrapped body, expanding tabs to 4-column stops
    /// and hard-wrapping lines only at the inner box width (`width - 4`).
    #[must_use]
    pub fn preformatted_block(mut self, block: impl Into<String>) -> Self {
        self.preformatted_tail = Some(block.into());
        self
    }

    /// Renders the box with a trailing newline.
    #[must_use]
    pub fn render(&self) -> String {
        let width = self.width.max(MIN_PANEL_WIDTH);
        let inner = width - 4;
        let mut out = String::new();

        let title = self.title.trim();
        if title.is_empty() {
            let _ = writeln!(out, "╭{}╮", "─".repeat(width - 2));
        } else {
            let max_title = width - 5;
            let title = format_panel_title(title, max_title);
            let fill = width - 5 - title.chars().count();
            let _ = writeln!(out, "╭─ {title} {}╮", "─".repeat(fill));
        }

        let mut lines = if self.preformatted {
            wrap_preformatted(&self.body, inner)
        } else {
            wrap_body(&self.body, inner)
        };
        if let Some(tail) = &self.preformatted_tail {
            lines.extend(wrap_preformatted(tail, inner));
        }
        for line in lines {
            let pad = inner.saturating_sub(line.chars().count());
            let _ = writeln!(out, "│ {line}{} │", " ".repeat(pad));
        }

        let _ = writeln!(out, "╰{}╯", "─".repeat(width - 2));
        out
    }
}

fn format_panel_title(title: &str, max_title: usize) -> String {
    if title.chars().count() <= max_title {
        return title.to_owned();
    }
    if title.ends_with(']')
        && let Some((prefix, kind_and_bracket)) = title.rsplit_once(" [")
    {
        let suffix = format!(" [{kind_and_bracket}");
        let suffix_len = suffix.chars().count();
        if suffix_len + 1 < max_title {
            let mut out: String = prefix.chars().take(max_title - suffix_len - 1).collect();
            out.push('…');
            out.push_str(&suffix);
            return out;
        }
    }
    title.chars().take(max_title).collect()
}

fn expand_tabs(line: &str, tab_stop: usize) -> Vec<char> {
    let mut out = Vec::with_capacity(line.len());
    for ch in line.chars() {
        if ch == '\t' {
            let spaces = tab_stop - (out.len() % tab_stop);
            out.extend(std::iter::repeat_n(' ', spaces));
        } else {
            out.push(ch);
        }
    }
    out
}

fn wrap_preformatted(body: &str, max_width: usize) -> Vec<String> {
    if body.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    for line in body.lines() {
        if line.is_empty() {
            lines.push(String::new());
        } else {
            let chars = expand_tabs(line, 4);
            for chunk in chars.chunks(max_width) {
                lines.push(chunk.iter().collect());
            }
        }
    }
    lines
}

fn wrap_body(body: &str, max_width: usize) -> Vec<String> {
    if body.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    for paragraph in body.lines() {
        if paragraph.is_empty() {
            lines.push(String::new());
        } else if paragraph.chars().count() <= max_width {
            lines.push(paragraph.to_owned());
        } else {
            wrap_paragraph(paragraph, max_width, &mut lines);
        }
    }
    lines
}

fn wrap_paragraph(paragraph: &str, max_width: usize, out: &mut Vec<String>) {
    let mut current = String::new();
    let mut current_len = 0;

    for word in paragraph.split_whitespace() {
        let word_len = word.chars().count();
        if current_len > 0 && current_len + 1 + word_len <= max_width {
            current.push(' ');
            current.push_str(word);
            current_len += 1 + word_len;
            continue;
        }
        if !current.is_empty() {
            out.push(std::mem::take(&mut current));
            current_len = 0;
        }
        if word_len <= max_width {
            current.push_str(word);
            current_len = word_len;
        } else {
            let chars: Vec<char> = word.chars().collect();
            for chunk in chars.chunks(max_width) {
                if chunk.len() == max_width {
                    out.push(chunk.iter().collect());
                } else {
                    current = chunk.iter().collect();
                    current_len = chunk.len();
                }
            }
        }
    }

    if !current.is_empty() || out.is_empty() {
        out.push(current);
    }
}

/// Left-aligned plain-text table builder.
///
/// Defaults to two-space column separation (`"  "`), no divider row, and unbounded column width.
/// Call [`Table::separator`], [`Table::divider`], and [`Table::max_col_width`] to customise layout.
///
/// # Examples
///
/// ```
/// use vdp_core::present::Table;
///
/// let out = Table::new(["id", "name"])
///     .rows([["1", "alice"], ["22", "bob"]])
///     .separator(" | ")
///     .divider("-+-")
///     .render();
/// assert_eq!(out, "id | name\n---+------\n1  | alice\n22 | bob\n");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    separator: String,
    divider: Option<String>,
    max_col_width: Option<usize>,
}

impl Table {
    /// Creates a table with column headers and default spacing.
    #[must_use]
    pub fn new<I, S>(headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            headers: headers.into_iter().map(Into::into).collect(),
            rows: Vec::new(),
            separator: "  ".to_owned(),
            divider: None,
            max_col_width: None,
        }
    }

    /// Appends a single row.
    #[must_use]
    pub fn row<I, S>(mut self, cells: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rows.push(cells.into_iter().map(Into::into).collect());
        self
    }

    /// Appends multiple rows.
    #[must_use]
    pub fn rows<R, I, S>(mut self, rows: R) -> Self
    where
        R: IntoIterator<Item = I>,
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rows.extend(
            rows.into_iter()
                .map(|r| r.into_iter().map(Into::into).collect()),
        );
        self
    }

    /// Sets the column separator (default `"  "`).
    #[must_use]
    pub fn separator(mut self, separator: impl Into<String>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Enables a rule between the header and rows, joining `"-".repeat(width)` segments with `divider`.
    #[must_use]
    pub fn divider(mut self, divider: impl Into<String>) -> Self {
        self.divider = Some(divider.into());
        self
    }

    /// Caps every column width to `max` characters, truncating wider values.
    #[must_use]
    pub const fn max_col_width(mut self, max: usize) -> Self {
        self.max_col_width = Some(max);
        self
    }

    /// Renders the aligned table with trailing whitespace trimmed on each line.
    #[must_use]
    pub fn render(&self) -> String {
        if self.headers.is_empty() {
            return String::new();
        }
        let widths: Vec<usize> = self
            .headers
            .iter()
            .enumerate()
            .map(|(i, header)| {
                let width = self
                    .rows
                    .iter()
                    .map(|row| row.get(i).map_or(0, |cell| cell.chars().count()))
                    .chain([header.chars().count()])
                    .max()
                    .unwrap_or(0);
                self.max_col_width.map_or(width, |cap| width.min(cap))
            })
            .collect();

        let format_line = |cells: &[String]| -> String {
            cells
                .iter()
                .zip(&widths)
                .map(|(cell, width)| {
                    let truncated: String = cell.chars().take(*width).collect();
                    format!("{truncated:<width$}")
                })
                .collect::<Vec<_>>()
                .join(&self.separator)
                .trim_end()
                .to_owned()
        };

        let mut out = String::new();
        let _ = writeln!(out, "{}", format_line(&self.headers));
        if let Some(divider) = &self.divider {
            let rule: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            let _ = writeln!(out, "{}", rule.join(divider));
        }
        for row in &self.rows {
            let _ = writeln!(out, "{}", format_line(row));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_wraps_at_88_columns() {
        let body = "SingleStore deployment prepared 42 DDL scripts cleanly and archived the previous staged artifacts before exit.";
        let rendered = Panel::new("Deploy", body).render();
        let expected = concat!(
            "╭─ Deploy ─────────────────────────────────────────────────────────────────────────────╮\n",
            "│ SingleStore deployment prepared 42 DDL scripts cleanly and archived the previous     │\n",
            "│ staged artifacts before exit.                                                        │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
        );
        assert_eq!(rendered, expected);
        assert!(rendered.lines().all(|line| line.chars().count() == 88));
    }

    #[test]
    fn panel_hard_wraps_word_longer_than_inner_width() {
        let body = format!("{} ok", "x".repeat(90));
        let rendered = Panel::new("Long", body).render();
        let expected = format!(
            concat!(
                "╭─ Long ───────────────────────────────────────────────────────────────────────────────╮\n",
                "│ {} │\n",
                "│ xxxxxx ok                                                                            │\n",
                "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
            ),
            "x".repeat(84)
        );
        assert_eq!(rendered, expected);
        assert!(rendered.lines().all(|line| line.chars().count() == 88));
    }

    #[test]
    fn panel_renders_empty_body_and_untitled_boxes_cleanly() {
        let titled = Panel::new("Empty", "").render();
        let expected_titled = concat!(
            "╭─ Empty ──────────────────────────────────────────────────────────────────────────────╮\n",
            "│                                                                                      │\n",
            "╰──────────────────────────────────────────────────────────────────────────────────────╯\n",
        );
        assert_eq!(titled, expected_titled);

        let untitled = Panel::untitled("").width(12).render();
        assert_eq!(untitled, "╭──────────╮\n│          │\n╰──────────╯\n");
    }

    #[test]
    fn panel_clamps_oversized_title_to_outer_width() {
        let rendered = Panel::new("t".repeat(120), "hi").render();
        let first_line = rendered.lines().next().unwrap();
        assert_eq!(first_line.chars().count(), 88);
        assert_eq!(first_line, format!("╭─ {} ╮", "t".repeat(83)));
    }

    #[test]
    fn panel_ellipsizes_long_title_while_preserving_bracketed_kind() {
        let title = format!("FAILED {} [CREATE PROCEDURE]", "j".repeat(120));
        let rendered = Panel::new(title, "ok").render();
        let first_line = rendered.lines().next().unwrap();
        assert_eq!(first_line.chars().count(), 88);
        assert!(
            first_line.ends_with("… [CREATE PROCEDURE] ╮"),
            "{first_line}"
        );

        let impossible_suffix = Panel::new("x [VERY LONG KIND LABEL]", "ok")
            .width(12)
            .render();
        let short_first_line = impossible_suffix.lines().next().unwrap();
        assert_eq!(short_first_line.chars().count(), 12);
    }

    #[test]
    fn panel_preformatted_preserves_whitespace_and_round_trips_wrapped_lines() {
        let sql = "  SELECT 'a   b', c_0123456789";
        let rendered = Panel::new("SQL", sql).width(24).preformatted().render();
        let expected = concat!(
            "╭─ SQL ────────────────╮\n",
            "│   SELECT 'a   b', c_ │\n",
            "│ 0123456789           │\n",
            "╰──────────────────────╯\n",
        );
        assert_eq!(rendered, expected);
        let inner_joined: String = rendered
            .lines()
            .filter_map(|line| line.strip_prefix("│ ").and_then(|s| s.strip_suffix(" │")))
            .enumerate()
            .map(|(i, s)| if i == 1 { s.trim_end_matches(' ') } else { s })
            .collect();
        assert_eq!(inner_joined, sql);
    }

    #[test]
    fn panel_preformatted_expands_tabs_to_4_column_stops() {
        let rendered = Panel::new("SQL", "\tSELECT\nab\t'x'")
            .width(18)
            .preformatted()
            .render();
        let expected = concat!(
            "╭─ SQL ──────────╮\n",
            "│     SELECT     │\n",
            "│ ab  'x'        │\n",
            "╰────────────────╯\n",
        );
        assert_eq!(rendered, expected);
        assert!(!rendered.contains('\t'));
        assert!(rendered.lines().all(|line| line.chars().count() == 18));
    }

    #[test]
    fn table_aligns_columns_and_caps_width() {
        let plain = Table::new(["col", "val"])
            .row(["a", "1"])
            .row(["longer", "222"])
            .render();
        assert_eq!(plain, "col     val\na       1\nlonger  222\n");

        let ruled = Table::new(["k", "description"])
            .row(["1", "abcdefghij"])
            .separator(" | ")
            .divider("-+-")
            .max_col_width(6)
            .render();
        assert_eq!(ruled, "k | descri\n--+-------\n1 | abcdef\n");

        assert_eq!(Table::new(Vec::<String>::new()).render(), "");
    }
}
