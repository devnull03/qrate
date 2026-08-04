-- Proves the plugin path end to end: a file on disk -> a VM -> the Problems panel -> a squiggle.
-- The folder name is the plugin's identity, so there is no `name` field to keep in sync.

return {
  validate = function(column, values)
    local found = {}
    for row, value in ipairs(values) do
      if string.find(string.lower(value), "canada", 1, true) then
        found[#found + 1] = {
          row = row,
          severity = "error",
          message = 'found "canada" in ' .. column.name,
        }
      end
    end
    return found
  end,
}
