use ratatui_core::{layout::Rect, text::{Line, Text}};
use tokio::task::JoinHandle;
use unicode_width::UnicodeWidthChar;
use yazi_adapter::ADAPTOR;
use yazi_binding::Scope;
use yazi_config::YAZI;
use yazi_fs::{FsHash64, file::File};
use yazi_macro::render;
use yazi_runner::{RUNNER, previewer::{PeekError, PeekJob}};
use yazi_shared::{id::Id, pool::Symbol, url::UrlBuf};

use crate::{AppProxy, Highlighter, MgrProxy, tab::{PreviewLock, PreviewSig}};

#[derive(Default)]
pub struct Preview {
	pub lock:        Option<PreviewLock>,
	pub skip:        usize,
	pub folder_lock: Option<UrlBuf>,
	pub selection:   Option<PreviewSelection>,

	handle: Option<JoinHandle<()>>,
	scope:  Scope,
}

impl Preview {
	pub fn go(&mut self, tab: Id, file: File, mime: Symbol<str>, force: bool) {
		if mime.is_empty() {
			return; // Wait till mimetype is resolved to avoid flickering
		}

		let sig = PreviewSig::new(&file, &mime).hash_id();
		if !force && self.same_lock(sig) {
			return;
		}

		let Some(previewer) = YAZI.plugin.previewers.matches(&file, &mime) else {
			return self.reset();
		};

		self.abort();
		self.scope = Scope::new();

		let job = PeekJob { tab, previewer, file, mime, sig, skip: self.skip };
		let scope = self.scope.clone();

		self.handle = Some(tokio::spawn(async move {
			let mut rx = RUNNER.peek(&job).await;
			match rx.recv().await.unwrap_or(Err(PeekError::Cancelled)) {
				Ok(()) | Err(PeekError::Cancelled) => {}
				Err(PeekError::ShouldSync) => AppProxy::plugin_peek(job, scope),
				Err(e) => MgrProxy::update_peeked_error(job, e.to_string(), scope),
			}
		}));
	}

	fn abort(&mut self) {
		self.handle.take().map(|ct| ct.abort());
		self.scope.take().cancel();
		Highlighter::abort();
	}

	pub fn reset(&mut self) {
		self.abort();
		ADAPTOR.image_hide().ok();
		render!(self.lock.take().is_some())
	}

	pub fn reset_image(&mut self) {
		self.abort();
		ADAPTOR.image_hide().ok();
	}

	pub fn same_url(&self, url: &UrlBuf) -> bool { matches!(&self.lock, Some(l) if l.url == *url) }

	pub fn same_folder(&self, url: &UrlBuf) -> bool { self.folder_lock.as_ref() == Some(url) }

	pub fn same_file(&self, file: &File, mime: &str) -> bool {
		self.same_url(&file.url)
			&& matches!(&self.lock, Some(l) if l.sig == PreviewSig::new(file, mime).hash_id())
	}

	fn same_lock(&self, sig: Id) -> bool {
		self.lock.as_ref().is_some_and(|l| l.sig == sig && l.skip == self.skip)
	}
}

// --- Selection

/// A char-wise selection in a text preview, kept as `(screen row, char index)` pairs so
/// wide characters are never cut in half
#[derive(Clone, Debug)]
pub struct PreviewSelection {
	pub anchor: (usize, usize),
	pub cursor: (usize, usize),
	pub url:    UrlBuf,
	pub sig:    Id,
	pub skip:   usize,
	pub area:   Rect,
}

impl PreviewSelection {
	/// Whether this selection still describes what the lock is showing: a different file,
	/// scroll, re-peek or window size invalidates it
	pub fn matches(&self, lock: &PreviewLock) -> bool {
		self.url == lock.url
			&& self.sig == lock.sig
			&& self.skip == lock.skip
			&& self.area == *lock.area
	}

	fn chars(line: &Line<'_>) -> Vec<char> {
		line.spans.iter().flat_map(|s| s.content.chars()).collect()
	}

	fn width(ch: char) -> usize { ch.width().unwrap_or(0) }

	/// The character under a cell column of a rendered row, snapping a wide character to
	/// its first column
	pub fn char_at(line: &Line<'_>, col: usize) -> usize {
		let (mut cell, mut idx) = (0, 0);
		for ch in Self::chars(line) {
			let w = Self::width(ch);
			if w > 0 && col < cell + w {
				return idx;
			}

			cell += w;
			idx += 1;
		}

		idx
	}

	/// The cell column where a character starts, so a wide character always starts there
	pub fn cell_at(line: &Line<'_>, idx: usize) -> usize {
		Self::chars(line).iter().take(idx).map(|&ch| Self::width(ch)).sum()
	}

	/// The index just past the character at `idx`, together with its zero-width marks
	fn end_at(line: &Line<'_>, idx: usize) -> usize {
		let chars = Self::chars(line);
		let mut end = (idx + 1).min(chars.len());
		while end < chars.len() && Self::width(chars[end]) == 0 {
			end += 1;
		}

		end
	}

	fn bounds(&self) -> ((usize, usize), (usize, usize)) {
		if self.anchor <= self.cursor { (self.anchor, self.cursor) } else { (self.cursor, self.anchor) }
	}

	/// The selected cell range of every selected row
	pub fn ranges(&self, text: &Text) -> Vec<(usize, usize, usize)> {
		let ((r0, c0), (r1, c1)) = self.bounds();
		let mut out = Vec::new();

		for r in r0..=r1.min(text.lines.len().saturating_sub(1)) {
			let line = &text.lines[r];
			let from = if r == r0 { Self::cell_at(line, c0) } else { 0 };
			let to = if r == r1 { Self::cell_at(line, Self::end_at(line, c1)) } else { line.width() };
			out.push((r, from, to));
		}

		out
	}

	/// The selected text: whole rows in between, the rest cut at the character boundaries
	pub fn text(&self, text: &Text) -> String {
		let ((r0, c0), (r1, c1)) = self.bounds();
		let mut out = String::new();

		for r in r0..=r1.min(text.lines.len().saturating_sub(1)) {
			if r > r0 {
				out.push('\n');
			}

			let line = &text.lines[r];
			let chars = Self::chars(line);
			let from = if r == r0 { c0.min(chars.len()) } else { 0 };
			let to = if r == r1 { Self::end_at(line, c1) } else { chars.len() };
			out.extend(chars[from.min(to)..to].iter());
		}

		out
	}
}
