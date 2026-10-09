//! Which renderer previews a Markdown file, and where that choice is remembered.
//!
//! The choice is a user preference, so it lives in the state directory rather than in
//! `yazi.toml` — switching it never rewrites the user's configuration.

use std::sync::{
	Once,
	atomic::{AtomicU8, Ordering},
};

use yazi_fs::Xdg;

/// How a Markdown file is turned into preview text.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Mode {
	/// The readable, Codex-style rendering (headings, lists, tables, …).
	#[default]
	Readable,
	/// The stock syntax-highlighted source view, as `ya.preview_code` shows it.
	Code,
}

impl Mode {
	#[inline]
	pub fn is_readable(self) -> bool { self == Self::Readable }

	pub const fn as_str(self) -> &'static str {
		match self {
			Self::Readable => "readable",
			Self::Code => "code",
		}
	}

	fn from_u8(n: u8) -> Self { if n == 1 { Self::Code } else { Self::Readable } }
}

/// `2` is "not read from disk yet", so that an unset preference still defaults to [`Mode::Readable`].
const UNSET: u8 = 2;

static MODE: AtomicU8 = AtomicU8::new(UNSET);
static LOADED: Once = Once::new();

fn path() -> std::path::PathBuf { Xdg::state_dir().join(".markdown") }

fn load() {
	let mode = std::fs::read_to_string(path())
		.ok()
		.and_then(|s| match s.trim() {
			"readable" => Some(Mode::Readable),
			"code" => Some(Mode::Code),
			_ => None,
		})
		.unwrap_or_default();

	MODE.store(mode as u8, Ordering::Relaxed);
}

/// Read the persisted choice. Called once at startup; also lazy on first use.
pub fn init() { LOADED.call_once(load); }

#[inline]
pub fn mode() -> Mode {
	init();
	Mode::from_u8(MODE.load(Ordering::Relaxed))
}

pub fn set_mode(mode: Mode) {
	init();
	MODE.store(mode as u8, Ordering::Relaxed);

	if let Err(e) = save(mode) {
		log::warn!("Failed to persist the Markdown renderer preference: {e}");
	}
}

#[inline]
pub fn toggle() -> Mode {
	let mode = if mode().is_readable() { Mode::Code } else { Mode::Readable };
	set_mode(mode);
	mode
}

fn save(mode: Mode) -> std::io::Result<()> {
	let dir = Xdg::state_dir();
	std::fs::create_dir_all(dir)?;
	std::fs::write(dir.join(".markdown"), format!("{}\n", mode.as_str()))
}

/// Extensions whose preview offers the renderer switch.
const EXTENSIONS: [&str; 6] = [ "md", "markdown", "mdown", "mkd", "mkdn", "mdx" ];

/// Whether a preview of this file offers the renderer switch. A MIME type is authoritative
/// when it is known, since `.md` does not always resolve to `text/markdown`.
pub fn is_markdown(mime: &str, ext: Option<&str>) -> bool {
	mime == "text/markdown" || ext.is_some_and(is_markdown_ext)
}

pub fn is_markdown_ext(ext: &str) -> bool {
	EXTENSIONS.iter().any(|e| ext.eq_ignore_ascii_case(e))
}
