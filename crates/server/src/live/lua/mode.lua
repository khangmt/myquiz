-- Switch a game between normal and exam mode, only while in the lobby.
-- KEYS[1] game:<pin>  ARGV[1] 'exam' or 'normal'
-- Returns 1, or 0 if the game has started (or is gone).
if redis.call('HGET', KEYS[1], 'state') ~= 'lobby' then return 0 end
redis.call('HSET', KEYS[1], 'mode', ARGV[1])
return 1
