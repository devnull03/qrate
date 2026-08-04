---@meta
-- Declarations for the host functions `plugin-host` installs into every plugin VM. Loaded by
-- lua-language-server through `.luarc.json`, never at runtime — Neovim describes its `vim` global
-- the same way. Keep this in sync with `install_http` in crates/plugin-host/src/plugin.rs.

---@class qrate.Response
---@field status number
---@field body string

---@class qrate.http
local http = {}

--- GET `url`. The host owns the timeout, so a hung server cannot stall the plugin indefinitely.
--- Returns the response, or `nil` and a message — an unreachable server is a value to report, not
--- an error to raise.
---@param url string
---@return qrate.Response? response
---@return string? err
function http.get(url) end

---@class qrate
---@field http qrate.http
qrate = {}
