-- Remove a player; their device cannot rejoin this game (FR-GM-03).
-- KEYS[1] game:<pin>:players  KEYS[2] game:<pin>:tokens
-- KEYS[3] game:<pin>:nicks    KEYS[4] game:<pin>:kicked
-- ARGV[1] player id
-- Returns 1 if removed, 0 if no such player.
local raw = redis.call('HGET', KEYS[1], ARGV[1])
if not raw then return 0 end
local p = cjson.decode(raw)
if p['deviceId'] and p['deviceId'] ~= '' then
  redis.call('SADD', KEYS[4], p['deviceId'])
  redis.call('EXPIRE', KEYS[4], redis.call('TTL', KEYS[1]))
end
redis.call('HDEL', KEYS[3], p['nickKey'])
local tokens = redis.call('HGETALL', KEYS[2])
for i = 1, #tokens, 2 do
  if tokens[i + 1] == ARGV[1] then redis.call('HDEL', KEYS[2], tokens[i]) end
end
redis.call('HDEL', KEYS[1], ARGV[1])
return 1
