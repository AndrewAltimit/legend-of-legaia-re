-- test_midi_sink.lua  -- offline validation of the MIDI sink selection + write.
-- Run:  luajit scripts/vrc-diorama/test_midi_sink.lua
-- No emulator / no virmidi: uses a regular temp file as a stand-in device node
-- (the rawmidi sink is just a binary file write).

local HERE = (arg[0]:match("(.*/)") or "./")
package.path = package.path
    .. ";" .. HERE .. "?.lua"
    .. ";" .. HERE .. "generated/?.lua"

local sink = require("midi_sink")
local enc  = require("midi_encoder")

local fails = 0
local function check(cond, msg)
    if cond then print("  ok  " .. msg)
    else print("  FAIL " .. msg); fails = fails + 1 end
end

-- null sink when LEGAIA_MIDI_DEVICE unset.
do
    local saved = os.getenv("LEGAIA_MIDI_DEVICE")
    -- ensure unset for this case (luajit can't unsetenv portably; rely on it
    -- being unset in the test environment).
    check(saved == nil or saved == "", "test env has LEGAIA_MIDI_DEVICE unset")
    local s = sink.from_env()
    check(s:name() == "null", "from_env() -> null when device unset")
    s:send("\xB0\x04\x64")  -- must not error
    s:close()
end

-- rawmidi sink writes the exact bytes to the target path.
do
    local path = os.tmpname()
    local s, err = sink.rawmidi(path)
    check(s ~= nil, "rawmidi(temp) opens: " .. tostring(err))
    check(s:name() == "rawmidi:" .. path, "rawmidi sink name")
    s:send(enc.pack({ ch = 0, cc = 0x04, val = 100 }))
    s:send(enc.pack({ ch = 3, cc = 0x7F, val = 0 }))
    s:close()
    local fh = io.open(path, "rb")
    local data = fh:read("*a"); fh:close()
    os.remove(path)
    check(#data == 6, "two CC messages = 6 bytes")
    check(data:byte(1) == 0xB0 and data:byte(2) == 0x04 and data:byte(3) == 100,
        "first message bytes B0 04 64")
    check(data:byte(4) == 0xB3 and data:byte(5) == 0x7F and data:byte(6) == 0,
        "commit on ch3 = B3 7F 00")
end

-- rawmidi open failure falls back to null (never aborts a run).
do
    local s = sink.from_env(function() end)  -- env unset -> null already covered
    check(s ~= nil, "from_env never returns nil")
    -- direct bad path
    local bad, err = sink.rawmidi("/nonexistent-dir-xyz/midiC9D9")
    check(bad == nil and err ~= nil, "rawmidi(bad path) returns nil + error")
end

print(fails == 0 and "ALL PASS" or (fails .. " FAILURES"))
os.exit(fails == 0 and 0 or 1)
