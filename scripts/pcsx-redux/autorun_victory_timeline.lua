-- autorun_victory_timeline.lua
--
-- Per-vsync poll of the battle END sequence (the results sequencer
-- FUN_8004E568 + the exit gate in FUN_80046A20), from a state that resolves
-- on its own into a victory (or a wipe). No breakpoints: runs under --fast.
--
-- What it pins (docs/subsystems/battle.md § "Battle end, retail's way"):
--   * the frame the action SM raises the battle-end signal
--     (DAT_8007BD71 = 0xFE) and the wipe cause (_DAT_8007BD2C: 0 victory,
--     5 party wipe);
--   * the load phases the cause word walks (0 -> 2 -> 4 -> 5) while the
--     hero voice clip (slot 7) and the PROT 0889 level-up bank (slot 11)
--     stream in, i.e. how many vsyncs the pose-8 hold lasts on real CD
--     timing;
--   * the results frame (ctx+0x6CE 0 -> 1): pose id staged into the pose
--     actor's +0x1DA, DAT_8007BD60 |= 0x80, result windows up;
--   * the results hold (0x8007BD6C counts vsyncs to 0x100), the white-out
--     (ctx+0x6CE -> 2, fade template at 0x801C9070 kind 2 / time 0x40),
--     and the exit (ctx+0x6CE >= 0x43 -> game_mode leaves 0x15).
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_victory_timeline.lua \
--     --scenario rim_elm_gimard_victory --frames 1500
-- Env:
--   LEGAIA_SSTATE   save state to load (a battle state that resolves)
--   LEGAIA_FRAMES   vsyncs to capture (default 1500)
--   LEGAIA_OUT[_DIR] output CSV path (default victory_timeline.csv)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate9")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1500)
local OUT_PATH = probe.out_path("victory_timeline.csv")

-- gp = 0x8007B318 in SCUS_942.54; the gp-relative cells the sequencer uses.
local CTX_PTR     = 0x8007BD24 -- battle context pointer (gp+0xA0C)
local SIGNAL      = 0x8007BD71 -- DAT_8007BD71: 0xFF running, 0xFE battle end
local CAUSE       = 0x8007BD2C -- _DAT_8007BD2C: wipe cause / results phase (gp+0xA14)
local SURVIVED    = 0x8007BD60 -- DAT_8007BD60: bit 0x80 = party survived / voice ready (gp+0xA48)
local POSE_ID     = 0x8007BD64 -- gp+0xA4C: chosen win-pose action id (0x11..0x18)
local HOLD_TIMER  = 0x8007BD6C -- gp+0xA54: results hold, vsyncs, fade at 0x100
local XP_SHOWN    = 0x8007BD1C -- gp+0xA04: per-member XP figure
local GOLD_SHOWN  = 0x8007BD54 -- gp+0xA3C: gold figure
local GAME_MODE   = 0x8007B83C -- _DAT_8007B83C (u16): 0x15 battle, 2 MAIN INIT
local FADE_TMPL   = 0x801C9070 -- DAT_801C9070 fade template: u16 kind, u16 time
local ACTOR_TABLE = 0x801C9370 -- 8 actor pointers
local PARTY_IDS   = 0x8007BD10 -- DAT_8007BD10: seat -> 1-based char id

local csv = probe.csv_open(OUT_PATH,
    "vsync,signal,cause,survived,mode,flow,astate,ctx13,ctx0b,ctx0c,phase6ce," ..
    "pose_id,hold,xp,gold,fade_kind,fade_time,p0_q,p0_c,p0_hp,p1_q,p1_c,p1_hp,p2_q,p2_c,p2_hp,m0_hp")

local function actor_triplet(seat)
    local p = probe.read_u32(ACTOR_TABLE + seat * 4)
    if not probe.in_ram(p, 0x200) then return 0, 0, 0 end
    return probe.read_u8(p + 0x1DA), probe.read_u8(p + 0x1D9), probe.read_u16(p + 0x14C)
end

local last_sig = nil
local rows = 0
local exit_seen = nil

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    on_arm = function() return {} end,
    on_capture = function(c, elapsed)
        if elapsed < 4 then return end
        local vsync = elapsed - 4
        local ctx = probe.read_u32(CTX_PTR)
        local ctx_ok = probe.in_ram(ctx, 0x1100)
        local signal   = probe.read_u8(SIGNAL)
        local cause    = probe.read_u32(CAUSE)
        local survived = probe.read_u8(SURVIVED)
        local mode     = probe.read_u16(GAME_MODE)
        local flow, astate, ctx13, ctx0b, ctx0c, phase = 0, 0, 0, 0, 0, 0
        if ctx_ok then
            flow   = probe.read_u8(ctx + 0x6)
            astate = probe.read_u8(ctx + 0x7)
            ctx13  = probe.read_u8(ctx + 0x13)
            ctx0b  = probe.read_u8(ctx + 0xB)
            ctx0c  = probe.read_u8(ctx + 0xC)
            phase  = probe.read_u16(ctx + 0x6CE)
        end
        local pose_id = probe.read_u32(POSE_ID)
        local hold    = probe.read_u32(HOLD_TIMER)
        local xp      = probe.read_u32(XP_SHOWN)
        local gold    = probe.read_u32(GOLD_SHOWN)
        local fk      = probe.read_u16(FADE_TMPL)
        local ft      = probe.read_u16(FADE_TMPL + 2)
        local p0q, p0c, p0hp = actor_triplet(0)
        local p1q, p1c, p1hp = actor_triplet(1)
        local p2q, p2c, p2hp = actor_triplet(2)
        local _, _, m0hp = actor_triplet(3)
        local sig = string.format("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d",
            signal, cause, survived, mode, flow, astate, ctx13, ctx0b, ctx0c, phase,
            pose_id, hold, xp, gold, fk, ft, p0q, p0c, p0hp, p1q, p1c, p1hp, p2q, p2c, p2hp, m0hp)
        -- Emit on any change, plus a heartbeat every 30 vsyncs so the hold
        -- timer's slope is visible even when nothing else moves.
        if sig ~= last_sig or vsync % 30 == 0 then
            csv:row("%d,%s", vsync, sig)
            rows = rows + 1
            last_sig = sig
        end
        if mode ~= 0x15 and exit_seen == nil then
            exit_seen = vsync
            PCSX.log(string.format("[victory] game_mode left battle at vsync %d (mode=0x%X)", vsync, mode))
        end
        -- Run a little past the exit so the field reload is visible, then stop.
        if exit_seen and vsync > exit_seen + 120 then
            c.stop = true
        end
    end,
    on_done = function()
        PCSX.log(string.format("[victory] %d rows -> %s", rows, OUT_PATH))
    end,
})
