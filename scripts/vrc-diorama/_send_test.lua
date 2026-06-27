-- _send_test.lua  -- send a few known CC messages through the encoder + sink to
-- LEGAIA_MIDI_DEVICE. Used by verify-virmidi.sh to prove the Lua-sink ->
-- snd-virmidi -> ALSA seq path end-to-end (no PCSX, no VRChat). Standalone
-- luajit; sends ch0 cc0x04 (controller 4) value 100, three times.

local HERE = (arg[0]:match("(.*/)") or "./")
package.path = package.path
    .. ";" .. HERE .. "?.lua"
    .. ";" .. HERE .. "generated/?.lua"

local sink = require("midi_sink").from_env(print)
local enc  = require("midi_encoder")

for _ = 1, 3 do
    sink:send(enc.pack({ ch = 0, cc = 0x04, val = 100 }))
end
sink:close()
print("[_send_test] sent 3x CC ch0 cc0x04 value 100")
