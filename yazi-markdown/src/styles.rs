//! The `[markdown]` theme section, as ratatui styles.
//!
//! Heading levels only ever add modifiers on top of one color, which is what makes the
//! hierarchy readable at a glance without six separate theme keys.

use ratatui_core::style::{Modifier, Style};
use yazi_config::THEME;

#[derive(Clone, Copy, Debug)]
pub struct Styles {
	pub heading: Style,
	pub code:    Style,
	pub quote:   Style,
	pub list:    Style,
	pub link:    Style,
	pub rule:    Style,
	pub badge:   Style,
}

impl Default for Styles {
	fn default() -> Self { Self::new() }
}

impl Styles {
	pub fn new() -> Self {
		let th = &THEME.markdown;
		Self {
			heading: th.heading.get().0,
			code:    th.code.get().0,
			quote:   th.quote.get().0,
			list:    th.list.get().0,
			link:    th.link.get().0,
			rule:    th.rule.get().0,
			badge:   th.badge.get().0,
		}
	}

	/// Headings step down in weight, so the outline reads as a hierarchy.
	pub fn heading(&self, level: u8) -> Style {
		match level {
			1 => self.heading.add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
			2 => self.heading.add_modifier(Modifier::BOLD),
			3 => self.heading.add_modifier(Modifier::BOLD | Modifier::ITALIC),
			4 => self.heading.add_modifier(Modifier::ITALIC),
			5 => self.heading.add_modifier(Modifier::ITALIC | Modifier::DIM),
			_ => self.heading.add_modifier(Modifier::DIM),
		}
	}

	/// The chip for the renderer that is *not* active reads as a quiet hint.
	pub fn badge_idle(&self) -> Style {
		self.badge.remove_modifier(Modifier::BOLD | Modifier::REVERSED).add_modifier(Modifier::DIM)
	}
}
