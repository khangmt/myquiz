-- End question n and hand back its answers (FR-GM-07). Runs from the
-- deadline sweep, the all-answered trigger or host:skip; only the first
-- caller wins, the rest get 0.
-- KEYS[1] game:<pin>  KEYS[2] games:deadlines  KEYS[3] game:<pin>:ans:<n>
-- ARGV[1] pin  ARGV[2] n
-- Returns {1, playerId, answerJSON, ...} or {0}.
local cur = redis.call('HMGET', KEYS[1], 'state', 'index')
if cur[1] ~= 'question' or cur[2] ~= ARGV[2] then return {0} end
redis.call('HSET', KEYS[1], 'state', 'reveal')
redis.call('ZREM', KEYS[2], ARGV[1])
local out = {1}
local answers = redis.call('HGETALL', KEYS[3])
for i = 1, #answers do out[#out + 1] = answers[i] end
return out
