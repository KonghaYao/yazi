use ratatui_core::{buffer::Buffer, style::Modifier, widgets::Widget};
use yazi_binding::elements::Spatial;
use yazi_config::LAYOUT;
use yazi_core::Core;
use yazi_shared::url::UrlLike;
use yazi_widgets::Renderable;

pub(crate) struct Preview<'a> {
	core: &'a Core,
}

impl<'a> Preview<'a> {
	#[inline]
	pub(crate) fn new(core: &'a Core) -> Self { Self { core } }

	/// The chip that switches the Markdown renderer, drawn last so that neither the
	/// preview content nor the selection highlight can cover it.
	fn badge(area: ratatui_core::layout::Rect, win: ratatui_core::layout::Rect, buf: &mut Buffer) {
		let rect = yazi_markdown::badge_rect(area);
		if rect.width == 0 || rect.height == 0 {
			return;
		}

		let mode = yazi_markdown::mode();
		let style = yazi_markdown::badge_style(mode);
		for (i, c) in yazi_markdown::badge_label(mode).chars().enumerate() {
			let x = rect.x + i as u16;
			if x >= rect.right() || x >= win.right() {
				break;
			}

			let cell = &mut buf[(x, rect.y)];
			cell.set_char(c);
			cell.set_style(style);
		}
	}
}

impl Widget for Preview<'_> {
	fn render(self, win: ratatui_core::layout::Rect, buf: &mut Buffer) {
		let Some(lock) = &self.core.active().preview.lock else {
			return;
		};

		if *lock.area != LAYOUT.get().preview {
			return;
		}

		for w in &lock.data {
			let rect = w.area().transform(|p| self.core.mgr.area(p));
			if rect.intersection(win) == rect {
				w.render(rect, buf);
			}
		}

		// The renderer chip sits on top of the content, and above the selection highlight
		if let [Renderable::Text(text)] = &lock.data[..] {
			let mime = self.core.mgr.mimetype.owned(&lock.url).unwrap_or_default();
			let ext = lock.url.ext().map(|e| e.to_string_lossy().into_owned());
			if yazi_markdown::is_markdown(&mime, ext.as_deref()) {
				let area = text.area().transform(|p| self.core.mgr.area(p));
				Self::badge(area, win, buf);
			}
		}

		// Highlight the selection on top of the content, keeping its syntax colors
		let Some(selection) = &self.core.active().preview.selection else {
			return;
		};
		let [Renderable::Text(text)] = &lock.data[..] else {
			return;
		};
		if !selection.matches(lock) {
			return;
		}

		let area = text.area().transform(|p| self.core.mgr.area(p));
		for (row, from, to) in selection.ranges(text.inner()) {
			for x in from..to {
				let (x, y) = (area.x + x as u16, area.y + row as u16);
				if x < area.right() && y < area.bottom() && x < win.right() && y < win.bottom() {
					buf[(x, y)].modifier |= Modifier::REVERSED;
				}
			}
		}
	}
}
