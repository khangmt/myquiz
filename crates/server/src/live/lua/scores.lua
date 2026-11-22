-- Add question n's points and times to player totals, exactly once.
-- If a worker dies between reveal and this call, another can retry safely.
-- KEYS[1] game:<pin>  KEYS[2] game:<pin>:players
-- ARGV[1] n  then triples: player id, points, time charged ms
-- Returns 1 if applied, 0 if question n was already scored.
if redis.call('HSETNX', KEYS[1], 'scored:' .. ARGV[1], 1) == 0 then return 0 end
for i = 2, #ARGV, 3 do
  local raw = redis.call('HGET', KEYS[2], ARGV[i])
  if raw then
    local p = cjson.decode(raw)
    p['score'] = (p['score'] or 0) + tonumber(ARGV[i + 1])
    p['timeMs'] = (p['timeMs'] or 0) + tonumber(ARGV[i + 2])
    redis.call('HSET', KEYS[2], ARGV[i], cjson.encode(p))
  end
end
return 1
