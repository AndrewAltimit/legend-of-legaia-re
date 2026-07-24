-- autorun_gaza2_approach_fix_verify.lua
--
-- Runtime verification of the --approach-softlock-fix patch word against the
-- LIVE parked state (scenario battle_gaza2_park_0x19).
--
-- A savestate replay cannot test the disc patch directly: the battle overlay
-- is already resident in the savestate's RAM, so a patched .bin never gets
-- re-read. Instead this pokes the exact word the patcher writes -
-- `j 0x801E3204` (0x08078C81) at VA 0x801E32AC - into the parked RAM and
-- watches the state machine: if the fix is right, the very next 0x14 pass...
-- no - the parked action sits in state 0x19, which is DOWNSTREAM of the
-- patched jump, so this state never re-runs 0x14. The poke therefore proves
-- un-wedging only for the FOLLOW-ON actions (Vahn's queued attack against
-- Gaza would re-enter 0x14). To break the *current* park the probe also
-- performs the fix's semantic effect once: it advances ctx+7 from 0x19 to
-- 0x1E by hand (exactly what the patched jump would have done at staging
-- time), then lets everything else run retail.
--
-- Verdict = the battle PROGRESSES: the state machine leaves the attack band,
-- later actions run (including at least one fresh 0x14 pass through the
-- patched word), HP moves, and no state parks past LEGAIA_PARK_N vsyncs for
-- the rest of the capture.
--
-- Run (interpreter - we are editing code):
--   bash scripts/pcsx-redux/run_probe.sh \
--     --lua scripts/pcsx-redux/autorun_gaza2_approach_fix_verify.lua \
--     --sstate saves/library/pcsx-redux/814dce6b90da114a8d8d37386a90623c4f871f7e380e14c010889ab4414c9dd8.sstate \
--     --frames 3600

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 3600)
local PARK_N = probe.getenv_num("LEGAIA_PARK_N", 1500)

local CTX_PTR = 0x8007BD24
local ACTORS = 0x801C9370
local HOOK_VA = 0x801E32AC
local FIX_LO, FIX_HI = 0x8C81, 0x0807 -- j 0x801E3204, little-endian halves

local function u8(a) return probe.read_u8(a) or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function in_ram(a) return a >= 0x80000000 and a < 0x80200000 end

local vsync = 0
local last_state = -1
local transitions = {}
local fresh_0x14 = 0
local park_state, park_since, max_dwell, max_dwell_state = nil, 0, 0, -1
local hp_moves = 0
local last_hp = {}
local poked = false

local csv = probe.csv_open(probe.out_path("states.csv"), "vsync,ctx7,acting,hp0,hp2,hp3")

local function on_vsync()
    vsync = vsync + 1
    local c = u32(CTX_PTR)
    if not in_ram(c) then return end

    if not poked then
        -- Poke the patch word, then perform its semantic effect on the
        -- already-staged parked action: 0x19 -> 0x1E.
        probe.write_u16(HOOK_VA, FIX_LO)
        probe.write_u16(HOOK_VA + 2, FIX_HI)
        if u8(c + 7) == 0x19 then
            -- Mirror the fix's landing site exactly: state = 0x1E and the
            -- ctx+0xD mask the in-range continuation applies (0x801E3204..2C).
            probe.write_u8(c + 7, 0x1E)
            probe.write_u8(c + 0xD, u8(c + 0xD) % 2)
            PCSX.log("[fixverify] poked j 0x801E3204 at 0x801E32AC; advanced parked 0x19 -> 0x1E")
        else
            PCSX.log(string.format(
                "[fixverify] poked patch word; ctx7=0x%02X was not the expected 0x19 park",
                u8(c + 7)))
        end
        poked = true
    end

    local st = u8(c + 7)
    if st ~= last_state then
        transitions[#transitions + 1] = string.format("%d:0x%02X", vsync, st)
        if st == 0x14 then fresh_0x14 = fresh_0x14 + 1 end
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

    for _, s in ipairs({ 0, 2, 3 }) do
        local a = u32(ACTORS + s * 4)
        if in_ram(a) then
            local hp = u16(a + 0x14C)
            if last_hp[s] ~= nil and last_hp[s] ~= hp then hp_moves = hp_moves + 1 end
            last_hp[s] = hp
        end
    end
    if vsync % 32 == 0 or (st ~= 0x19 and vsync < 600) then
        local a0, a2, a3 = u32(ACTORS), u32(ACTORS + 8), u32(ACTORS + 12)
        csv:row("%d,0x%02X,%d,%d,%d,%d", vsync, st, u8(c + 0x13),
            in_ram(a0) and u16(a0 + 0x14C) or -1,
            in_ram(a2) and u16(a2 + 0x14C) or -1,
            in_ram(a3) and u16(a3 + 0x14C) or -1)
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
        local verdict = (max_dwell < PARK_N and #transitions > 3 and hp_moves > 0)
            and "UNWEDGED" or "STILL PARKED / INCONCLUSIVE"
        local lines = {
            string.format("=== approach-fix verify: %s over %d vsyncs ===", verdict, vsync),
            string.format("state transitions (%d): %s", #transitions,
                table.concat(transitions, " ", 1, math.min(#transitions, 60))),
            string.format("fresh 0x14 passes through the patched word: %d", fresh_0x14),
            string.format("max state dwell: %d vsyncs in 0x%02X (park threshold %d)",
                max_dwell, max_dwell_state, PARK_N),
            string.format("live-HP changes observed (seats 0/2/3): %d", hp_moves),
        }
        probe.write_snapshot(probe.out_path("summary.txt"), table.concat(lines, "\n"))
        for _, l in ipairs(lines) do PCSX.log("[fixverify] " .. l) end
    end,
    on_summary = function() end,
}
