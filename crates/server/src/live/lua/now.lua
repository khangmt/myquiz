-- Shared prelude, prepended to every script: the Redis server clock in ms.
-- Every worker uses this one clock, so deadlines never depend on which
-- machine handles a message.
local function now_ms()
  local t = redis.call('TIME')
  return tonumber(t[1]) * 1000 + math.floor(tonumber(t[2]) / 1000)
end
