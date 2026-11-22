-- Store one answer: right state, right question, inside the time window,
-- first answer wins (FR-GM-06, SEC-16). The ack is sent only after this
-- returns OK, so an acknowledged answer is never lost (NFR-06).
-- KEYS[1] game:<pin>  KEYS[2] game:<pin>:ans:<n>
-- ARGV[1] n  ARGV[2] player id  ARGV[3] submission JSON
-- Returns {'OK', responseMs, answeredCount} or {refusal code}.
local g = redis.call('HMGET', KEYS[1], 'state', 'index', 'startsAt', 'endsAt')
if g[1] ~= 'question' or g[2] ~= ARGV[1] then return {'WRONG_STATE'} end
local now = now_ms()
local starts = tonumber(g[3])
if now < starts then return {'EARLY'} end
if now >= tonumber(g[4]) then return {'LATE'} end
local rt = now - starts
-- Splice the submission in as is: a cjson round trip would turn an empty
-- JSON array into an object. ARGV[3] is produced by serde_json, so it is valid.
local stored = '{"r":' .. rt .. ',"s":' .. ARGV[3] .. '}'
if redis.call('HSETNX', KEYS[2], ARGV[2], stored) == 0 then
  return {'DUPLICATE'}
end
redis.call('EXPIRE', KEYS[2], redis.call('TTL', KEYS[1]))
local answered = redis.call('HINCRBY', KEYS[1], 'answered', 1)
return {'OK', rt, answered}
