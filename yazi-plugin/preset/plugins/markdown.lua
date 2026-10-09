local M = {}

-- Markdown has two renderings, switched by clicking the badge in the top-right corner of the
-- preview: a readable one (the default), and the raw source as `code` shows it. Both live
-- here so that clicking never has to re-route the previewer itself.
function M:peek(job)
	local err, bound
	if ya.markdown_mode() == "readable" then
		err, bound = ya.preview_markdown(job)
	else
		err, bound = ya.preview_code(job)
	end

	if bound then
		ya.emit("peek", { bound, only_if = job.file.url, upper_bound = true })
	elseif err and not err:find("cancelled", 1, true) then
		require("empty").msg(job, err)
	end
end

function M:seek(job)
	local h = cx.tab.current.hovered
	if not h or h.url ~= job.file.url then
		return
	end

	local step = math.floor(job.units * job.area.h / 10)
	step = step == 0 and ya.clamp(-1, job.units, 1) or step

	ya.emit("peek", {
		math.max(0, cx.tab.preview.skip + step),
		only_if = job.file.url,
		tab = job.tab,
	})
end

function M:spot(job) require("file"):spot(job) end

return M
