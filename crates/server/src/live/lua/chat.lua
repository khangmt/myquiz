-- Rate limits for a chat message (players' popups on the host screen).
-- KEYS[1] game:<pin>:chat:<player>:<kind>  per player and kind, a cooldown
-- KEYS[2] game:<pin>:chat:rate               per game, a one-second window
-- ARGV[1] cooldown ms  ARGV[2] most messages per game and second
-- Returns 1 if the message may go out, 0 if the player sent one too
-- recently, -1 if the whole game is over its rate.
if redis.call('EXISTS', KEYS[1]) == 1 then return 0 end
local n = redis.call('INCR', KEYS[2])
if n == 1 then redis.call('PEXPIRE', KEYS[2], 1000) end
if n > tonumber(ARGV[2]) then return -1 end
redis.call('SET', KEYS[1], '1', 'PX', tonumber(ARGV[1]))
return 1
