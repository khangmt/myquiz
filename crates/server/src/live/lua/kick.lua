-- Remove a player (FR-GM-03). A plain removal frees their name and lets
-- the same device join again under another name; with the ban flag their
-- device (and their account, if signed in) cannot rejoin this game.
-- KEYS[1] game:<pin>:players  KEYS[2] game:<pin>:tokens
-- KEYS[3] game:<pin>:nicks    KEYS[4] game:<pin>:kicked (banned devices)
-- ARGV[1] player id  ARGV[2] '1' to ban, '0' to only remove
-- Returns 1 if removed, 0 if no such player.
local raw = redis.call('HGET', KEYS[1], ARGV[1])
if not raw then return 0 end
local p = cjson.decode(raw)
if ARGV[2] == '1' then
  local banned = false
  if type(p['deviceId']) == 'string' and p['deviceId'] ~= '' then
    redis.call('SADD', KEYS[4], p['deviceId'])
    banned = true
  end
  if type(p['userId']) == 'string' and p['userId'] ~= '' then
    redis.call('SADD', KEYS[4], 'user:' .. p['userId'])
    banned = true
  end
  if banned then redis.call('EXPIRE', KEYS[4], redis.call('TTL', KEYS[1])) end
end
redis.call('HDEL', KEYS[3], p['nickKey'])
local tokens = redis.call('HGETALL', KEYS[2])
for i = 1, #tokens, 2 do
  if tokens[i + 1] == ARGV[1] then redis.call('HDEL', KEYS[2], tokens[i]) end
end
redis.call('HDEL', KEYS[1], ARGV[1])
return 1
