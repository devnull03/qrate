-- Controlled vocabulary: flags any cell whose value is not on its column's allowed list.
--
-- The Rust validator this replaces needed a field on `ColumnSettings` and two hardcoded entries in
-- the column-header menu. Both now live here, which is the point: the app knows the label and the
-- command string, and nothing about vocabularies.

return {
  menu = {
    { label = "Restrict to current values", target = "column", command = "restrict" },
    { label = "Clear restriction", target = "column", command = "clear", requires_settings = true },
  },

  on_command = function(command, ctx)
    if command == "clear" then
      return { column = {} }
    end

    -- Every distinct non-blank value already in the column, sorted so the stored list is
    -- diff-friendly. Restricting a column to what it already holds turns a categorical column into
    -- a typo-catcher in one click.
    local seen, allowed = {}, {}
    for _, value in ipairs(ctx.values) do
      local trimmed = string.match(value, "^%s*(.-)%s*$")
      if trimmed ~= "" and not seen[trimmed] then
        seen[trimmed] = true
        allowed[#allowed + 1] = trimmed
      end
    end
    table.sort(allowed)
    return { column = { allowed_values = allowed } }
  end,

  validate = function(column, values, settings)
    local allowed = settings.column.allowed_values
    -- An empty list is how a column stays unrestricted, so this is the opt-out for every column
    -- the user has not deliberately restricted.
    if not allowed or #allowed == 0 then
      return {}
    end

    local permitted = {}
    for _, value in ipairs(allowed) do
      permitted[value] = true
    end

    local found = {}
    for row, value in ipairs(values) do
      local trimmed = string.match(value, "^%s*(.-)%s*$")
      -- A blank cell is missing data, not a vocabulary breach — that is a different check.
      if trimmed ~= "" and not permitted[trimmed] then
        found[#found + 1] = {
          row = row,
          severity = "error",
          message = '"' .. value .. '" is not an allowed value for ' .. column.name,
        }
      end
    end
    return found
  end,
}
