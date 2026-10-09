Git = {}

-- Badge letters, their dominance when aggregating a folder, and the theme keys
local RANK = { ["!"] = 6, D = 5, M = 4, R = 3, C = 2, A = 1, U = 0 }
local KEY =
	{ ["!"] = "conflict", U = "untracked", A = "added", M = "modified", D = "deleted", R = "renamed", C = "copied" }
local FALLBACK = {
	["!"] = "lightred",
	U = "lightgreen",
	A = "lightgreen",
	M = "lightyellow",
	D = "lightred",
	R = "lightblue",
	C = "lightblue",
}

local TTL, FAIL_TTL = 10, 30

local repos = {} -- [path] = repo root path, or false when the path is not in a repo
local cache = {} -- [repo root] = { files = { [path] = letter }, dirs = { [path] = letter }, pending, at, ok }
local wanted = {} -- [key] = directory awaiting repo detection and/or a fetch

local dirty = false -- a file operation happened since the last refresh

-- --- Parsing -----------------------------------------------------------------

---@param x string
---@param y string
---@return string
local function letter(x, y)
	if x == "?" then
		return "U"
	elseif x == "U" or y == "U" or (x == "A" and y == "A") or (x == "D" and y == "D") then
		return "!"
	elseif x == "R" or y == "R" then
		return "R"
	elseif x == "C" or y == "C" then
		return "C"
	elseif x == "A" then
		return "A"
	elseif x == "D" or y == "D" then
		return "D"
	end

	return "M"
end

---@param counts table
---@return string
local function dominant(counts)
	local best, best_count, best_rank = "M", 0, -1
	for code, count in pairs(counts) do
		local rank = RANK[code] or 0
		if count > best_count or (count == best_count and rank > best_rank) then
			best, best_count, best_rank = code, count, rank
		end
	end

	return best
end

-- Every directory above a changed path is impacted by it too
---@param dirs table
---@param path string
---@param code string
local function ancestors(dirs, path, code)
	local i = 0
	while true do
		i = path:find("/", i + 1, true)
		if not i then
			return
		end

		local dir = path:sub(1, i - 1)
		local counts = dirs[dir] or {}
		counts[code] = (counts[code] or 0) + 1
		dirs[dir] = counts
	end
end

-- Directory URLs may carry a trailing slash; keys and lookups must not
---@param url UrlBuf
---@return string
local function plain(url)
	local path = tostring(url.path)
	return #path > 1 and path:sub(-1) == "/" and path:sub(1, -2) or path
end

local NUL = string.char(0)

---@param st table
---@param out string
local function parse(st, out)
	local files, dirs = {}, {}

	local fields, i = {}, 0
	for field in out:gmatch("[^" .. NUL .. "]+") do
		i = i + 1
		fields[i] = field
	end

	i = 0
	while true do
		i = i + 1
		local field = fields[i]
		if not field then
			break
		end

		local x, y, path = field:sub(1, 1), field:sub(2, 2), field:sub(4)
		if path ~= "" then
			local code = letter(x, y)
			files[path] = code
			ancestors(dirs, path, code)

			if x == "R" or x == "C" then
				i = i + 1 -- the source path follows
			end
		end
	end

	local total = {}
	for _, code in pairs(files) do
		total[code] = (total[code] or 0) + 1
	end
	for dir, counts in pairs(dirs) do
		dirs[dir] = dominant(counts)
	end
	if next(total) then
		dirs[""] = dominant(total) -- the repo root itself
	end

	st.files, st.dirs, st.at, st.ok = files, dirs, os.time(), true
end

-- --- Fetching ----------------------------------------------------------------

-- Detect the repo by walking up, so the nearest one wins (nested repos, submodules),
-- then run a single `git status` for the whole repo
---@param dir UrlBuf
---@param key string
local function update(dir, key)
	ya.async(function()
		local root = repos[plain(dir)]
		if root == nil then
			local up = dir
			while up do
				if fs.stat(up:join(".git")) then
					root = plain(up)
					break
				end
				up = up.parent
			end

			repos[key] = root or false
		end

		local st = root and cache[root]
		if not root or (st and st.pending) then
			ui.render()
			ya.emit("plugin", { "tree", "refresh" })
			return
		end

		st = st or { files = {}, dirs = {}, pending = false, at = 0, ok = false }
		cache[root], st.pending = st, true

		local child = Command("git")
			:arg({ "-C", root, "status", "--porcelain=v1", "-z", "--untracked-files=all" })
			:stdout(Command.PIPED)
			:stderr(Command.PIPED)
			:spawn()

		local out = child and child:wait_with_output()
		if out and out.status.success and #out.stdout < 8 * 1024 * 1024 then
			parse(st, out.stdout)
		else
			st.files, st.dirs, st.at, st.ok = {}, {}, os.time(), false
		end

		st.pending = false
		ui.render()
		ya.emit("plugin", { "tree", "refresh" }) -- wake the event loop so the new status is drawn
	end)
end

local subscribed = false

-- A file operation may have changed the working tree; subscribing needs a runtime frame,
-- so it happens on the first render instead of while the component loads
local function subscribe()
	if subscribed then
		return
	end
	subscribed = true

	ps.sub("patch", function()
		dirty = true
		ui.render()
	end)
end

-- --- API ---------------------------------------------------------------------

---@param dir UrlBuf
---@return string?
local function root_of(dir) return repos[plain(dir)] or nil end

---@param url UrlBuf
---@param is_dir boolean
---@return string?
function Git.letter(url, is_dir)
	local dir = is_dir and url or url.parent
	local root = dir and root_of(dir)
	local st = root and cache[root]
	if not st then
		return nil
	end

	local rel = plain(url):sub(#root + 2)
	return is_dir and st.dirs[rel] or st.files[rel]
end

-- Whether the repo containing this directory has anything to show
---@param dir UrlBuf
---@return boolean
function Git.changed(dir)
	local root = root_of(dir)
	local st = root and cache[root]
	return st ~= nil and next(st.files) ~= nil
end

---@param code string
---@return ui.Style
function Git.style(code)
	local section = th.git
	local style = section and section[KEY[code] or "modified"]
	return style or ui.Style():fg(FALLBACK[code] or "yellow")
end

-- Whether a repo needs a fetch, marking it as wanted so it is asked for once
---@param dir UrlBuf
---@return boolean
function Git.want(dir)
	subscribe()

	local path = plain(dir)
	local root = repos[path]
	if root == nil then
		wanted[path] = dir
		return true
	elseif root == false then
		return false
	end

	local st = cache[root]
	if not st then
		cache[root] = { files = {}, dirs = {}, pending = false, at = 0, ok = false }
		wanted[root] = dir
		return true
	elseif st.pending then
		return false
	elseif dirty or os.time() - st.at > (st.ok and TTL or FAIL_TTL) then
		dirty, st.ok = false, false
		wanted[root] = dir
		return true
	end

	return false
end

-- Fetch everything `Git.want` asked for, off the UI thread
function Git.refresh()
	dirty = false

	for key, dir in pairs(wanted) do
		wanted[key] = nil
		update(dir, key)
	end
end
