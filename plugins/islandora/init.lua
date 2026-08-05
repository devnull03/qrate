-- Islandora controlled vocabularies: holds a column to the terms a site actually publishes, and
-- offers those terms while the user types.
--
-- `vocab` restricts a column to the values already in it. This restricts it to lists held somewhere
-- else, checked where they live rather than downloaded — see `api.lua` for why that is one request
-- rather than one per term.
--
-- A column may be mapped to several vocabularies at once; a value is fine if any of them holds it.

local api = require("api")
local cache = require("cache")
local match = require("match")
local status = qrate.status

local ITEM = "islandora"

-- How many unseen values one validate run will look up. The rest roll to the next run, which the
-- next edit triggers — a first pass over a wide sheet finishes over a few seconds instead of
-- spending the plugin's whole request budget at once and being cut off mid-column.
local PER_RUN = 200

local function bar(text)
  status.set(ITEM, text)
end

-- The mapped vocabularies for a column, as a plain list. Written by the mapping tool, which is
-- declared below and rendered by qrate in two places.
local function mapped(settings)
  local list = settings.column.vocabularies
  if type(list) ~= "table" then
    return {}
  end
  return list
end

return {
  api_version = 1,
  description = "Holds a column to controlled vocabularies from an Islandora site.",
  permissions = { "net" },

  settings = {
    {
      key = "server",
      label = "Islandora server",
      type = "text",
      scope = "project",
      description = "Base URL, e.g. https://islandora.example.org",
    },
    {
      key = "subdelimiter",
      label = "Sub-delimiter",
      type = "text",
      scope = "project",
      description = "Splits one cell into several terms. Blank checks the whole cell as one.",
    },
  },

  column_map = {
    key = "vocabularies",
    label = "Vocabularies",
    description = "Columns are checked against every vocabulary you map them to.",
    options = "vocabularies",
    refresh = "refresh",
    multiple = true,
  },

  bar = {
    {
      id = ITEM,
      bar = "status",
      side = "right",
      text = "Islandora ?",
      tooltip = "Click to check the Islandora connection",
      left = { command = "check" },
    },
  },

  menu = {
    { label = "Refresh Islandora vocabularies", target = "column", command = "refresh" },
    {
      label = "Forget cached Islandora terms",
      target = "column",
      command = "forget",
      requires_settings = true,
    },
  },

  on_command = function(command, ctx)
    local server = api.base(ctx.settings.project.server)
    if server == "" then
      bar("Islandora [red]no server set[/]")
      return nil
    end

    -- The bar says what is happening before any blocking call, since the answer can be ten seconds
    -- away and a bar that says nothing reads as a click that did nothing.
    bar("⁘ Islandora")

    if command == "check" then
      local failure = api.reachable(server)
      if failure then
        print("connection failed: " .. failure)
      end
      bar(failure and "~~Islandora~~ [red]✗[/]" or "**Islandora** [green]✓[/]")
      return nil
    end

    if command == "forget" then
      cache.forget(server, mapped(ctx.settings))
      bar("**Islandora** cache cleared")
      return nil
    end

    -- `refresh` repopulates the list the mapping tool offers. Stored in the project scope, which is
    -- also where the server address lives, so both travel with the project.
    local vocabularies, err = api.vocabularies(server)
    if not vocabularies then
      print(err)
      bar("~~Islandora~~ [red]✗[/]")
      return nil
    end

    bar("**Islandora** " .. #vocabularies .. " vocabularies")
    return {
      project = {
        server = ctx.settings.project.server,
        subdelimiter = ctx.settings.project.subdelimiter,
        vocabularies = vocabularies,
      },
    }
  end,

  validate = function(_, values, settings)
    local vocabularies = mapped(settings)
    local server = api.base(settings.project.server)
    -- Every column nobody has mapped, which is most of them.
    if #vocabularies == 0 or server == "" then
      return {}
    end

    local wanted = match.distinct(values, settings.project.subdelimiter)
    local good = {}
    local budget = PER_RUN

    for _, vocabulary in ipairs(vocabularies) do
      local held, unknown, stored = cache.split(server, vocabulary, wanted)
      good[vocabulary] = held

      if #unknown > budget then
        -- Reporting the tail as wrong would flag good values, so this run checks what it can and
        -- says nothing about the rest until a later run has asked.
        for index = budget + 1, #unknown do
          held[unknown[index]] = true
        end
        for index = #unknown, budget + 1, -1 do
          table.remove(unknown, index)
        end
      end
      budget = budget - #unknown

      if #unknown > 0 then
        local known, fetch_err = api.known(server, vocabulary, unknown)
        if not known then
          print(fetch_err)
          -- A server that cannot answer must not turn every value in the column red.
          return {}
        end
        cache.record(server, vocabulary, unknown, known, stored)
        for value in pairs(known) do
          held[value] = true
        end
      end
    end

    return match.report(values, vocabularies, good, settings.project.subdelimiter)
  end,

  suggest = function(ctx)
    local vocabularies = mapped(ctx.settings)
    local server = api.base(ctx.settings.project.server)
    local prefix = match.trim(ctx.prefix or "")
    -- One character is every term in the vocabulary; the user has not said enough yet to be helped.
    if #vocabularies == 0 or server == "" or #prefix < 2 then
      return {}
    end

    local found = {}
    for _, vocabulary in ipairs(vocabularies) do
      local names, err = api.starting_with(server, vocabulary, prefix)
      if names then
        for _, name in ipairs(names) do
          found[#found + 1] = name
        end
      else
        print(err)
      end
    end
    return found
  end,
}
