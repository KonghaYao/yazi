//! Markdown → styled preview text.
//!
//! Ported from openai/codex codex-rs/tui/src/markdown_render.rs @ 9a59289fbea0e7e829c06aecfdd627651854da0f, Apache-2.0
//!
//! Upstream is coupled to Codex's own transcript, hyperlink and theme modules, so what is
//! carried over is its rendering policy — the heading weight ladder, the marker and quote
//! gutters, the `━`/`─` table rules with a 2-column gap, and keeping link destinations
//! visible — reimplemented against yazi's theme system and a `skip`-windowed preview so
//! scrolling works exactly like `ya.preview_code`.

use std::sync::LazyLock;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui_core::{style::{Color, Modifier, Style}, text::{Line, Span, Text}};
use syntect::{dumps, easy::HighlightLines, highlighting, highlighting::{Theme, ThemeSet}, parsing::{SyntaxReference, SyntaxSet}};
use unicode_width::UnicodeWidthChar;
use yazi_config::THEME;

use crate::{Styles, wrap};

const COLUMN_GAP: usize = 2;
const HEADER_SEPARATOR: char = '━';
const BODY_SEPARATOR: char = '─';
const QUOTE_BAR: &str = "│ ";
const BULLET: &str = "• ";
const CODE_INDENT: usize = 2;
const MIN_COLUMN: usize = 3;

/// A rendered window of a Markdown document.
pub struct Rendered {
	pub text:  Text<'static>,
	/// Lines the document produced in total, so the caller can bound the scroll offset.
	pub total: usize,
}

/// Render `source` for a `width`-column preview, showing `height` lines from `skip`.
pub fn render(source: &str, width: u16, skip: usize, height: usize) -> Rendered {
	let mut r = Renderer::new(Styles::new(), width.max(1) as usize);
	r.run(source);

	let total = r.lines.len();
	let text = Text::from(r.lines.into_iter().skip(skip).take(height).collect::<Vec<_>>());
	Rendered { text, total }
}

#[derive(Default)]
struct TableState {
	aligns: Vec<Alignment>,
	head:   Vec<Vec<Span<'static>>>,
	rows:   Vec<Vec<Vec<Span<'static>>>>,
	row:    Vec<Vec<Span<'static>>>,
	cell:   Option<Vec<Span<'static>>>,
}

struct Renderer {
	styles: Styles,
	width:  usize,
	lines:  Vec<Line<'static>>,

	/// Blockquote nesting, drawn as a bar gutter in front of everything else
	quote:  usize,
	/// Indentation inherited from enclosing list items
	base:   Vec<Span<'static>>,
	/// The marker of the list item the cursor is currently in
	marker: Vec<Span<'static>>,
	saved:  Vec<(Vec<Span<'static>>, Vec<Span<'static>>)>,
	lists:  Vec<Option<u64>>,

	inline:  Vec<Style>,
	heading: Option<u8>,
	link:    Option<(String, usize)>,
	pending: Vec<Span<'static>>,

	code:      Option<Vec<String>>,
	code_lang: Option<String>,
	table:     Option<TableState>,
}

impl Renderer {
	fn new(styles: Styles, width: usize) -> Self {
		Self {
			styles,
			width,
			lines: Vec::new(),
			quote: 0,
			base: Vec::new(),
			marker: Vec::new(),
			saved: Vec::new(),
			lists: Vec::new(),
			inline: Vec::new(),
			heading: None,
			link: None,
			pending: Vec::new(),
			code: None,
			code_lang: None,
			table: None,
		}
	}

	fn run(&mut self, source: &str) {
		let mut options = Options::empty();
		options.insert(Options::ENABLE_STRIKETHROUGH);
		options.insert(Options::ENABLE_TABLES);
		options.insert(Options::ENABLE_TASKLISTS);
		options.insert(Options::ENABLE_FOOTNOTES);

		for event in Parser::new_ext(source, options) {
			self.event(event);
		}

		self.flush();
	}

