//! Width-aware wrapping for styled text.
//!
//! Adapted from the word-wrapping and width-measurement policy of `openai/codex`
//! (`codex-rs/tui/src/wrapping.rs` and `codex-rs/tui/src/width.rs`), rewritten on top of
//! `ratatui` spans and `unicode-width`.
//!
//! Ported from openai/codex codex-rs/tui/src/wrapping.rs @ 9a59289fbea0e7e829c06aecfdd627651854da0f, Apache-2.0
//!
//! Characters are the break unit, so a wide (CJK) character is never split across two lines,
//! and a break is preferred at the last space so words are not cut in half.

use ratatui_core::{
	style::Style,
	text::{Line, Span},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Display width of a run of spans.
pub fn width_of(spans: &[Span<'_>]) -> usize {
	spans.iter().map(|s| s.content.as_ref().width()).sum()
}

/// Coalesce styled characters back into as few spans as possible.
pub fn from_chars(chars: &[(char, Style)]) -> Vec<Span<'static>> {
	let mut out: Vec<Span<'static>> = Vec::new();
	for &(c, style) in chars {
		match out.last_mut() {
			Some(last) if last.style == style => last.content.to_mut().push(c),
			_ => out.push(Span::styled(c.to_string(), style)),
		}
	}
	out
}

fn flatten(spans: &[Span<'static>]) -> Vec<(char, Style)> {
	let mut out = Vec::new();
	for s in spans {
		for c in s.content.chars() {
			out.push((c, s.style));
		}
	}
	out
}

/// Wrap `spans` to `width` columns, prefixing the first line with `first` and every
/// following line with `cont`.
pub fn wrap(
	spans: Vec<Span<'static>>,
	first: &[Span<'static>],
	cont: &[Span<'static>],
	width: usize,
) -> Vec<Line<'static>> {
	let first_avail = width.saturating_sub(width_of(first)).max(1);
	let cont_avail = width.saturating_sub(width_of(cont)).max(1);

	let chars = flatten(&spans);
	let mut lines: Vec<Line<'static>> = Vec::new();
	let mut i = 0;

	while i < chars.len() {
		let limit = if lines.is_empty() { first_avail } else { cont_avail };

		// Greedy fill: never let a character cross the right edge, so wide characters stay whole
		let mut used = 0;
		let mut j = i;
		while j < chars.len() {
			let w = chars[j].0.width().unwrap_or(0);
			if used + w > limit {
				break;
			}
			used += w;
			j += 1;
		}
		// A single character wider than the whole line still has to go somewhere
		if j == i {
			j = i + 1;
		}

		let mut end = j;
		if j < chars.len() {
			// Back off to just before the last space, so words survive intact
			let mut k = j;
			while k > i && chars[k - 1].0 != ' ' {
				k -= 1;
			}
			if k > i {
				end = k - 1;
			}
		}
		while end > i && chars[end - 1].0 == ' ' {
			end -= 1;
		}

		if end > i {
			let gutter = if lines.is_empty() { first } else { cont };
			let mut line = gutter.to_vec();
			line.extend(from_chars(&chars[i..end]));
			lines.push(Line::from(line));
		}

		i = if end > i {
			// Swallow the space we broke at, so the next line does not start with one
			if end < chars.len() && chars[end].0 == ' ' { end + 1 } else { end }
		} else {
			i + 1
		};
	}

	lines
}
