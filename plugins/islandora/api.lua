-- Drupal's JSON:API, which is how an Islandora site publishes its taxonomies.
--
-- Every call here answers a value, or `nil` and a message — the shape `qrate.http.get` already
-- uses, so a site that is down, behind a bot wall, or refusing anonymous reads stays something the
-- caller can put on the status bar instead of something that kills the plugin.
--
-- Nothing here downloads a vocabulary. Terms are checked where they live: one request asks the
-- server which of fifty values it recognises, which is what makes holding a column to a vocabulary
-- of a hundred thousand terms cost the same as one of ten.

local http = qrate.http
local json = qrate.json

-- Drupal's own ceiling on `page[limit]`, and the size of one batched check.
local PAGE = 50

-- ponytail: how many suggestions to ask for. A completion list nobody scrolls does not need to be
-- longer; raise it if the popup ever grows a scrollbar worth using.
local SUGGESTIONS = 12

local api = {}

-- A trailing slash would double up in every URL built from this.
function api.base(server)
  return (string.gsub(server or "", "/+$", ""))
end

-- Percent-encoding, because a term name is user text and `Jordan, Mark` or `50% cotton` would
-- otherwise change the query rather than travel through it.
local function encode(text)
  return (string.gsub(text, "[^%w%-%_%.%~]", function(char)
    return string.format("%%%02X", string.byte(char))
  end))
end

local function fetch(url)
  local response, err = http.get(url)
  if not response then
    return nil, err
  end
  if response.status ~= 200 then
    return nil, url .. " answered " .. response.status
  end
  local body, decode_err = json.decode(response.body)
  if not body then
    return nil, url .. " did not answer JSON: " .. decode_err
  end
  return body
end

-- Whether the site answers, and whether JSON:API is switched on at all — a 404 here is the usual
-- cause of everything else failing, and it is worth naming rather than discovering per vocabulary.
function api.reachable(server)
  local _, err = fetch(server .. "/jsonapi")
  return err
end

-- Every vocabulary on the site, as `{ value = machine name, label = human name }`.
--
-- Drupal hides vocabularies from anonymous readers unless the `access taxonomy overview`
-- permission is granted, and it does so by answering 200 with an empty list and an `omitted`
-- note rather than by refusing. Reporting that as "no vocabularies" would send someone looking
-- for the wrong problem, so it is reported as what it is.
function api.vocabularies(server)
  local body, err = fetch(server .. "/jsonapi/taxonomy_vocabulary/taxonomy_vocabulary")
  if not body then
    return nil, err
  end

  local found = {}
  for _, item in ipairs(body.data or {}) do
    local attributes = item.attributes or {}
    if attributes.drupal_internal__vid then
      found[#found + 1] = {
        value = attributes.drupal_internal__vid,
        label = attributes.name or attributes.drupal_internal__vid,
      }
    end
  end

  if #found == 0 and body.meta and body.meta.omitted then
    return nil,
      "the site will not list its vocabularies to an anonymous reader — grant the "
        .. "'access taxonomy overview' permission, or type the machine names in by hand"
  end
  return found
end

local function terms_url(server, vocabulary, query)
  return server
    .. "/jsonapi/taxonomy_term/"
    .. encode(vocabulary)
    .. "?fields%5Btaxonomy_term--"
    .. encode(vocabulary)
    .. "%5D=name&"
    .. query
end

-- Which of `values` the vocabulary holds, as a set of the ones it does.
--
-- One request per batch, whatever the size of the vocabulary: JSON:API answers a filter, and what
-- comes back is exactly the names that exist. Anything asked for and not returned is a miss, which
-- is the whole check.
function api.known(server, vocabulary, values)
  local known = {}
  for start = 1, #values, PAGE do
    local query = "filter%5Bm%5D%5Bcondition%5D%5Bpath%5D=name"
      .. "&filter%5Bm%5D%5Bcondition%5D%5Boperator%5D=IN"
      .. "&page%5Blimit%5D="
      .. PAGE
    for offset = 0, PAGE - 1 do
      local value = values[start + offset]
      if not value then
        break
      end
      -- Indexed rather than empty brackets: Drupal's own documentation warns that repeated bare
      -- `value[]` keys collapse to one value in several HTTP clients.
      query = query
        .. "&filter%5Bm%5D%5Bcondition%5D%5Bvalue%5D%5B"
        .. (offset + 1)
        .. "%5D="
        .. encode(value)
    end

    local body, err = fetch(terms_url(server, vocabulary, query))
    if not body then
      return nil, err
    end
    for _, item in ipairs(body.data or {}) do
      local name = (item.attributes or {}).name
      if name then
        known[name] = true
      end
    end
  end
  return known
end

-- Terms starting with what the user has typed. The server does the matching, so this stays one
-- request no matter how large the vocabulary.
function api.starting_with(server, vocabulary, prefix)
  local query = "filter%5Bs%5D%5Bcondition%5D%5Bpath%5D=name"
    .. "&filter%5Bs%5D%5Bcondition%5D%5Boperator%5D=STARTS_WITH"
    .. "&filter%5Bs%5D%5Bcondition%5D%5Bvalue%5D="
    .. encode(prefix)
    .. "&page%5Blimit%5D="
    .. SUGGESTIONS

  local body, err = fetch(terms_url(server, vocabulary, query))
  if not body then
    return nil, err
  end
  local names = {}
  for _, item in ipairs(body.data or {}) do
    local name = (item.attributes or {}).name
    if name then
      names[#names + 1] = name
    end
  end
  return names
end

return api