	fn event(&mut self, event: Event<'_>) {
		match event {
			Event::Start(tag) => self.start(tag),
			Event::End(tag) => self.end(tag),
			Event::Text(t) => self.text(&t),
			Event::Code(t) => self.code_span(&t),
			Event::SoftBreak => self.soft_break(),
			Event::HardBreak => self.flush(),
			Event::Rule => self.rule(),
			Event::TaskListMarker(done) => self.task_list_marker(done),
			_ => {}
		}
	}

	// --- Blocks
	fn start(&mut self, tag: Tag<'_>) {
		match tag {
			Tag::Heading { level, .. } => {
				self.blank();
				self.heading = Some(level_of(level));
			}
			Tag::BlockQuote(_) => {
				self.blank();
				self.quote += 1;
			}
			Tag::CodeBlock(kind) => {
				self.flush();
				self.blank();
				self.code_lang = match kind {
					CodeBlockKind::Fenced(info) => {
						info.split_whitespace().next().filter(|s| !s.is_empty()).map(str::to_string)
					}
					CodeBlockKind::Indented => None,
				};
				self.code = Some(Vec::new());
			}
			Tag::List(start) => {
				if self.lists.is_empty() {
					self.blank();
				}
				self.lists.push(start);
			}
			Tag::Item => {
				self.flush();
				let (marker, style) = match self.lists.last_mut() {
					Some(Some(n)) => {
						let s = format!("{n}. ");
						*n += 1;
						(s, self.styles.list)
					}
					_ => (BULLET.to_string(), self.styles.list),
				};

				let outer_base = std::mem::take(&mut self.base);
				let outer_marker = std::mem::take(&mut self.marker);

				// A nested item is indented past the marker of the item that contains it
				self.base = outer_base.clone();
				if !outer_marker.is_empty() {
					self.base.push(Span::raw(" ".repeat(wrap::width_of(&outer_marker))));
				}
				self.marker = vec![Span::styled(marker, style)];

				self.saved.push((outer_base, outer_marker));
			}
			Tag::Table(aligns) => {
				self.flush();
				self.blank();
				self.table = Some(TableState { aligns, ..Default::default() });
			}
			Tag::TableHead => {
				if let Some(t) = &mut self.table {
					t.row.clear();
				}
			}
			Tag::TableRow => {
				if let Some(t) = &mut self.table {
					t.row.clear();
				}
			}
			Tag::TableCell => {
				if let Some(t) = &mut self.table {
					t.cell = Some(Vec::new());
				}
			}
			Tag::Emphasis => self.inline.push(Style::default().add_modifier(Modifier::ITALIC)),
			Tag::Strong => self.inline.push(Style::default().add_modifier(Modifier::BOLD)),
			Tag::Strikethrough => self.inline.push(Style::default().add_modifier(Modifier::CROSSED_OUT)),
			Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
				self.inline.push(self.styles.link);
				self.link = Some((dest_url.to_string(), self.pending.len()));
			}
			_ => {}
		}
	}

	fn end(&mut self, tag: TagEnd) {
		match tag {
			TagEnd::Paragraph => self.flush(),
			TagEnd::Heading(_) => {
				self.heading = None;
				self.blank();
			}
			TagEnd::BlockQuote(_) => {
				self.flush();
				self.quote = self.quote.saturating_sub(1);
				self.blank();
			}
			TagEnd::CodeBlock => {
				self.flush();
				let raw = self.code.take().unwrap_or_default().concat();
				let lang = self.code_lang.take();
				self.code_block(&raw, lang.as_deref());
				self.lines.push(Line::default());
			}
			TagEnd::Item => {
				self.flush();
				if let Some((base, marker)) = self.saved.pop() {
					self.base = base;
					self.marker = marker;
				}
			}
			TagEnd::List(_) => {
				self.lists.pop();
				if self.lists.is_empty() {
					self.blank();
				}
			}
			TagEnd::Table => {
				if let Some(t) = self.table.take() {
					self.table_block(t);
				}
				self.lines.push(Line::default());
			}
			TagEnd::TableHead => {
				if let Some(t) = &mut self.table {
					t.head = std::mem::take(&mut t.row);
				}
			}
			TagEnd::TableRow => {
				if let Some(t) = &mut self.table
					&& !t.row.is_empty()
				{
					let row = std::mem::take(&mut t.row);
					t.rows.push(row);
				}
			}
			TagEnd::TableCell => {
				if let Some(t) = &mut self.table
					&& let Some(cell) = t.cell.take()
				{
					t.row.push(cell);
				}
			}
			TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
				self.inline.pop();
			}
			TagEnd::Link => self.end_link(false),
			TagEnd::Image => self.end_link(true),
			_ => {}
		}
	}

	// --- Inlines
	fn text(&mut self, t: &str) {
		let style = self.inline_style();

		if let Some(code) = self.code.as_mut() {
			code.push(t.to_string());
		} else if let Some(cell) = self.table.as_mut().and_then(|t| t.cell.as_mut()) {
			cell.push(Span::styled(t.to_string(), style));
		} else {
			self.pending.push(Span::styled(t.to_string(), style));
		}
	}

	fn code_span(&mut self, t: &str) {
		let style = self.inline_style().patch(self.styles.code);
		if let Some(cell) = self.table.as_mut().and_then(|t| t.cell.as_mut()) {
			cell.push(Span::styled(t.to_string(), style));
		} else {
			self.pending.push(Span::styled(t.to_string(), style));
		}
	}

	fn soft_break(&mut self) {
		if let Some(code) = self.code.as_mut() {
			code.push("\n".to_string());
		} else if let Some(cell) = self.table.as_mut().and_then(|t| t.cell.as_mut()) {
			cell.push(Span::raw(" "));
		} else {
			self.pending.push(Span::raw(" "));
		}
	}

	fn task_list_marker(&mut self, done: bool) {
		let text = if done { "[x] " } else { "[ ] " };
		self.marker.push(Span::styled(text, self.styles.list));
	}

	fn end_link(&mut self, image: bool) {
		self.inline.pop();
		let Some((dest, start)) = self.link.take() else {
			return;
		};
		if dest.is_empty() {
			return;
		}

		let style = self.styles.link.add_modifier(Modifier::DIM);
		if image {
			self.pending.push(Span::styled(format!(" ({dest})"), style));
			return;
		}

		// Keep destinations visible, but only when the label does not already say it
		let label: String =
			self.pending.get(start..).unwrap_or_default().iter().map(|s| s.content.as_ref()).collect();
		let label = label.trim();
		let web = dest.starts_with("http://") || dest.starts_with("https://");
		if web && label != dest {
			self.pending.push(Span::styled(format!(" ({dest})"), style));
		}
	}

	fn inline_style(&self) -> Style {
		let mut style = Style::default();
		if let Some(level) = self.heading {
			style = style.patch(self.styles.heading(level));
		}
		for s in &self.inline {
			style = style.patch(*s);
		}
		style
	}

	// --- Lines
	fn flush(&mut self) {
		if self.pending.is_empty() {
			return;
		}

		let content = std::mem::take(&mut self.pending);
		let gutter = self.gutter();
		let cont = self.cont();
		self.lines.extend(wrap::wrap(content, &gutter, &cont, self.width));
	}

	/// A blank line, collapsed so consecutive blocks do not stack up several of them.
	fn blank(&mut self) {
		self.flush();
		if self.lines.last().is_some_and(|l| !l.spans.is_empty()) {
			self.lines.push(Line::default());
		}
	}

	fn quote_bars(&self) -> Vec<Span<'static>> {
		let mut out = Vec::new();
		for _ in 0..self.quote {
			out.push(Span::styled(QUOTE_BAR, self.styles.quote));
		}
		out
	}

	fn gutter(&self) -> Vec<Span<'static>> {
		let mut out = self.quote_bars();
		out.extend(self.base.iter().cloned());
		out.extend(self.marker.iter().cloned());
		out
	}

	/// Continuation lines hang under the text of the item, not under its marker.
	fn cont(&self) -> Vec<Span<'static>> {
		let mut out = self.quote_bars();
		out.extend(self.base.iter().cloned());
		if !self.marker.is_empty() {
			out.push(Span::raw(" ".repeat(wrap::width_of(&self.marker))));
		}
		out
	}

	fn rule(&mut self) {
		self.flush();
		let cont = self.cont();
		let width = self.width.saturating_sub(wrap::width_of(&cont)).max(1);
		let mut spans = cont;
		spans.push(Span::styled(BODY_SEPARATOR.to_string().repeat(width), self.styles.rule));
		self.lines.push(Line::from(spans));
		self.lines.push(Line::default());
	}

	// --- Code
	fn code_block(&mut self, raw: &str, lang: Option<&str>) {
		let mut first = self.gutter();
		first.push(Span::raw(" ".repeat(CODE_INDENT)));
		let mut cont = self.cont();
		cont.push(Span::raw(" ".repeat(CODE_INDENT)));

		let lines = highlight(raw, lang, &self.styles);
		for spans in lines {
			self.lines.extend(wrap::wrap(spans, &first, &cont, self.width));
		}
	}

	// --- Tables
	fn table_block(&mut self, table: TableState) {
		let styles = self.styles;
		let ncols = table.aligns.len().max(1);

		let head = normalize(&table.head, ncols);
		let rows: Vec<_> = table.rows.into_iter().map(|r| normalize(&r, ncols)).collect();

		let mut widths = vec![MIN_COLUMN; ncols];
		for row in std::iter::once(&head).chain(rows.iter()) {
			for (i, cell) in row.iter().enumerate() {
				widths[i] = widths[i].max(wrap::width_of(cell));
			}
		}

		// Shrink the widest column first, so an oversized path does not squeeze out prose
		let avail = self.width.saturating_sub(COLUMN_GAP * ncols.saturating_sub(1));
		while widths.iter().sum::<usize>() > avail {
			let Some(i) = (0..ncols).max_by_key(|&i| widths[i]) else {
				break;
			};
			if widths[i] <= MIN_COLUMN {
				break;
			}
			widths[i] -= 1;
		}

		let gutter = self.gutter();
		let cont = self.cont();
		let total = widths.iter().sum::<usize>() + COLUMN_GAP * ncols.saturating_sub(1);

		let mut first = gutter;
		first.extend(cells(&head, &widths, true, &styles));
		self.lines.push(Line::from(first));

		let mut sep = cont.clone();
		sep.push(Span::styled(HEADER_SEPARATOR.to_string().repeat(total), styles.rule));
		self.lines.push(Line::from(sep));

		for (i, row) in rows.iter().enumerate() {
			if i > 0 {
				let mut sep = cont.clone();
				sep.push(Span::styled(BODY_SEPARATOR.to_string().repeat(total), styles.rule));
				self.lines.push(Line::from(sep));
			}

			let mut spans = cont.clone();
			spans.extend(cells(row, &widths, false, &styles));
			self.lines.push(Line::from(spans));
		}
	}
}

