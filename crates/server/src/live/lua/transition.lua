-- Compare-and-set the game state (spec 3.3). The Rust engine decides the
-- next state; this script applies it only if the game is still where the
-- caller saw it, so double clicks and racing workers cannot skip anything.
-- KEYS[1] game:<pin>  KEYS[2] games:deadlines
-- ARGV[1] pin  ARGV[2] from state  ARGV[3] from index ('' if none)
-- ARGV[4] to state  ARGV[5] to index ('' if none)
-- ARGV[6] countdown ms before options show  ARGV[7] time limit ms
-- Returns {1, startsAt, endsAt} on success, {0, state, index} if stale.
local cur = redis.call('HMGET', KEYS[1], 'state', 'index')
if not cur[1] then return {0, '', ''} end
if cur[1] ~= ARGV[2] or cur[2] ~= ARGV[3] then
  return {0, cur[1], cur[2]}
end
redis.call('HSET', KEYS[1], 'state', ARGV[4], 'index', ARGV[5])
if ARGV[4] == 'question' then
  local starts = now_ms() + tonumber(ARGV[6])
  local ends = starts + tonumber(ARGV[7])
  redis.call('HSET', KEYS[1], 'startsAt', starts, 'endsAt', ends, 'answered', 0)
  redis.call('ZADD', KEYS[2], ends, ARGV[1])
  return {1, starts, ends}
end
redis.call('ZREM', KEYS[2], ARGV[1])
return {1, 0, 0}
