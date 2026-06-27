-- autorun_battle_midi_stream.lua
--
-- The battle-diorama MIDI RELAY (PRD milestone M1->M2 bridge). Per VSync it
-- reads the typed battle state (probe.battle_state), runs the MIDI register
-- encoder (scripts/vrc-diorama/midi_encoder.lua), and pushes the resulting
-- Control-Change messages to a SINK. A full sweep is sent on battle-enter and
-- every LEGAIA_STREAM_SWEEP vsyncs (so a late/dropped consumer self-recovers);
-- otherwise only deltas go out.
--
-- This composes the two halves built so far:
--   extraction (transport-free)  probe.battle_state.read()  ->  BattleState
--   transport (this PRD)         midi_encoder               ->  CC messages
--
-- SINK: by default the messages are written to a text log (vsync ch cc val) so
-- the stream is inspectable offline and a companion process can replay it to a
-- real virtual MIDI port. Sending to an actual port from inside PCSX-Redux
-- (LuaJIT FFI midiOutShortMsg on Windows, or an ALSA seq write) is PRD
-- milestone M0 (platform risk) and slots into `send_midi` below unchanged.
--
-- Usage (interpreter mode -- the recompiler diverges on interpreter saves):
--   bash scripts/pcsx-redux/run_probe.sh \
--     --scenario party_basic_attack_vs_gobu_gobu \
--     --lua scripts/pcsx-redux/autorun_battle_midi_stream.lua
-- Env: LEGAIA_SSTATE, LEGAIA_STREAM_FRAMES (default 1800), LEGAIA_STREAM_SWEEP
--      (default 120), LEGAIA_OUT[_DIR] (default battle_midi_stream.log).

package.path = package.path
    .. ";scripts/pcsx-redux/lib/?.lua"
    .. ";scripts/vrc-diorama/?.lua"
    .. ";scripts/vrc-diorama/generated/?.lua"

local probe   = require("probe")
local bs      = require("probe.battle_state")
local encoder = require("midi_encoder")
local midisink = require("midi_sink")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local CAP_FRAMES  = probe.getenv_num("LEGAIA_STREAM_FRAMES", 1800)
local SWEEP_EVERY = probe.getenv_num("LEGAIA_STREAM_SWEEP", 120)
local OUT_PATH    = probe.out_path("battle_midi_stream.log")

local out = io.open(OUT_PATH, "w")
if not out then PCSX.log("[battle_midi_stream] FATAL: cannot open " .. OUT_PATH) end

-- The transport sink: a real ALSA snd-virmidi device when LEGAIA_MIDI_DEVICE is
-- set (see setup-virmidi.sh), else a null sink (dry run; the text log below
-- still records the full CC stream for inspection / offline replay).
local sink = midisink.from_env(function(s) PCSX.log(s) end)

local total = 0
local function flush_messages(kind, vsync, msgs)
    if #msgs == 0 then return end
    for _, m in ipairs(msgs) do
        sink:send(encoder.pack(m))
        if out then
            out:write(string.format("%d %s %d %d %d\n", vsync, kind, m.ch, m.cc, m.val))
        end
    end
    if out then out:flush() end
    total = total + #msgs
    PCSX.log(string.format("[midi %s] v%d -> %d CC msgs (total %d)",
        kind, vsync, #msgs, total))
end

local e = encoder.new()
local was_in_battle = false

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = CAP_FRAMES + 80,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed < 8 then return end
        local vsync = elapsed - 8
        e:tick()
        local st = bs.read()

        local enter = st.in_battle and not was_in_battle
        if enter or (vsync % SWEEP_EVERY == 0) then
            flush_messages(enter and "full-enter" or "full", vsync, e:full(st))
        else
            flush_messages("delta", vsync, e:delta(st))
        end
        was_in_battle = st.in_battle

        if vsync >= CAP_FRAMES then
            PCSX.log(string.format("[battle_midi_stream] done: %d CC msgs -> %s",
                total, OUT_PATH))
            if out then out:close(); out = nil end
            sink:close()
            c.request_quit = true
        end
    end,
})