fn normalize(row: &[Vec<Span<'static>>], ncols: usize) -> Vec<Vec<Span<'static>>> {
	let mut out: Vec<Vec<Span<'static>>> = row.to_vec();
	out.resize(ncols, Vec::new());
	out
}

fn cells(
	row: &[Vec<Span<'static>>],
	widths: &[usize],
	header: bool,
	styles: &Styles,
) -> Vec<Span<'static>> {
	let mut out = Vec::new();
	for (i, cell) in row.iter().enumerate() {
		if i > 0 {
			out.push(Span::raw(" ".repeat(COLUMN_GAP)));
		}

		let mut spans = pad(cell, widths[i], styles.rule);
		if header {
			let style = styles.heading.add_modifier(Modifier::BOLD);
			for s in &mut spans {
				s.style = s.style.patch(style);
			}
		}
		out.extend(spans);
	}
	out
}

/// Fit a cell into exactly `width` columns, never splitting a wide character.
fn pad(cell: &[Span<'static>], width: usize, filler: Style) -> Vec<Span<'static>> {
	let mut chars: Vec<(char, Style)> = Vec::new();
	let mut used = 0;
	let mut truncated = false;

	'outer: for s in cell {
		for c in s.content.chars() {
			let w = c.width().unwrap_or(0);
			if used + w > width {
				truncated = true;
				break 'outer;
			}
			chars.push((c, s.style));
			used += w;
		}
	}

	if truncated {
		while used >= width {
			let Some((c, _)) = chars.pop() else {
				break;
			};
			used -= c.width().unwrap_or(0);
		}
		chars.push(('…', filler));
		used += 1;
	}

	while used < width {
		chars.push((' ', filler));
		used += 1;
	}

	wrap::from_chars(&chars)
}

// --- Syntax highlighting
fn syntaxes() -> Option<&'static SyntaxSet> {
	static SET: LazyLock<Option<SyntaxSet>> =
		LazyLock::new(|| dumps::from_uncompressed_data(yazi_prebuilt::syntaxes()).ok());
	SET.as_ref()
}

fn theme() -> Option<&'static Theme> {
	static THEME_SET: LazyLock<Option<Theme>> = LazyLock::new(|| {
		let path = &*THEME.mgr.syntect_theme.load();
		std::fs::File::open(path.as_path())
			.map_err(syntect::LoadingError::Io)
			.and_then(|f| ThemeSet::load_from_reader(&mut std::io::BufReader::new(f)))
			.or_else(|_| {
				ThemeSet::load_from_reader(&mut std::io::Cursor::new(yazi_prebuilt::ansi_theme()))
			})
			.ok()
	});
	THEME_SET.as_ref()
}

fn find_syntax<'a>(set: &'a SyntaxSet, lang: Option<&str>) -> Option<&'a SyntaxReference> {
	let lang = lang?;
	set.find_syntax_by_token(lang).or_else(|| set.find_syntax_by_extension(lang))
}

