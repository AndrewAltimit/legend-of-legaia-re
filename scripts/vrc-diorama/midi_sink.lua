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
-- On Windows there is no snd-virmidi, so the real sink is WINMM: a LuaJIT FFI
-- binding to winmm.dll's midiOutShortMsg, selected by LEGAIA_MIDI_WINPORT (a
-- case-insensitive substring of the MIDI output port name). It runs inside
-- PCSX-Redux's LuaJIT with no extra dependency. See M.winmm below.
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

-- ---- winmm sink: push CC bytes to a Windows MIDI port (midiOutShortMsg) ----
-- The Windows counterpart of the rawmidi sink. Only ever constructed when
-- LEGAIA_MIDI_WINPORT is set, so requiring this module on Linux/macOS never
-- touches FFI or winmm.dll. Port is matched by case-insensitive name substring.
--
-- NOTE (Win11 MIDI Services loopback pairs are a CROSSOVER): bytes written to the
-- "(A)" endpoint are received on "(B)" and vice versa, so the relay and VRChat
-- must sit on OPPOSITE endpoints, e.g. relay -> "LegaiaDiorama (B)" while VRChat
-- launches with --midi="LegaiaDiorama (A)". Pointing both at the same letter
-- means the decoder never updates. (loopMIDI ports are single-name; no crossover.)
local Winmm = {}
Winmm.__index = Winmm

-- Lazily declare the winmm cdefs exactly once, then return ffi + the dll handle.
local _winmm_cdef_done = false
local function winmm_load()
    local ffi = require("ffi")
    if not _winmm_cdef_done then
        ffi.cdef[[
            typedef void* HMIDIOUT;
            typedef unsigned int MMRESULT;
            // Natural alignment matches the Win32 MIDIOUTCAPS layout (52 bytes):
            // the two uint members already fall on 4-byte boundaries, so no
            // #pragma pack is needed (and avoiding it sidesteps cdef quirks).
            typedef struct {
                unsigned short wMid;
                unsigned short wPid;
                unsigned int   vDriverVersion;
                char           szPname[32];
                unsigned short wTechnology;
                unsigned short wVoices;
                unsigned short wNotes;
                unsigned short wChannelMask;
                unsigned int   dwSupport;
            } MIDIOUTCAPSA;
            unsigned int midiOutGetNumDevs(void);
            MMRESULT midiOutGetDevCapsA(uintptr_t uDeviceID, MIDIOUTCAPSA* pmoc, unsigned int cbmoc);
            MMRESULT midiOutOpen(HMIDIOUT* phmo, unsigned int uDeviceID, uintptr_t cb, uintptr_t inst, unsigned int flags);
            MMRESULT midiOutShortMsg(HMIDIOUT hmo, unsigned int dwMsg);
            MMRESULT midiOutReset(HMIDIOUT hmo);
            MMRESULT midiOutClose(HMIDIOUT hmo);
        ]]
        _winmm_cdef_done = true
    end
    return ffi, ffi.load("winmm")
end

-- Find the first MIDI output whose name contains `substr` (case-insensitive).
-- Returns id, name on success, or nil plus the list of names actually seen.
local function winmm_find(ffi, lib, substr)
    substr = substr:lower()
    local caps = ffi.new("MIDIOUTCAPSA[1]")
    local size = ffi.sizeof("MIDIOUTCAPSA")
    local seen = {}
    local n = tonumber(lib.midiOutGetNumDevs())
    for id = 0, n - 1 do
        if lib.midiOutGetDevCapsA(id, caps, size) == 0 then
            local nm = ffi.string(caps[0].szPname)
            seen[#seen + 1] = nm
            if nm:lower():find(substr, 1, true) then return id, nm end
        end
    end
    return nil, nil, seen
end

function Winmm:send(bytes)
    -- bytes is one or more packed 3-byte CC messages (see midi_encoder.pack).
    local lib, h = self.lib, self.h
    local i, n = 1, #bytes
    while i + 2 <= n do
        local b1, b2, b3 = bytes:byte(i, i + 2)
        -- midiOutShortMsg packs the message little-endian: status | d1<<8 | d2<<16.
        lib.midiOutShortMsg(h, b1 + b2 * 256 + b3 * 65536)
        i = i + 3
    end
end

function Winmm:close()
    if self.h ~= nil then
        self.lib.midiOutReset(self.h)
        self.lib.midiOutClose(self.h)
        self.h = nil
    end
end

function Winmm:name() return "winmm:" .. self.port end

-- Open the MIDI output matching `port_substr`. Returns sink, or nil + message.
function M.winmm(port_substr)
    local ok, ffi, lib = pcall(winmm_load)
    if not ok then
        return nil, "winmm/FFI unavailable (need LuaJIT on Windows): " .. tostring(ffi)
    end
    local id, name, seen = winmm_find(ffi, lib, port_substr)
    if not id then
        local list = (seen and #seen > 0) and table.concat(seen, ", ") or "(none)"
        return nil, string.format("no MIDI output matching %q; visible outputs: %s",
            port_substr, list)
    end
    local h = ffi.new("HMIDIOUT[1]")
    local rc = tonumber(lib.midiOutOpen(h, id, 0, 0, 0))
    if rc ~= 0 then
        return nil, string.format("midiOutOpen(%q) failed (MMRESULT %d)", name, rc)
    end
    return setmetatable({ ffi = ffi, lib = lib, h = h[0], port = name }, Winmm)
end

-- ---- null sink: discard (dry run; the relay still writes its text log) ----
local Null = {}
Null.__index = Null
function Null:send() end
function Null:close() end
function Null:name() return "null" end
function M.null() return setmetatable({}, Null) end

-- Select a sink from the environment:
--   LEGAIA_MIDI_WINPORT=<name substr>         -> winmm  (Windows real port)
--   LEGAIA_MIDI_DEVICE=/dev/snd/midiC<x>D<y>  -> rawmidi (Linux real port)
--   (both unset)                              -> null (dry run / log only)
-- WINPORT is checked first so the Windows relay wins when both happen to be set.
-- `log` is an optional logging callback (e.g. PCSX.log). On open failure a real
-- sink falls back to null so a misconfigured device never aborts a run.
function M.from_env(log)
    local function say(s) if log then log(s) end end

    local winport = os.getenv("LEGAIA_MIDI_WINPORT")
    if winport and winport ~= "" then
        local s, err = M.winmm(winport)
        if s then say("[midi_sink] -> " .. s:name()); return s end
        say("[midi_sink] FALLBACK null: " .. err)
        return M.null()
    end

    local dev = os.getenv("LEGAIA_MIDI_DEVICE")
    if dev and dev ~= "" then
        local s, err = M.rawmidi(dev)
        if s then say("[midi_sink] -> " .. s:name()); return s end
        say("[midi_sink] FALLBACK null: " .. err)
        return M.null()
    end
    say("[midi_sink] LEGAIA_MIDI_WINPORT/LEGAIA_MIDI_DEVICE unset -> null sink (text log only)")
    return M.null()
end

return M
