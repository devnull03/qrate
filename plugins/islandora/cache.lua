-- Verdicts already fetched, kept in `qrate.storage` so they survive a restart.
--
-- Without this, holding a column to a vocabulary would cost a request per value on every edit. With
-- it, only a value nobody has checked before costs anything, so re-validating a sheet after one
-- typo is free.
--
-- Keyed by server as well as vocabulary, because two projects on two sites can hold the same
-- column name to the same-named vocabulary and mean different terms.

local storage = qrate.storage

local cache = {}

local function bucket(server, vocabulary)
  return "terms:" .. server .. "|" .. vocabulary
end

-- Split `values` into what is already known good, and what still has to be asked about.
function cache.split(server, vocabulary, values)
  local held = storage.get(bucket(server, vocabulary)) or {}
  local good, unknown = {}, {}
  for _, value in ipairs(values) do
    local verdict = held[value]
    if verdict == nil then
      unknown[#unknown + 1] = value
    elseif verdict then
      good[value] = true
    end
  end
  return good, unknown, held
end

-- Record what the server said about a batch. `known` is the set it recognised; everything else in
-- `asked` it did not, and a miss is worth remembering too or every bad value would be re-fetched on
-- every edit.
function cache.record(server, vocabulary, asked, known, held)
  for _, value in ipairs(asked) do
    held[value] = known[value] == true
  end
  storage.set(bucket(server, vocabulary), held)
end

-- Forget everything about one server, so a term added on the site can be picked up without waiting
-- for the values that reference it to change.
function cache.forget(server, vocabularies)
  for _, vocabulary in ipairs(vocabularies) do
    storage.set(bucket(server, vocabulary), {})
  end
end

return cache
