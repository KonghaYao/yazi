Current = {
	_id = "current",
}

-- Tree state per tab, keyed by tab ID: Tab IDs are userdata, so key them by string
local states = {}

---@return table
local function state()
	local key = tostring(cx.tab.id)
	local st = states[key]
	if not st then
		st = { root = cx.tab.current.cwd, expanded = {}, offset = 0, height = 1 }
		states[key] = st
	end

	return st
end

function Current:new(area, tab)
	return setmetatable({
		_area = area,
		_tab = tab,
		_folder = tab.current,
	}, { __index = self })
end

-- --- Helpers -----------------------------------------------------------------

local function inside(url, root)
	local ok, child = pcall(function() return url:strip_prefix(root) end)
	return ok and child ~= nil
end

-- Loaded folders live in the current/parent panes or in the tab history; `cd` moves the
-- folder it lands on out of the history, so check the parent pane before falling back
local function folder(url)
	if cx.tab.current.cwd == url then
		return cx.tab.current
	elseif cx.tab.parent and cx.tab.parent.cwd == url then
		return cx.tab.parent
	end

	return cx.tab:history(url)
end

-- A directory is unfolded when the user expanded it, or when it leads to the cwd
local function unfolded(st, file) return st.expanded[tostring(file.url)] or inside(cx.tab.current.cwd, file.url) end