fn highlight(raw: &str, lang: Option<&str>, styles: &Styles) -> Vec<Vec<Span<'static>>> {
	let plain = |raw: &str| {
		raw.lines().map(|l| vec![Span::styled(l.to_string(), styles.code)]).collect::<Vec<_>>()
	};

	let (Some(set), Some(theme)) = (syntaxes(), theme()) else {
		return plain(raw);
	};
	let Some(syntax) = find_syntax(set, lang) else {
		return plain(raw);
	};

	let mut hl = HighlightLines::new(syntax, theme);
	let mut out = Vec::new();
	for line in raw.lines() {
		let Ok(regions) = hl.highlight_line(line, set) else {
			return plain(raw);
		};

		let spans = regions
			.into_iter()
			.map(|(style, text)| Span { content: text.to_string().into(), style: convert(style) })
			.collect();
		out.push(spans);
	}
	out
}

fn convert(style: highlighting::Style) -> Style {
	let mut modifier = Modifier::empty();
	if style.font_style.contains(highlighting::FontStyle::BOLD) {
		modifier |= Modifier::BOLD;
	}
	if style.font_style.contains(highlighting::FontStyle::ITALIC) {
		modifier |= Modifier::ITALIC;
	}
	if style.font_style.contains(highlighting::FontStyle::UNDERLINE) {
		modifier |= Modifier::UNDERLINED;
	}

	Style { fg: ansi_color(style.foreground), add_modifier: modifier, ..Default::default() }
}

// Copied from https://github.com/sharkdp/bat/blob/master/src/terminal.rs
fn ansi_color(color: highlighting::Color) -> Option<Color> {
	if color.a == 0 {
		// Themes name the user-configurable palette as #RRGGBB00, where RR is the index
		Some(match color.r {
			0x00 => Color::Black,
			0x01 => Color::Red,
			0x02 => Color::Green,
			0x03 => Color::Yellow,
			0x04 => Color::Blue,
			0x05 => Color::Magenta,
			0x06 => Color::Cyan,
			0x07 => Color::White,
			n => Color::Indexed(n),
		})
	} else {
		Some(Color::Rgb(color.r, color.g, color.b))
	}
}

const fn level_of(level: HeadingLevel) -> u8 {
	match level {
		HeadingLevel::H1 => 1,
		HeadingLevel::H2 => 2,
		HeadingLevel::H3 => 3,
		HeadingLevel::H4 => 4,
		HeadingLevel::H5 => 5,
		_ => 6,
	}
}
