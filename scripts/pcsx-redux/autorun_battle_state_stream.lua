-- autorun_battle_state_stream.lua
--
-- The KEYSTONE live-probe: a VSync-poll exporter of the typed battle state.
-- Each frame it reads the pinned battle addresses via probe.battle_state,
-- diffs against the previous frame, and emits a newline-delimited JSON
-- stream of CHANGES (plus a periodic full-state sweep and a sweep on every
-- battle-enter, so a late consumer / dropped event self-recovers).
--
-- This is the shared EVENT SOURCE for both delivery targets in the PRDs:
--   - the VRChat live battle diorama (MIDI register stream)
--   - the wgpu/OpenXR spectator viewport (UDP BattleState packets)
-- Transport is intentionally NOT here -- this probe's transport is a JSONL
-- file + the PCSX log. A MIDI/UDP encoder consumes battle_state.read()
-- directly. See the VRChat live battle-diorama PRD (secs 5.2/11).
--
-- It is ALSO the reusable capture harness for the still-open battle RE
-- threads (F-RAGE delegated-pick variability, F-RENDERMODE enemy summon,
-- F-PAL palette write): point it at a mid-cast save and read the stream.
--
-- No breakpoints: pure per-VSync polling, so it is hot-path-safe and runs
-- under --fast (recompiler) as well as the default interpreter.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_battle_state_stream.lua \
--     --scenario overworld_battle_bg_angle_a
-- Env:
--   LEGAIA_SSTATE        save state to load (a battle save)
--   LEGAIA_STREAM_FRAMES vsyncs to capture (default 1800 ~ 30s)
--   LEGAIA_STREAM_SWEEP  full-sweep cadence in vsyncs (default 120)
--   LEGAIA_OUT[_DIR]     output JSONL path (default battle_state_stream.jsonl)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local bs    = require("probe.battle_state")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local CAP_FRAMES = probe.getenv_num("LEGAIA_STREAM_FRAMES", 1800)
local SWEEP_EVERY = probe.getenv_num("LEGAIA_STREAM_SWEEP", 120)
local OUT_PATH = probe.out_path("battle_state_stream.jsonl")

local out = io.open(OUT_PATH, "w")
if not out then
    PCSX.log("[battle_state_stream] FATAL: cannot open " .. OUT_PATH)
end

local function emit(line)
    if out then out:write(line); out:write("\n"); out:flush() end
end

-- Concise one-line human summary for the live PCSX log.
local function log_summary(st, kind, vsync)
    local enemies = {}
    for slot = bs.PARTY_SLOTS, bs.SLOT_COUNT - 1 do
        local s = st.slots[slot]
        if s.present then
            enemies[#enemies + 1] = string.format("#%d id%d %d/%d",
                slot, s.monster_id, s.hp, s.hp_max)
        end
    end
    PCSX.log(string.format(
        "[bss %s] v%d battle=%s mode=0x%02X act(p/m)=%d/%d turn=%d | %s",
        kind, vsync, tostring(st.in_battle), st.render_mode,
        st.party_action_id, st.monster_action_id, st.turn,
        table.concat(enemies, "  ")))
end

local last_sig = nil
local was_in_battle = false
local emitted = 0

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = CAP_FRAMES + 80,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        -- Let the save settle a few frames before sampling.
        if elapsed < 8 then return end
        local vsync = elapsed - 8
        local st = bs.read()
        local sig = bs.signature(st)

        local enter = st.in_battle and not was_in_battle
        local periodic = (vsync % SWEEP_EVERY == 0)
        local changed = (sig ~= last_sig)

        if enter or periodic then
            emit(bs.to_json(st, "full", vsync))
            log_summary(st, enter and "enter" or "full", vsync)
            emitted = emitted + 1
        elseif changed then
            emit(bs.to_json(st, "delta", vsync))
            log_summary(st, "delta", vsync)
            emitted = emitted + 1
        end

        last_sig = sig
        was_in_battle = st.in_battle
        if vsync >= CAP_FRAMES then
            PCSX.log(string.format(
                "[battle_state_stream] done: %d records -> %s",
                emitted, OUT_PATH))
            if out then out:close(); out = nil end
            c.request_quit = true
        end
    end,
})
