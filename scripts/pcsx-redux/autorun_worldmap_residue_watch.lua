-- autorun_worldmap_residue_watch.lua
--
-- One run over the drake_castle_to_worldmap transition closes BOTH A4
-- world-map residue items:
--
--   (a) DAT_8007C018[45..53] mid-load pointers: write-watch each of the 9
--       table words through the scene load. The steady-state model says
--       entries past the walker counter DAT_8007BB38 are stale leftovers
--       never consumed; a write during the load (writer pc) either confirms
--       the sole-writer FUN_80026B4C model or surfaces the mid-load producer
--       the old snapshot saw.
--   (b) Slot-4 freeze flag _DAT_8007B824: write-watch during retail play.
--       Either an overlay sets it live, or BSS-init zero holds and the
--       "persistent slots" semantic is vestigial.
--
-- Run (the scenario resolves the pre-transition Drake Castle save):
--   LEGAIA_FRAMES=1800 timeout --kill-after=30s 1500s \
--     bash scripts/pcsx-redux/run_probe.sh \
--       --lua scripts/pcsx-redux/autorun_worldmap_residue_watch.lua \
--       --scenario drake_castle_to_worldmap --frames 1800

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1800)

local TABLE_BASE = 0x8007C018
local SLOT_LO, SLOT_HI = 45, 53
local FREEZE_FLAG = 0x8007B824
local WALKER_COUNT = 0x8007BB38
local GMODE = 0x8007B83C

local function tou32(v) v = tonumber(v) or 0 if v < 0 then v = v + 0x100000000 end return v end
local function u8(a) return probe.read_u8(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end

local f = assert(io.open(probe.out_path("worldmap_residue_watch.txt"), "w"))
local function w(s) f:write(s .. "\n") f:flush() end

local writes = 0
local function arm_word(addr, name)
    probe.arm_breakpoint(addr, "Write", 4, name, function()
        local r = PCSX.getRegisters()
        local pc = tou32(r.pc)
        local ra = tou32(r.GPR and r.GPR.n and r.GPR.n.ra or 0)
        writes = writes + 1
        w(string.format("[write] %s addr=0x%08X val=0x%08X pc=0x%08X ra=0x%08X gmode=0x%02X",
            name, addr, u32(addr), pc, ra, u8(GMODE)))
    end)
end

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    -- Hold UP from the state load - the proven drive for this save (the
    -- door-dispatch trace triggered the same castle exit this way).
    hold_button = probe.BTN.UP,
    hold_frames = 240,
    on_arm = function()
        w("== world-map residue watch ==")
        for slot = SLOT_LO, SLOT_HI do
            arm_word(TABLE_BASE + slot * 4, string.format("c018[%d]", slot))
        end
        arm_word(FREEZE_FLAG, "freeze_8007B824")
        w(string.format("[arm] gmode=0x%02X walker=%d freeze=0x%08X",
            u8(GMODE), u32(WALKER_COUNT), u32(FREEZE_FLAG)))
        for slot = SLOT_LO, SLOT_HI do
            w(string.format("[arm] c018[%d]=0x%08X", slot, u32(TABLE_BASE + slot * 4)))
        end
        return {}
    end,
    on_capture = function(c, elapsed)
        -- The harness holds UP for the first 240 vsyncs (the castle exit);
        -- afterwards walk Right in bursts so the freeze flag sees live
        -- overworld play on the far side of the warp.
        if elapsed > 400 then
            if (elapsed % 120) < 50 then
                probe.pad_force(probe.BTN.RIGHT)
            else
                probe.pad_release(probe.BTN.RIGHT)
            end
        end
        if elapsed % 150 == 0 then
            w(string.format("[diag t%d] gmode=0x%02X walker=%d freeze=0x%08X writes=%d",
                elapsed, u8(GMODE), u32(WALKER_COUNT), u32(FREEZE_FLAG), writes))
        end
        if elapsed == FRAMES - 5 then
            for slot = SLOT_LO, SLOT_HI do
                w(string.format("[final] c018[%d]=0x%08X", slot, u32(TABLE_BASE + slot * 4)))
            end
            w(string.format("[final] walker=%d freeze=0x%08X total_writes=%d",
                u32(WALKER_COUNT), u32(FREEZE_FLAG), writes))
        end
    end,
})
