-- Shim module for the tenon plugin.
--
-- The Rust core (`tenon.core`) exposes only setup(). Every other entrypoint
-- (open/toggle/close/action) exists solely as setup's return value, so no
-- config-less call path can exist. This proxy is the user-facing `tenon`
-- module: `setup` is a raw key, and everything else resolves through the
-- metatable below, which refuses to hand out functions before setup has
-- succeeded.
local core = require("tenon.core")

local entries

local proxy = {}

function proxy.setup(opts)
	-- tenon.core never raises (a Rust-side raise would abort the process);
	-- an invalid config arrives as a tagged error table, converted to a
	-- notify + nil return so plugin managers never print a stacktrace.
	local res = core.setup(opts)
	if res == nil then
		return nil
	end
	if res.ok then
		entries = res.entries
		return res.entries
	end
	vim.notify("Setup failed for tenon.nvim\n" .. res.error, vim.log.levels.ERROR)
	return nil
end

setmetatable(proxy, {
	__index = function(_, key)
		if not entries then
			vim.notify("[tenon] not configured; call require('tenon').setup({...}) first", vim.log.levels.ERROR)
			return nil
		end
		return entries[key]
	end,
})

return proxy
