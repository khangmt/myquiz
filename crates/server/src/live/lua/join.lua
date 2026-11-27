-- Add a player, or let a returning player take their name back
-- (FR-GM-02, FR-GM-03, FR-GM-04, FR-GM-09).
-- KEYS[1] game:<pin>  KEYS[2] game:<pin>:players  KEYS[3] game:<pin>:tokens
-- KEYS[4] game:<pin>:nicks  KEYS[5] game:<pin>:kicked (banned devices and
-- 'user:<id>' accounts)  KEYS[6] game:<pin>:online
-- KEYS[7] game:<pin>:rejoin (player id -> device asking to rejoin)
-- KEYS[8] game:<pin>:rejoin:ok (player id -> device the host let back in)
-- ARGV[1] player id  ARGV[2] player record JSON (without joinOrder)
-- ARGV[3] normalised nickname  ARGV[4] token hash  ARGV[5] device id
-- ARGV[6] max players  ARGV[7] signed-in user id ('' if none)
-- Returns {'OK', joinOrder}, {'RECLAIMED', playerId}, {'REJOIN_WAIT', playerId}
-- or {refusal code}.
local state = redis.call('HGET', KEYS[1], 'state')
if not state then return {'PIN_INVALID'} end
if state == 'podium' or state == 'ended' then return {'PIN_INVALID'} end
if ARGV[5] ~= '' and redis.call('SISMEMBER', KEYS[5], ARGV[5]) == 1 then
  return {'BANNED'}
end
if ARGV[7] ~= '' and redis.call('SISMEMBER', KEYS[5], 'user:' .. ARGV[7]) == 1 then
  return {'BANNED'}
end
local ttl = redis.call('TTL', KEYS[1])

-- The name is in use. A player whose phone lost its session (another
-- browser, cleared storage) gets it back if nobody is playing under it
-- right now and it is the same device, the same signed-in user, or the
-- host let this device in. Locked lobbies and full games do not stop them.
local owner = redis.call('HGET', KEYS[4], ARGV[3])
if owner then
  if tonumber(redis.call('HGET', KEYS[6], owner) or '0') > 0 then
    return {'NICK_TAKEN'}
  end
  local raw = redis.call('HGET', KEYS[2], owner)
  if not raw then return {'NICK_TAKEN'} end
  local p = cjson.decode(raw)
  local same_device = ARGV[5] ~= '' and p['deviceId'] == ARGV[5]
  local same_user = ARGV[7] ~= '' and p['userId'] == ARGV[7]
  local approved = ARGV[5] ~= '' and redis.call('HGET', KEYS[8], owner) == ARGV[5]
  if not (same_device or same_user or approved) then
    redis.call('HSET', KEYS[7], owner, ARGV[5])
    redis.call('EXPIRE', KEYS[7], ttl)
    return {'REJOIN_WAIT', owner}
  end
  local tokens = redis.call('HGETALL', KEYS[3])
  for i = 1, #tokens, 2 do
    if tokens[i + 1] == owner then redis.call('HDEL', KEYS[3], tokens[i]) end
  end
  redis.call('HSET', KEYS[3], ARGV[4], owner)
  if not same_device then
    p['deviceId'] = ARGV[5]
    redis.call('HSET', KEYS[2], owner, cjson.encode(p))
  end
  redis.call('HDEL', KEYS[7], owner)
  redis.call('HDEL', KEYS[8], owner)
  return {'RECLAIMED', owner}
end

if redis.call('HGET', KEYS[1], 'locked') == '1' then return {'LOCKED'} end
if redis.call('HLEN', KEYS[2]) >= tonumber(ARGV[6]) then return {'FULL'} end
if redis.call('HSETNX', KEYS[4], ARGV[3], ARGV[1]) == 0 then
  return {'NICK_TAKEN'}
end
local join_order = redis.call('HINCRBY', KEYS[1], 'nextJoin', 1) - 1
local record = cjson.decode(ARGV[2])
record['joinOrder'] = join_order
redis.call('HSET', KEYS[2], ARGV[1], cjson.encode(record))
redis.call('HSET', KEYS[3], ARGV[4], ARGV[1])
for i = 2, 4 do redis.call('EXPIRE', KEYS[i], ttl) end
return {'OK', join_order}
