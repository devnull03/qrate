-- Proves the network path: a setting holds a server address, a menu command reaches it over HTTP,
-- and a failure lands in the Problems panel.
--
-- The request runs in `on_command`, never in `validate` — `validate` runs on every edit, and a
-- blocking fetch there would stall the grid on each keystroke. The click stores the verdict; the
-- next validate reports it.

local http = qrate.http

return {
  description = "Checks the Islandora server is reachable.",

  settings = {
    {
      key = "server",
      label = "Islandora server",
      type = "text",
      scope = "project",
      description = "Base URL, e.g. https://islandora.example.org",
    },
  },

  menu = {
    { label = "Check Islandora connection", target = "column", command = "check" },
    { label = "Stop checking", target = "column", command = "stop", requires_settings = true },
  },

  on_command = function(command, ctx)
    -- A project write replaces this plugin's whole object, so `server` has to be written back or
    -- the check would erase the address it just used.
    local stored = ctx.settings.project.server
    if command == "stop" then
      return { column = {} }
    end

    local server = string.gsub(stored or "", "/+$", "")
    local failure
    if server == "" then
      failure = "no server address set"
    else
      -- Every Islandora site exposes JSON:API here, so a 200 means "an Islandora is answering",
      -- not merely "something is listening".
      local response, err = http.get(server .. "/jsonapi")
      if not response then
        failure = err
      elseif response.status ~= 200 then
        failure = server .. " answered " .. response.status
      end
    end

    return {
      project = { server = stored, failure = failure },
      column = { checked = true },
    }
  end,

  validate = function(column, values, settings)
    -- Only the column the check was run on reports, so one bad address does not flag every column.
    if not settings.column.checked or not settings.project.failure or #values == 0 then
      return {}
    end
    return { {
      row = 1,
      severity = "warning",
      message = "Islandora unreachable for " .. column.name .. ": " .. settings.project.failure,
    } }
  end,
}
