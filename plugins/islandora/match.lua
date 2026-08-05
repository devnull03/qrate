-- What counts as a match between a cell and a vocabulary.
--
-- Split from `api.lua` so each half reads on its own: nothing here touches the network, and nothing
-- there decides what is wrong with a value.

local match = {}

local function trim(value)
  return (string.match(value, "^%s*(.-)%s*$"))
end

match.trim = trim

-- One cell as the terms it holds. A blank sub-delimiter leaves the cell whole, which is what a
-- column of single-term values wants. The delimiter is escaped because a `.` typed into that
-- setting is a character, not a Lua pattern.
function match.parts(value, subdelimiter)
  if not subdelimiter or subdelimiter == "" then
    return { trim(value) }
  end
  local escaped = string.gsub(subdelimiter, "(%W)", "%%%1")
  local found = {}
  for part in string.gmatch(value .. subdelimiter, "(.-)" .. escaped) do
    found[#found + 1] = trim(part)
  end
  return found
end

-- Every distinct non-blank term across a column, which is what gets checked. A sheet of five
-- thousand rows is usually a few hundred distinct subjects, and only the distinct ones cost a
-- request.
function match.distinct(values, subdelimiter)
  local seen, found = {}, {}
  for _, value in ipairs(values) do
    for _, part in ipairs(match.parts(value, subdelimiter)) do
      if part ~= "" and not seen[part] then
        seen[part] = true
        found[#found + 1] = part
      end
    end
  end
  return found
end

-- Where each term sits, given what every mapped vocabulary recognised. A value is fine if any one
-- of them holds it, since a column mapped to several vocabularies is asking whether the value is in
-- *the union*, not in all of them.
--
-- `good` is `vocabulary -> set of recognised values`.
function match.report(column_values, mapped, good, subdelimiter)
  local found = {}
  for row, value in ipairs(column_values) do
    for _, part in ipairs(match.parts(value, subdelimiter)) do
      -- A blank cell is missing data, not a vocabulary breach — that is a different check.
      if part ~= "" then
        local held = false
        for _, vocabulary in ipairs(mapped) do
          if (good[vocabulary] or {})[part] then
            held = true
            break
          end
        end
        if not held then
          found[#found + 1] = {
            row = row,
            severity = "error",
            message = '"' .. part .. '" is not a term in ' .. match.list(mapped),
          }
        end
      end
    end
  end
  return found
end

-- "the Islandora subject vocabulary" / "any of the Islandora subject, genre vocabularies", so the
-- message says which lists were actually consulted.
function match.list(mapped)
  if #mapped == 1 then
    return "the Islandora " .. mapped[1] .. " vocabulary"
  end
  return "any of the Islandora " .. table.concat(mapped, ", ") .. " vocabularies"
end

return match