local function unfold(url, depth, st, out)
	local dir = folder(url)
	if not dir then
		return
	end

	local files = dir.files
	for _, file in ipairs(files) do
		out[#out + 1] = { file = file, depth = depth }

		if file.stat.is_dir and unfolded(st, file) then
			unfold(file.url, depth + 1, st, out)
		end
	end
end

-- Fold or unfold a directory by its URL
local function fold(st, file)
	local key = tostring(file.url)
	if st.expanded[key] then
		st.expanded[key] = nil
	else
		st.expanded[key] = true
	end
end

---@return table
local function rows(st)
	local out = {}
	unfold(st.root, 0, st, out)
	return out
end

-- Re-root the tree whenever the cwd escapes it, e.g. after `cd ~`
local function reattach(st)
	local cwd = cx.tab.current.cwd
	if inside(cwd, st.root) then
		return
	end

	st.root, st.expanded, st.offset = cwd, {}, 0
end

---@return integer
local function cursor(list)
	local hovered = cx.tab.current.hovered
	if not hovered then
		return 0
	end

	for i, row in ipairs(list) do
		if row.file.url == hovered.url then
			return i
		end
	end

	return 0
end

-- The visible rows plus the row under the cursor, which is the core cursor itself
---@return table, integer, table?
local function focused(st)
	local list = rows(st)
	local at = cursor(list)
	return list, at, list[at]
end

-- --- Rows --------------------------------------------------------------------

-- Only the row under the core cursor may look hovered: rows of a folder other than
-- the current one still carry a stale `is_hovered` flag from their last visit
local function hovered(file) return file.in_current and file.is_hovered end

-- `Entity` and `Linemode` are defined by components loaded after `Current`, so chain lazily
---@param base fun(): table
---@return table
local function extend(base)
	return setmetatable({}, { __index = function(_, key) return base()[key] end })
end

local Row = extend(function() return Entity end)
function Row:hovered() return hovered(self._file) end

local RowLine = extend(function() return Linemode end)
function RowLine:hovered() return hovered(self._file) end
function RowLine:in_current() return true end

-- --- Rendering ---------------------------------------------------------------

-- GUI-style scrollbar: a thumb whose size and position reflect the viewport
local function bar(area, total, offset, height)
	local h = math.max(1, math.floor(height * height / total))
	local y = area.y + math.floor((height - h) * offset / (total - height))

	return ui.Bar(ui.Edge.LEFT)
		:area(ui.Rect { x = area.right - 1, y = y, w = 1, h = h })
		:symbol("█")
		:style(th.mgr.border_style)
end

function Current:empty()
	local s
	if self._folder.files.filter then
		s = "No filter results"
	else
		local done, err = self._folder.stage()
		s = not done and "Loading..." or not err and "No items" or string.format("Error: %s", err)
	end

	return {
		ui.Text(s):area(self._area):align(ui.Align.CENTER):wrap(ui.Wrap.YES),
		table.unpack(Dnd:new(self._area):redraw()),
	}
end

function Current:reflow() return { self } end

function Current:redraw()
	local st = state()
	reattach(st)

	st.height = self._area.h

	local list = rows(st)
	if #list == 0 then
		return self:empty()
	end

	-- Reserve the badge column as soon as a repo has anything to show
	local badge = Git.changed(cx.tab.current.cwd) or Git.changed(st.root)
	local needs = Git.want(cx.tab.current.cwd)

	-- Follow the cursor whenever it moves (`j`/`k`, `<C-d>`, `G`, …), while leaving a
	-- wheel-panned viewport alone
	local at = cursor(list)
	if at ~= 0 and at ~= st.cursor then
		if at <= st.offset then
			st.offset = at - 1
		elseif at > st.offset + st.height then
			st.offset = at - st.height
		end
		st.cursor = at
	end

	st.offset = math.max(0, math.min(st.offset, math.max(0, #list - st.height)))

	local left, right, codes = {}, {}, {}
	for i = st.offset + 1, math.min(st.offset + st.height, #list) do
		local row, entity = list[i], Row:new(list[i].file)
		local prefix = string.rep("  ", row.depth)

		-- Git: the row keeps the filetype style, with the status color patched in
		local code = Git.letter(row.file.url, row.file.stat.is_dir)
		if not code and row.file.stat.is_dir and Git.want(row.file.url) then
			needs = true
		end
		local style = entity:style()
		if code then
			style = style:patch(Git.style(code))
		end

		local body = entity:redraw()
		body:style(style)

		local line = ui.Line { prefix, body }
		if hovered(row.file) then
			line:style(style)
		end

		-- Right side: [padding][linemode][space][badge], the badge on the pane's last column
		local meta = RowLine:new(row.file):redraw()
		if code then
			codes[#codes + 1] = code
		end
		if badge or code then
			right[#right + 1] = ui.Line { meta, " ", code and ui.Span(code):style(Git.style(code)) or " " }
		else
			right[#right + 1] = meta
		end

		local max = math.max(0, self._area.w - right[#right]:width())
		left[#left + 1] = line
		line:truncate { max = max, ellipsis = entity:ellipsis(math.max(0, max - ui.width(prefix))) }
	end

	local elements = {
		ui.Text(left):area(self._area),
		ui.Text(right):area(self._area):align(ui.Align.RIGHT),
	}
	if #list > st.height then
		elements[#elements + 1] = bar(self._area, #list, st.offset, st.height)
	end

	-- Fetch off the UI thread, at most once per repo per refresh
	if needs then
		ya.emit("plugin", { "tree", "refresh" })
	end

	return ya.list_merge(elements, Dnd:new(self._area):redraw())
end

-- --- Navigation --------------------------------------------------------------

---@param step integer
function Current:move(step)
	local id, st = cx.tab.id, state()
	reattach(st)

	local list, at = focused(st)
	if at == 0 then
		return ya.emit("arrow", { step })
	end

	at = at + math.floor(step)
	if at < 1 or at > #list then
		return
	end

	ya.emit("reveal", { list[at].file.url, raw = true, tab = id })
	ui.render()
end

function Current:expand()
	local st = state()
	reattach(st)

	local _, _, row = focused(st)
	if not row or not row.file.stat.is_dir then
		return
	end
	if unfolded(st, row.file) then
		return self:move(1)
	end

	st.expanded[tostring(row.file.url)] = true
	ui.render()
end

function Current:collapse()
	local st = state()
	reattach(st)

	local list, at, row = focused(st)
	if not row then
		return
	end

	local key = tostring(row.file.url)
	if row.file.stat.is_dir and st.expanded[key] and not inside(cx.tab.current.cwd, row.file.url) then
		st.expanded[key] = nil
		return ui.render()
	end

	-- Otherwise, step out to the parent row: the closest preceding row one level up
	for i = at - 1, 1, -1 do
		if list[i].depth < row.depth then
			return ya.emit("reveal", { list[i].file.url, raw = true, tab = cx.tab.id })
		end
	end
end

function Current:enter()
	local st = state()
	reattach(st)

	local _, _, row = focused(st)
	if not row then
		return
	end
	if not row.file.stat.is_dir then
		return ya.emit("open", {})
	end

	fold(st, row.file)
	ui.render()
end

-- Mouse events
function Current:click(event, up)
	if up then
		return
	elseif event.is_back then
		return ya.emit("back", {})
	elseif event.is_forward then
		return ya.emit("forward", {})
	elseif not event.is_left and not event.is_right then
		return
	end

	local st = state()
	local row = rows(st)[st.offset + event.y - self._area.y + 1]
	if not row then
		return
	end

	ya.emit("reveal", { row.file.url, raw = true, tab = cx.tab.id })

	if event.is_right then
		ya.emit("open", {})
	elseif row.file.stat.is_dir then
		-- Fold/unfold the clicked directory, the same as `<Enter>` on it
		fold(st, row.file)
		ui.render()
	end
end

-- Scrolling pans the viewport only: the cursor row and the cwd stay put
function Current:scroll(event, step)
	local st = state()
	local list = rows(st)

	st.offset = math.max(0, math.min(st.offset + step, math.max(0, #list - st.height)))
	ui.render()
end

function Current:touch(event, step) end
