--- @sync entry

local M = {}

-- Keymap front-end for the tree, which `Current` itself renders and owns the state of
function M:entry(job)
	local action = job.args[1] and tostring(job.args[1])

	if action == "refresh" then
		return Git.refresh()
	elseif action == "next" then
		return Current:move(1)
	elseif action == "prev" then
		return Current:move(-1)
	elseif action == "expand" then
		return Current:expand()
	elseif action == "collapse" then
		return Current:collapse()
	elseif action == "enter" then
		return Current:enter()
	end

	ya.notify {
		title = "Tree",
		content = string.format("Unknown action: %s", tostring(action)),
		timeout = 5,
		level = "warn",
	}
end

return M
