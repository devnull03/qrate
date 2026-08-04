---@meta
-- Declarations for the host functions `plugin-host` installs into every plugin VM. Loaded by
-- lua-language-server through `.luarc.json`, never at runtime — Neovim describes its `vim` global
-- the same way. Keep this in sync with `install_qrate` in crates/plugin-host/src/plugin.rs.

---@class qrate.Response
---@field status number
---@field body string

---@class qrate.http
local http = {}

--- GET `url`. The host owns the timeout, so a hung server cannot stall the plugin indefinitely,
--- and rate limits each plugin to 30 requests a minute. Returns the response, or `nil` and a
--- message — an unreachable server is a value to report, not an error to raise.
---@param url string
---@return qrate.Response? response
---@return string? err
function http.get(url) end

---@class qrate.status
local status = {}

--- Retitle one of this plugin's own declared `bar` items. `text` takes inline markup:
--- `**bold**`, `*italic*`, `~~strike~~`, `__underline__` — note `__` is underline here, not the
--- second spelling of bold CommonMark makes it. Unicode passes through, so an icon is just an icon.
--- Naming an item this plugin never declared does nothing.
---@param id string
---@param text string
function status.set(id, text) end

---@class qrate
---@field http qrate.http
---@field status qrate.status
qrate = {}
