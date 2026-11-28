-- autorun_continue_level_watch.lua
--
-- Who writes the live audio level _DAT_8007B910 between the Continue
-- grid's confirm and the loaded field, and what the title theme's sequence
-- volume does meanwhile (docs/subsystems/audio.md, "The load fades the
-- title theme").
--
-- From load_grid_endgame_card (the title's Continue grid, cursor on cell 0)
-- the probe presses CROSS (block), UP (the confirm's Yes) and CROSS (load)
-- at LEGAIA_PRESS_AT, +100 and +160 vsyncs, then logs every store to the
-- cell with its pc / ra and one row per vsync with the cell, the applied
-- level DAT_8007B6EC, the front-end entry word 0x8007BB00, the card
-- screen's sub-mode 0x801F0204, the launch fade 0x801EF154, the result
-- hold 0x801EF104, the master mode and each sequence record's +0x58.
--
-- Needs the BP core (no --fast): the write watch is a Lua breakpoint.
--
-- Outputs (--out-dir):
--   writes.csv  vsync,pc,ra,old
--   level.csv   vsync,mode,submode,bb00,fade,hold,b910,b6ec,seqvol1,seqvol2,seqvol3
-- Retail RAM-derived: never commit them.
--
-- Env: LEGAIA_FRAMES (900), LEGAIA_PRESS_AT (30), LEGAIA_PRESSES (see below).
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--       --scenario load_grid_endgame_card \
--       --lua scripts/pcsx-redux/autorun_continue_level_watch.lua --frames 900
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 900)
local PRESS_AT    = probe.getenv_num("LEGAIA_PRESS_AT", 30)

local B910      = 0x8007B910
local B6EC      = 0x8007B6EC
local BB00      = 0x8007BB00
local SUBMODE   = 0x801F0204
local FADE      = 0x801EF154
local HOLD      = 0x801EF104
local MODE      = 0x8007B83C
local SEQ_TABLE = 0x801CD2C0

local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function s32(a)
    local v = u32(a)
    if v >= 0x80000000 then v = v - 0x100000000 end
    return v
end
local function ptr_ok(p) return p >= 0x80000000 and p < 0x80200000 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end
local function seqvol(slot)
    local p = u32(SEQ_TABLE + slot * 4)
    if not ptr_ok(p) then return -1 end
    return u16(p + 0x58)
end

-- `offset:BUTTON` pairs after LEGAIA_PRESS_AT; the default is the grid's
-- block / Yes / load. From title_menu_idle (cursor on CONTINUE),
-- `0:UP,60:CROSS` takes NEW GAME instead.
local PRESSES = {}
for off, btn in string.gmatch(probe.getenv("LEGAIA_PRESSES", "0:CROSS,100:UP,160:CROSS"),
    "(%d+):(%u+)") do
    PRESSES[#PRESSES + 1] = { at = PRESS_AT + tonumber(off), btn = probe.BTN[btn] }
end

local writes, level
local vsync = -1
local last_b910 = s32(B910)

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        writes = probe.csv_open(probe.out_path("writes.csv"), "vsync,pc,ra,old")
        level = probe.csv_open(probe.out_path("level.csv"),
            "vsync,mode,submode,bb00,fade,hold,b910,b6ec,seqvol1,seqvol2,seqvol3")
        probe.arm_breakpoint(B910, "Write", 4, "b910", function()
            local r = PCSX.getRegisters()
            writes:row("%d,0x%08X,0x%08X,%d", vsync, tou32(r.pc),
                tou32(r.GPR.n.ra), last_b910)
        end)
        return {}
    end,

    on_capture = function(c, elapsed)
        vsync = elapsed
        for _, p in ipairs(PRESSES) do
            if elapsed >= p.at and elapsed < p.at + 4 then
                probe.pad_force(p.btn)
            elseif elapsed == p.at + 4 then
                probe.pad_release(p.btn)
            end
        end
        last_b910 = s32(B910)
        level:row("%d,0x%X,%d,%d,%d,%d,%d,%d,%d,%d,%d", elapsed, u16(MODE),
            s32(SUBMODE), s32(BB00), s32(FADE), s32(HOLD), last_b910, s32(B6EC),
            seqvol(1), seqvol(2), seqvol(3))
    end,

    on_done = function()
        if writes then writes:close() end
        if level then level:close() end
        PCSX.quit(0)
    end,
})
