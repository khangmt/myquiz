-- End the game, release the PIN (SEC-17) and let every key expire in 24 h.
-- KEYS[1] game:<pin>  KEYS[2] games:deadlines  KEYS[3] pin:<pin>
-- KEYS[4..] every other key of this game
-- ARGV[1] pin  ARGV[2] session id  ARGV[3] ttl seconds after the end
-- Returns 1, or 0 if the game had already ended.
local state = redis.call('HGET', KEYS[1], 'state')
if not state or state == 'ended' then return 0 end
redis.call('HSET', KEYS[1], 'state', 'ended', 'index', '', 'endedAt', now_ms())
redis.call('ZREM', KEYS[2], ARGV[1])
if redis.call('GET', KEYS[3]) == ARGV[2] then redis.call('DEL', KEYS[3]) end
local ttl = tonumber(ARGV[3])
redis.call('EXPIRE', KEYS[1], ttl)
for i = 4, #KEYS do redis.call('EXPIRE', KEYS[i], ttl) end
return 1
