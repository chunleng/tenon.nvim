-- Headless smoke test for the tenon plugin's Lua-facing module contract.
-- scripts/smoke-test.sh launches one nvim process per phase (SMOKE_PHASE):
--   baseline     - module loads; setup with a valid config completes
--   gate-closed  - entrypoint access before setup notifies "not configured"
--                  and yields nil; bypassing the shim via tenon.core exposes
--                  no entrypoints
--   setup-error  - setup with an invalid config notifies ('Setup failed for
--                  tenon.nvim' + validation message) and returns nil; the
--                  gate stays closed afterwards, and a valid setup in the
--                  same session still succeeds
--   gate-open    - after a successful setup the entry table is returned and
--                  entrypoints resolve through the proxy
-- Each phase exits via cq with the failure count, or qa when clean. Phases
-- that must observe CONFIG unset (gate-closed, setup-error) only work as
-- fresh processes, since config initialization is not reversible.

local phase = SMOKE_PHASE

local failures = {}

local function check(name, fn)
    local ok, err = pcall(fn)
    if ok then
        print(string.format("PASS  %s", name))
    else
        print(string.format("FAIL  %s: %s", name, err))
        table.insert(failures, name)
    end
end

local function assert_eq(actual, expected, msg)
    if actual ~= expected then
        error(string.format(
            "%s: expected %s, got %s",
            msg or "assert_eq",
            tostring(expected),
            tostring(actual)
        ))
    end
end

-- Minimal valid config: one ollama connector, one model, one default agent.
local function valid_config()
    return {
        connectors = {
            local_ollama = { type = "ollama" },
        },
        models = {
            fast = { connector = "local_ollama", name = "llama3" },
        },
        agents = {
            main = { model = { id = "fast" }, default = true },
        },
    }
end

-- open/toggle/close are functions; action is a table of action functions.
local ENTRY_TYPES = {
    open = "function",
    toggle = "function",
    close = "function",
    action = "table",
}
local ENTRYPOINTS = { "open", "toggle", "close", "action" }

-- Gated attribute access must notify (ERROR) and yield nil, never raise.
local function assert_gate_closed(name)
    local notifications = {}
    local real_notify = vim.notify
    vim.notify = function(msg, level)
        table.insert(notifications, { msg = msg, level = level })
    end
    local ok, val = pcall(function()
        return require("tenon")[name]
    end)
    vim.notify = real_notify
    if not ok then
        error("access raised: " .. tostring(val))
    end
    if val ~= nil then
        error(("access returned %s before setup"):format(tostring(val)))
    end
    if #notifications ~= 1 then
        error(("expected exactly 1 notification, got %d"):format(#notifications))
    end
    local n = notifications[1]
    if type(n.msg) ~= "string" or not n.msg:find("not configured", 1, true) then
        error("notification is not 'not configured': " .. tostring(n.msg))
    end
    if n.level ~= vim.log.levels.ERROR then
        error("notification level is not ERROR: " .. tostring(n.level))
    end
end

if phase == "baseline" then
    check("require('tenon') loads the module and exposes setup", function()
        local tenon = require("tenon")
        assert_eq(type(tenon), "table", "module type")
        assert_eq(type(tenon.setup), "function", "setup type")
    end)
    check("setup() with a valid config completes", function()
        require("tenon").setup(valid_config())
    end)
elseif phase == "gate-closed" then
    for _, name in ipairs(ENTRYPOINTS) do
        check(("gate closed: '%s' access before setup notifies and yields nil"):format(name), function()
            assert_gate_closed(name)
        end)
    end
    check("bypass: require('tenon.core') exposes no entrypoints", function()
        local ok, core = pcall(require, "tenon.core")
        if not ok then
            return -- module absent pre-flip: nothing to bypass
        end
        assert_eq(type(core.setup), "function", "core.setup type")
        for _, name in ipairs(ENTRYPOINTS) do
            assert_eq(type(core[name]), "nil", ("core.%s type"):format(name))
        end
    end)
elseif phase == "setup-error" then
    check("setup() with an invalid config notifies and returns nil", function()
        -- Agent references a model that is not in the models registry; this is
        -- invalid under current validation, independent of empty-list rules.
        local notifications = {}
        local real_notify = vim.notify
        vim.notify = function(msg, level)
            table.insert(notifications, { msg = msg, level = level })
        end
        local ok, ret = pcall(function()
            return require("tenon").setup({
                agents = {
                    main = { model = { id = "nonexistent" }, default = true },
                },
            })
        end)
        vim.notify = real_notify
        if not ok then
            error("setup() raised on invalid config: " .. tostring(ret))
        end
        if ret ~= nil then
            error("setup() did not return nil on invalid config: " .. tostring(ret))
        end
        if #notifications ~= 1 then
            error(("expected exactly 1 notification, got %d"):format(#notifications))
        end
        local n = notifications[1]
        if type(n.msg) ~= "string"
            or not n.msg:find("Setup failed for tenon.nvim", 1, true)
            or not n.msg:find("unknown model: nonexistent", 1, true)
        then
            error("notification missing header or validation fragment: "
                .. tostring(n.msg))
        end
        if n.level ~= vim.log.levels.ERROR then
            error("notification level is not ERROR: " .. tostring(n.level))
        end
    end)
    check("gate stays closed after a failed setup", function()
        assert_gate_closed("open")
    end)
    check("a valid setup after a failed one succeeds", function()
        -- nil here would mean the failed setup marked the plugin as
        -- configured (CONFIG is only set on success).
        local entries = require("tenon").setup(valid_config())
        if entries == nil then
            error("setup() ignored a valid config after a failed one")
        end
        assert_eq(type(entries.open), "function", "recovered entries.open type")
    end)
elseif phase == "gate-open" then
    check("setup() returns an entry table with all entrypoints", function()
        local entries = require("tenon").setup(valid_config())
        local target = entries
        if target == nil then
            -- pre-flip shape: entrypoints live directly on the module
            target = require("tenon")
        end
        for _, name in ipairs(ENTRYPOINTS) do
            assert_eq(type(target[name]), ENTRY_TYPES[name], ("%s type"):format(name))
        end
    end)
    check("entrypoints resolve through the proxy after setup", function()
        local tenon = require("tenon")
        for _, name in ipairs(ENTRYPOINTS) do
            assert_eq(type(tenon[name]), ENTRY_TYPES[name], ("%s type"):format(name))
        end
    end)
    check("second setup() returns nil and gate stays open", function()
        local again = require("tenon").setup(valid_config())
        assert_eq(again, nil, "second setup() return value")
        local tenon = require("tenon")
        for _, name in ipairs(ENTRYPOINTS) do
            assert_eq(type(tenon[name]), ENTRY_TYPES[name], ("%s type"):format(name))
        end
    end)
else
    error("unknown SMOKE_PHASE: " .. tostring(phase))
end

print(string.format("%s: %d failed", phase, #failures))
for _, name in ipairs(failures) do
    print("  - " .. name)
end
if #failures > 0 then
    vim.cmd(("cq %d"):format(#failures))
else
    vim.cmd("qa")
end
