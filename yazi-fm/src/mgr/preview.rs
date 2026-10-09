use ratatui_core::{buffer::Buffer, style::Modifier, widgets::Widget};
use yazi_binding::elements::Spatial;
use yazi_config::LAYOUT;
use yazi_core::Core;
use yazi_widgets::Renderable;

pub(crate) struct Preview<'a> {
	core: &'a Core,
}

impl<'a> Preview<'a> {
	#[inline]
	pub(crate) fn new(core: &'a Core) -> Self { Self { core } }
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
