-- midi_sink.lua  -- pluggable MIDI byte sinks for the diorama relay.
--
-- A sink consumes already-packed MIDI bytes (3-byte CC messages from
-- midi_encoder.pack) and delivers them somewhere. The relay picks one from the
-- environment so the same probe drives either a real port or a dry run.
--
-- The default real sink is RAWMIDI: a plain write to an ALSA snd-virmidi device
-- node (/dev/snd/midiC<x>D<y>). snd-virmidi loops those bytes onto an ALSA
-- sequencer port that Wine/Proton exposes to VRChat as a MIDI input, so no FFI
-- or libasound binding is needed -- it is just a file write, which works inside
-- PCSX-Redux's LuaJIT sandbox. Run scripts/vrc-diorama/setup-virmidi.sh once to
-- create the device and learn its path + the --midi= port name.
--
-- Usage:
--   local sink = require("midi_sink").from_env(function(s) PCSX.log(s) end)
--   sink:send(midi_encoder.pack(msg))
--   sink:close()

local M = {}

-- ---- rawmidi sink: write bytes straight to an ALSA rawmidi device node ----
local Raw = {}
Raw.__index = Raw
function Raw:send(bytes)
    if self.fh then self.fh:write(bytes); self.fh:flush() end
end
function Raw:close()
    if self.fh then self.fh:close(); self.fh = nil end
end
function Raw:name() return "rawmidi:" .. self.path end

function M.rawmidi(path)
    -- "wb": write/binary. NOT "ab" -- O_APPEND is invalid on a non-seekable
    -- rawmidi char device (open fails EINVAL); O_TRUNC is a harmless no-op on
    -- a device node.
    local fh, err = io.open(path, "wb")
    if not fh then
        return nil, string.format("cannot open MIDI device %s (%s)", path, tostring(err))
    end
    return setmetatable({ fh = fh, path = path }, Raw)
end

-- ---- null sink: discard (dry run; the relay still writes its text log) ----
local Null = {}
Null.__index = Null
function Null:send() end
function Null:close() end
function Null:name() return "null" end
function M.null() return setmetatable({}, Null) end

-- Select a sink from the environment:
--   LEGAIA_MIDI_DEVICE=/dev/snd/midiC<x>D<y>  -> rawmidi (real port)
--   (unset)                                   -> null (dry run / log only)
-- `log` is an optional logging callback (e.g. PCSX.log). On open failure the
-- rawmidi path falls back to null so a misconfigured device never aborts a run.
function M.from_env(log)
    local function say(s) if log then log(s) end end
    local dev = os.getenv("LEGAIA_MIDI_DEVICE")
    if dev and dev ~= "" then
        local s, err = M.rawmidi(dev)
        if s then say("[midi_sink] -> " .. s:name()); return s end
        say("[midi_sink] FALLBACK null: " .. err)
        return M.null()
    end
    say("[midi_sink] LEGAIA_MIDI_DEVICE unset -> null sink (text log only)")
    return M.null()
end

return M
