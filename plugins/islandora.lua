-- Proves the network path: a setting holds a server address, a status-bar item reaches it over
-- HTTP, and a failure lands in the Problems panel as well as on the bar.
--
-- The request runs in `on_command`, never in `validate` — `validate` runs on every edit, and a
-- blocking fetch there would stall it behind the server on each keystroke. The click stores the
-- verdict; the next validate reports it.

local http = qrate.http
local status = qrate.status

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

  bar = {
    {
      id = "conn",
      bar = "status",
      side = "right",
      text = "Islandora ?",
      tooltip = "Click to check the Islandora connection",
      left = { command = "check" },
      right = { menu = {
        { label = "Check now", command = "check" },
        { label = "Stop checking this column", command = "stop" },
      } },
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
      status.set("conn", "Islandora ?")
      return { column = {} }
    end

    local server = string.gsub(stored or "", "/+$", "")
    local failure
    if server == "" then
      failure = "no server address set"
    else
      -- The bar says what is happening before the blocking call, since the answer can be ten
      -- seconds away and a bar that says nothing reads as a click that did nothing.
      status.set("conn", "⁘ Islandora")
      local response, err = http.get(server)
      if not response then
        failure = err
      elseif response.status ~= 200 then
        failure = server .. " answered " .. response.status
      end
    end

    status.set("conn", failure and "~~Islandora~~ [red]✗[/]" or "**Islandora** [green]✓[/]")

    return {
      project = { server = stored, failure = failure },
      -- A bar click has no column, and the host drops a column write that has nowhere to land.
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
