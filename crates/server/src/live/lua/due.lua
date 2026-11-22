-- PINs whose current question is past its deadline. Every worker runs this
-- every 250 ms; reveal.lua makes sure only one of them acts on each PIN.
-- KEYS[1] games:deadlines  ARGV[1] max PINs to return
return redis.call('ZRANGEBYSCORE', KEYS[1], '-inf', now_ms(), 'LIMIT', 0, tonumber(ARGV[1]))
