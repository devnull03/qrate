-- Proves the plugin path end to end: a file on disk -> a VM -> the Problems panel -> a squiggle.
-- Also the single-file form, next to `vocab/` in the folder form. The file name is the plugin's
-- identity, so there is no `name` field here to keep in sync — declare one only to be called
-- something the file cannot be, and remember settings are stored under whichever name wins.
--
-- `settings` declares knobs for the app's Settings window. Each one is a field inside this
-- plugin's own object in its scope, so `validate` reads it back as `settings.user.<key>`.

return {
  settings = {
    {
      key = "word",
      label = "Word to flag",
      type = "text",
      scope = "user",
      description = 'Flagged wherever it appears in a cell. Empty means "canada".',
    },
    {
      key = "lenient",
      label = "Warn instead of error",
      type = "switch",
      scope = "project",
    },
  },

  validate = function(column, values, settings)
    local word = settings.user.word
    if word == nil or word == "" then
      word = "canada"
    end
    local found = {}
    for row, value in ipairs(values) do
      if string.find(string.lower(value), string.lower(word), 1, true) then
        found[#found + 1] = {
          row = row,
          severity = settings.project.lenient and "warning" or "error",
          message = 'found "' .. word .. '" in ' .. column.name,
        }
      end
    end
    return found
  end,
}
