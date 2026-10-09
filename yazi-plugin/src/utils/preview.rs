use mlua::{ExternalError, Function, IntoLuaMulti, Lua, Table, Value};
use yazi_binding::{elements::Area, runtime};
use yazi_core::{Highlighter, MgrProxy, tab::PreviewLock};
use yazi_fs::file::FileRef;
use yazi_runner::previewer::PeekError;
use yazi_shim::fs::Error;
use yazi_widgets::Renderable;

use super::Utils;

impl Utils {
	// TODO:
	// return (Text?, PeekError?) instead of (String, usize?) to align with other
	// APIs, and allow users to use the Text for more flexible preview
	pub(super) fn preview_code(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, t: Table| async move {
			let area: Area = t.raw_get("area")?;
			let path = t.raw_get::<FileRef>("file")?.borrow(|f| Ok(f.content_path().into_owned()))?;

			let mut lock = PreviewLock::try_from(t)?;
			let inner = match Highlighter::oneshot(path, lock.skip, area.size()).await {
				Ok(text) => text,
				Err(e @ PeekError::Exceeded(max)) => return (e, max).into_lua_multi(&lua),
				Err(e) => {
					return e.into_lua_multi(&lua);
				}
			};

			lock.data = vec![Renderable::Text(inner.into()).with_area(area)];

			MgrProxy::update_peeked(lock, runtime!(lua)?.scope());
			().into_lua_multi(&lua)
		})
	}

	pub(super) fn preview_widget(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, (t, value): (Table, Value)| async move {
			let mut lock = PreviewLock::try_from(t)?;
			lock.data = match value {
				Value::Nil => vec![],
				Value::Table(tbl) => tbl.sequence_values::<Renderable>().collect::<mlua::Result<_>>()?,
				Value::UserData(ud) => match Renderable::try_from(&ud) {
					Ok(r) => vec![r],
					Err(e) => {
						if let Ok(err) = ud.take::<Error>() {
							vec![
								Renderable::Clear(Default::default()).with_area(lock.area),
								Renderable::from(err).with_area(lock.area),
							]
						} else {
							Err(e)?
						}
					}
				},
				_ => Err("preview widget must be a renderable element or a table of them".into_lua_err())?,
			};

			MgrProxy::update_peeked(lock, runtime!(lua)?.scope());
			Ok(())
		})
	}

	pub(super) fn preview_markdown(lua: &Lua) -> mlua::Result<Function> {
		lua.create_async_function(|lua, t: Table| async move {
			let area: Area = t.raw_get("area")?;
			let path = t.raw_get::<FileRef>("file")?.borrow(|f| Ok(f.content_path().into_owned()))?;

			let mut lock = PreviewLock::try_from(t)?;
			let size = area.size();
			let skip = lock.skip;

			let rendered = tokio::task::spawn_blocking(move || {
				let source = std::fs::read(&path)?;
				Ok::<_, std::io::Error>(yazi_markdown::render(
					&String::from_utf8_lossy(&source),
					size.width,
					skip,
					size.height as usize,
				))
			})
			.await
			.map_err(|e| e.into_lua_err())?
			.map_err(|e| e.into_lua_err())?;

			// Scrolling past the end has to be reported the same way `preview_code` does it
			if skip > 0 && rendered.total < skip + size.height as usize {
				let max = rendered.total.saturating_sub(size.height as usize);
				return (PeekError::Exceeded(max), max).into_lua_multi(&lua);
			}

			lock.data = vec![Renderable::Text(rendered.text.into()).with_area(area)];

			MgrProxy::update_peeked(lock, runtime!(lua)?.scope());
			().into_lua_multi(&lua)
		})
	}

	/// Which Markdown renderer the preview is using: `"readable"` or `"code"`.
	pub(super) fn markdown_mode(lua: &Lua) -> mlua::Result<Function> {
		lua.create_function(|_, ()| Ok(yazi_markdown::mode().as_str()))
	}

	/// Switch renderers and remember the choice; returns the new mode.
	pub(super) fn markdown_toggle(lua: &Lua) -> mlua::Result<Function> {
		lua.create_function(|_, ()| Ok(yazi_markdown::toggle().as_str()))
	}
}
