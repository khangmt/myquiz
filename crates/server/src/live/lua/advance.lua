-- Exam mode: after question n is revealed and scored, schedule the move
-- to the next question. The deadline sweep picks the PIN up from
-- games:deadlines like a question deadline and finds the game in 'reveal'.
-- Writes nothing if the game has already left reveal n (the host pressed
-- Next, or ended it), so it can never shorten the next question's deadline.
-- KEYS[1] game:<pin>  KEYS[2] games:deadlines
-- ARGV[1] pin  ARGV[2] n  ARGV[3] pause ms
-- Returns the time it advances at (ms), or 0.
local cur = redis.call('HMGET', KEYS[1], 'state', 'index', 'mode')
if cur[1] ~= 'reveal' or cur[2] ~= ARGV[2] or cur[3] ~= 'exam' then return 0 end
local at = now_ms() + tonumber(ARGV[3])
redis.call('HSET', KEYS[1], 'advanceAt', at)
redis.call('ZADD', KEYS[2], at, ARGV[1])
return at
