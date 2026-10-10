Markers = {
	_id = "markers",
}

function Markers:new(chunks, tab)
	local me = setmetatable({ _chunks = chunks, _tab = tab }, { __index = self })
	me:build()
	return me
end

-- The current pane is a tree, which numbers its rows itself for the files they own, and
-- so draws the marks of the current folder; the parent pane is the only one left here
function Markers:build() self._children = { Marker:new(self._chunks[1], self._tab.parent) } end

function Markers:reflow() return {} end

function Markers:redraw()
	local elements = {}
	for _, child in ipairs(self._children) do
		elements = ya.list_merge(elements, ui.redraw(child))
	end
	return elements
end

-- Mouse events
function Markers:click(event, up) end

function Markers:scroll(event, step) end

function Markers:touch(event, step) end
