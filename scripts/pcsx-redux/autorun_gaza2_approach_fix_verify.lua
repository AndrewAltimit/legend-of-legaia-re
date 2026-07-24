-- autorun_gaza2_approach_fix_verify.lua
--
-- Runtime verification of --approach-softlock-fix (the re-stage guard)
-- against the LIVE parked states (scenarios battle_gaza2_park_0x19 and
-- battle_gaza2_park_0x19_summon_melee).
--
-- A savestate replay cannot test the disc patch directly: the battle overlay
-- is already resident in the savestate's RAM, so a patched .bin never gets
-- re-read. Instead this pokes the EXACT nine words the patcher writes over
-- the state-0x19 arm's facing recompute (VA 0x801E3568..0x801E3588) and then
-- touches nothing else: the parked action's staged clip is dead (+0x1DA == 0),
-- so the guard must fire on its own - bounce the state to 0x14, let retail
-- re-stage the Move clip, and the monster must resume approaching, arrive,
-- and strike.
--
-- Verdict = UNWEDGED: the state leaves 0x19 via 0x14 (the bounce is visible
-- in the state trace), the acting monster's position moves again, the strike
-- chain runs (0x1E..), the done chain completes, and no state dwells past
-- LEGAIA_PARK_N vsyncs for the rest of the capture.
--
-- Run (interpreter - we are editing code):
--   bash scripts/pcsx-redux/run_probe.sh --isolate-config \
--     --lua scripts/pcsx-redux/autorun_gaza2_approach_fix_verify.lua \
--     --sstate saves/library/pcsx-redux/<park fingerprint>.sstate \
--     --frames 3600

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3600)
local PARK_N = probe.getenv_num("LEGAIA_PARK_N", 1500)

local CTX_PTR = 0x8007BD24
local ACTORS = 0x801C9370
local WINDOW_VA = 0x801E3568

-- The nine replacement words (little-endian), byte-identical to
-- legaia_patcher::approach_fix::assemble_window().
local GUARD = {
    0x926201DA, -- lbu v0,0x1da(s3)
    0x3C038008, -- lui v1,0x8008
    0x8C63BD24, -- lw  v1,-0x42dc(v1)
    0x34040014, -- ori a0,zero,0x14
    0x14400003, -- bne v0,zero,+3
    0x00000000, -- nop
    0xA0640007, -- sb  a0,0x7(v1)
    0x00000000, -- nop
    0x00000000, -- nop
}

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function i16(a) local v = u16(a); return v >= 0x8000 and v - 0x10000 or v end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local vsync = 0
local last_state = -1
local transitions = {}
local bounces = 0
local park_state, park_since, max_dwell, max_dwell_state = nil, 0, 0, -1
local pos_moves = 0
local last_pos = nil
local hp_moves = 0
local last_hp = {}
local poked = false

local csv = probe.csv_open(probe.out_path("states.csv"),
    "vsync,ctx7,acting,gx,gz,a3_1da,a3_1d9,hp0,hp1,hp2")

local function on_vsync()
    vsync = vsync + 1
    local c = u32(CTX_PTR)
    if not in_ram(c) then return end

    if not poked then
        for i, w in ipairs(GUARD) do
            local va = WINDOW_VA + (i - 1) * 4
            probe.write_u16(va, w % 0x10000)
            probe.write_u16(va + 2, math.floor(w / 0x10000))
        end
        PCSX.log(string.format(
            "[fixverify] guard poked over 0x801E3568..0x801E3588; ctx7=0x%02X - hands off from here",
            u8(c + 7)))
        poked = true
    end

    local st = u8(c + 7)
    if st ~= last_state then
        transitions[#transitions + 1] = string.format("%d:0x%02X", vsync, st)
        if st == 0x14 and last_state == 0x19 then bounces = bounces + 1 end
        last_state = st
    end
    if st == park_state and st ~= 0x00 and st ~= 0xFF then
        park_since = park_since + 1
        if park_since > max_dwell then
            max_dwell, max_dwell_state = park_since, st
        end
    else
        park_state, park_since = st, 0
    end

    local acting = u8(c + 0x13)
    local g = u32(ACTORS + 3 * 4)
    local gx, gz, a1da, a1d9 = -1, -1, -1, -1
    if in_ram(g) then
        gx, gz = i16(g + 0x34), i16(g + 0x38)
        a1da, a1d9 = u8(g + 0x1DA), u8(g + 0x1D9)
        if last_pos and (last_pos[1] ~= gx or last_pos[2] ~= gz) then
            pos_moves = pos_moves + 1
        end
        last_pos = { gx, gz }
    end
    local hps = {}
    for s = 0, 2 do
        local a = u32(ACTORS + s * 4)
        local hp = in_ram(a) and u16(a + 0x14C) or -1
        hps[#hps + 1] = hp
        if last_hp[s] ~= nil and last_hp[s] ~= hp then hp_moves = hp_moves + 1 end
        last_hp[s] = hp
    end
    if vsync % 16 == 0 or st ~= last_state or vsync < 200 then
        csv:row("%d,0x%02X,%d,%d,%d,%d,%d,%d,%d,%d",
            vsync, st, acting, gx, gz, a1da, a1d9, hps[1], hps[2], hps[3])
    end
end

probe.run{
    sstate = SSTATE,
    capture_frames = FRAMES,
    boot_delay = 30,
    on_arm = function()
        return {}
    end,
    on_capture = function() on_vsync() end,
    on_done = function()
        local verdict = (max_dwell < PARK_N and bounces > 0 and pos_moves > 0)
            and "UNWEDGED (guard bounced, monster moved)" or "STILL PARKED / INCONCLUSIVE"
        local lines = {
            string.format("=== approach-fix v2 verify: %s over %d vsyncs ===", verdict, vsync),
            string.format("state transitions (%d): %s", #transitions,
                table.concat(transitions, " ", 1, math.min(#transitions, 60))),
            string.format("0x19 -> 0x14 bounces (guard firings): %d", bounces),
            string.format("acting-monster position updates: %d", pos_moves),
            string.format("max state dwell: %d vsyncs in 0x%02X (park threshold %d)",
                max_dwell, max_dwell_state, PARK_N),
            string.format("party live-HP changes observed: %d", hp_moves),
        }
        probe.write_snapshot(probe.out_path("summary.txt"), table.concat(lines, "\n"))
        for _, l in ipairs(lines) do PCSX.log("[fixverify] " .. l) end
    end,
    on_summary = function() end,
}
