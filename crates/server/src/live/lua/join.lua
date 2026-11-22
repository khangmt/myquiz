-- Add a player (FR-GM-02, FR-GM-03, FR-GM-04).
-- KEYS[1] game:<pin>  KEYS[2] game:<pin>:players  KEYS[3] game:<pin>:tokens
-- KEYS[4] game:<pin>:nicks  KEYS[5] game:<pin>:kicked
-- ARGV[1] player id  ARGV[2] player record JSON (without joinOrder)
-- ARGV[3] normalised nickname  ARGV[4] token hash  ARGV[5] device id
-- ARGV[6] max players
-- Returns {'OK', joinOrder} or {refusal code}.
local state = redis.call('HGET', KEYS[1], 'state')
if not state then return {'PIN_INVALID'} end
if state == 'podium' or state == 'ended' then return {'PIN_INVALID'} end
if redis.call('HGET', KEYS[1], 'locked') == '1' then return {'LOCKED'} end
if ARGV[5] ~= '' and redis.call('SISMEMBER', KEYS[5], ARGV[5]) == 1 then
  return {'KICKED'}
end
if redis.call('HLEN', KEYS[2]) >= tonumber(ARGV[6]) then return {'FULL'} end
if redis.call('HSETNX', KEYS[4], ARGV[3], ARGV[1]) == 0 then
  return {'NICK_TAKEN'}
end
local join_order = redis.call('HINCRBY', KEYS[1], 'nextJoin', 1) - 1
local record = cjson.decode(ARGV[2])
record['joinOrder'] = join_order
redis.call('HSET', KEYS[2], ARGV[1], cjson.encode(record))
redis.call('HSET', KEYS[3], ARGV[4], ARGV[1])
local ttl = redis.call('TTL', KEYS[1])
for i = 2, 4 do redis.call('EXPIRE', KEYS[i], ttl) end
return {'OK', join_order}
