use std::time::Duration;

use anyhow::Result;
use futures::executor::block_on;
use mlua::{ObjectLike, Table};
use ratatui_core::layout::{Position, Rect};
use yazi_actor::lives::Lives;
use yazi_binding::{elements::Spatial, runtime_scope};
use yazi_config::LAYOUT;
use yazi_core::{Ctx, notify::{MessageLevel, MessageOpt}, tab::PreviewSelection};
use yazi_macro::{log_if_err, render, succ};
use yazi_parser::app::MouseForm;
use yazi_plugin::LUA;
use yazi_proxy::NotifyProxy;
use yazi_shared::{Layer, data::Data};
use yazi_term::event::{MouseButton, MouseEvent, MouseEventKind};
use yazi_widgets::{CLIPBOARD, Renderable};

use crate::Actor;

pub struct Mouse;

/// What the native text selection in the preview wants to do with an event
enum Intercept {
	/// Not ours: hand the event to Lua as usual
	Forward,
	/// Consumed; the flag says whether the screen needs a repaint
	Handled(bool),
	/// Consumed as a click that never dragged: replay it as the stock press
	Replay,
}

impl Actor for Mouse {
	type Form = MouseForm;

	const NAME: &str = "mouse";

	fn act(cx: &mut Ctx, form: Self::Form) -> Result<Data> {
		let Some(size) = cx.term.as_ref().and_then(|t| t.size().ok()) else { succ!() };
		let area = yazi_binding::elements::Rect::from(size);

		// Selecting text in the preview pane is handled before Lua, and only in the manager layer
		let mut replay = false;
		if cx.layer() == Layer::Mgr {
			match Self::select(cx, form.event) {
				Intercept::Forward => {}
				Intercept::Handled(draw) => {
					render!(draw);
					succ!();
				}
				Intercept::Replay => replay = true,
			}
		}

		let result = Lives::scope(cx, move |cx| {
			runtime_scope!(cx, "root", {
				let root = LUA.globals().raw_get::<Table>("Root")?.call_method::<Table>("new", area)?;

				match form.event.kind {
					MouseEventKind::Down(_) => root.call_method("click", (form.event, false))?,
					// A click that never dragged is replayed as a stock press
					MouseEventKind::Up(_) => root.call_method("click", (form.event, !replay))?,

					MouseEventKind::ScrollDown => root.call_method("scroll", (form.event, 1))?,
					MouseEventKind::ScrollUp => root.call_method("scroll", (form.event, -1))?,

					MouseEventKind::ScrollRight => root.call_method("touch", (form.event, 1))?,
					MouseEventKind::ScrollLeft => root.call_method("touch", (form.event, -1))?,

					MouseEventKind::Moved => root.call_method("move", form.event)?,
					MouseEventKind::Drag(_) => root.call_method("drag", form.event)?,
				}

				Ok(())
			})
		});

		log_if_err!("Mouse event handler", &result);
		succ!(result?);
	}
}

impl Mouse {
	/// The screen rect of the preview, when it is showing a text preview
	fn text_area(cx: &Ctx) -> Option<Rect> {
		let lock = cx.active().preview.lock.as_ref()?;
		if *lock.area != LAYOUT.get().preview {
			return None;
		}

		let [Renderable::Text(text)] = &lock.data[..] else {
			return None;
		};

		Some(text.area().transform(|p| cx.core.mgr.area(p)))
	}

	/// The character under a screen position, snapped to a whole character
	fn char_at(cx: &Ctx, rect: Rect, event: MouseEvent) -> (usize, usize) {
		let row = event.row().saturating_sub(rect.y) as usize;
		let col = event.column().saturating_sub(rect.x) as usize;

		let Some(lock) = cx.active().preview.lock.as_ref() else {
			return (row, 0);
		};
		let [Renderable::Text(text)] = &lock.data[..] else {
			return (row, 0);
		};
		let Some(line) = text.inner().lines.get(row) else {
			return (row, 0);
		};

		(row, PreviewSelection::char_at(line, col))
	}

	/// Text selection in the preview pane, as in a terminal or an editor
	fn select(cx: &mut Ctx, event: MouseEvent) -> Intercept {
		let Some(rect) = Self::text_area(cx) else {
			// Nothing selectable on screen: drop any selection and let Lua have the event
			if cx.active_mut().preview.selection.take().is_some() {
				render!();
			}

			return Intercept::Forward;
		};

		// A different file, scroll, re-peek or window size invalidates the selection
		let stale = {
			let Some(lock) = cx.active().preview.lock.as_ref() else {
				return Intercept::Forward;
			};

			cx.active().preview.selection.as_ref().is_some_and(|s| !s.matches(lock))
		};
		if stale {
			cx.active_mut().preview.selection = None;
		}

		let inside = rect.contains(Position { x: event.column(), y: event.row() });

		match event.kind {
			MouseEventKind::Down(MouseButton::Left) if inside => {
				let pos = Self::char_at(cx, rect, event);
				let Some(lock) = cx.active().preview.lock.as_ref() else {
					return Intercept::Forward;
				};

				let selection = PreviewSelection {
					anchor: pos,
					cursor: pos,
					url:    lock.url.clone(),
					sig:    lock.sig,
					skip:   lock.skip,
					area:   *lock.area,
				};

				cx.active_mut().preview.selection = Some(selection);
				Intercept::Handled(true)
			}
			MouseEventKind::Down(MouseButton::Left) => {
				if cx.active_mut().preview.selection.take().is_some() {
					render!();
				}

				Intercept::Forward
			}
			MouseEventKind::Drag(MouseButton::Left) => {
				if cx.active().preview.selection.is_none() {
					// Not ours: e.g. dragging a file from the tree over the preview
					return Intercept::Forward;
				} else if !inside {
					// Keep the last position that was inside the pane
					return Intercept::Handled(false);
				}

				let pos = Self::char_at(cx, rect, event);
				if let Some(selection) = cx.active_mut().preview.selection.as_mut() {
					selection.cursor = pos;
				}

				Intercept::Handled(true)
			}
			MouseEventKind::Up(MouseButton::Left) => {
				let copied = {
					let Some(selection) = cx.active().preview.selection.as_ref() else {
						return Intercept::Forward;
					};

					match cx.active().preview.lock.as_ref() {
						Some(lock) if selection.anchor != selection.cursor => {
							let [Renderable::Text(text)] = &lock.data[..] else {
								return Intercept::Forward;
							};

							Some(selection.text(text.inner()))
						}
						_ => None,
					}
				};

				match copied {
					Some(text) if !text.is_empty() => {
						block_on(CLIPBOARD.set(text.as_bytes()));
						NotifyProxy::push(MessageOpt {
							title:   "Copy".to_owned(),
							content: format!("Copied {} characters", text.chars().count()),
							level:   MessageLevel::Info,
							timeout: Duration::from_secs(2),
						});

						Intercept::Handled(false)
					}
					Some(_) => Intercept::Handled(false),
					None => {
						cx.active_mut().preview.selection = None;
						render!();
						Intercept::Replay
					}
				}
			}
			_ => Intercept::Forward,
		}
	}
}
