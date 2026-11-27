-- Mute or unmute a player's chat (texts and emoji).
-- KEYS[1] game:<pin>:players
-- ARGV[1] player id  ARGV[2] '1' to mute, '0' to unmute
-- Returns 1 if set, 0 if no such player.
local raw = redis.call('HGET', KEYS[1], ARGV[1])
if not raw then return 0 end
local p = cjson.decode(raw)
p['muted'] = ARGV[2] == '1'
redis.call('HSET', KEYS[1], ARGV[1], cjson.encode(p))
return 1
