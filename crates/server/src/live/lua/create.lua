-- Claim a PIN and create a game in the lobby (FR-GM-01).
-- KEYS[1] pin:<pin>   KEYS[2] game:<pin>   KEYS[3] game:<pin>:quiz
-- ARGV[1] session id  ARGV[2] host user id  ARGV[3] total questions
-- ARGV[4] quiz snapshot JSON  ARGV[5] ttl seconds
-- Returns 1, or 0 if the PIN is already in use (caller draws another).
local ttl = tonumber(ARGV[5])
if not redis.call('SET', KEYS[1], ARGV[1], 'NX', 'EX', ttl) then
  return 0
end
redis.call('DEL', KEYS[2])
redis.call('HSET', KEYS[2],
  'state', 'lobby', 'index', '', 'session', ARGV[1], 'host', ARGV[2],
  'total', ARGV[3], 'locked', '0', 'nextJoin', '0', 'answered', '0',
  'startsAt', '0', 'endsAt', '0', 'createdAt', tostring(now_ms()))
redis.call('EXPIRE', KEYS[2], ttl)
redis.call('SET', KEYS[3], ARGV[4], 'EX', ttl)
return 1
