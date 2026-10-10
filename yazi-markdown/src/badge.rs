//! The renderer toggle chip drawn at the top-right of a Markdown preview.

use ratatui_core::{layout::Rect, style::Style};

use crate::{Mode, Styles};

/// What the badge says. Pure ASCII, so it is exactly one cell per character everywhere.
pub const fn badge_label(mode: Mode) -> &'static str {
	match mode {
		Mode::Readable => "[md]",
		Mode::Code => "[src]",
	}
}

/// A chip for the active renderer reads loud; the alternative reads as a quiet hint.
pub fn badge_style(mode: Mode) -> Style {
	let styles = Styles::new();
	if mode.is_readable() { styles.badge } else { styles.badge_idle() }
}

/// The chip sits in the top-right corner of the preview's text area.
pub fn badge_rect(area: Rect) -> Rect {
	let width =
		(badge_label(Mode::Readable).len().max(badge_label(Mode::Code).len()) as u16).min(area.width);

	Rect { x: area.right().saturating_sub(width), y: area.y, width, height: area.height.min(1) }
}

/// Whether a screen position hits the chip. Checked before text selection, so clicking the
/// chip never starts a drag-select.
#[inline]
pub fn badge_hit(area: Rect, x: u16, y: u16) -> bool {
	let r = badge_rect(area);
	area.height > 0 && x >= r.x && x < r.right() && y >= r.y && y < r.bottom()
}
