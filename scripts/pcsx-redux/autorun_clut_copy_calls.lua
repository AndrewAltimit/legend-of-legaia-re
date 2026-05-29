-- autorun_clut_copy_calls.lua
--
-- Enumerates every party-palette CLUT copy at battle load. FUN_80053B9C copies
-- ONE source CLUT struct [u16 base][u16 count][BGR555] into a character's palette
-- row (a3 = slot -> VRAM row 481+slot) with the STP bit OR-ed in, and is called
-- once per struct (several times per character). An Exec BP at its entry fires at
-- low frequency (a few dozen total), so it's safe (unlike the LZS-entry BP).
--
-- For each call it logs the source struct ptr (a0), the slot (a3 & 0xff), and the
-- struct's base/count read from a0. That gives the full per-character CLUT-struct
-- list; matching a0 against the decompressed 0861 buffer maps each to a disc
-- offset so the palette can be reproduced offline.
--
-- Run (PCSX sstate5 = agreed-to-fight, auto-loads the battle):
--   LEGAIA_FRAMES=900 \
--   timeout --kill-after=20s 500s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate $HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--       --lua scripts/pcsx-redux/autorun_clut_copy_calls.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_PATH = probe.out_path("clut_copy_calls.csv")

local FUN = 0x80053B9C

local function u32(r, nm)
    local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
    return ok and v or 0
end

local csv = probe.csv_open(OUT_PATH, "tick,src_ptr,slot,base,count,col0,col1")
local n = 0
local CAP = 200

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(FUN, "Exec", 4, "clutcopy", function()
            if n >= CAP then return end
            n = n + 1
            local r = PCSX.getRegisters()
            local a0 = u32(r, "a0")
            local a3 = u32(r, "a3") % 0x100
            local base, count, c0, c1 = 0, 0, 0, 0
            if probe.in_ram(a0, 8) then
                base = probe.read_u16(a0)
                count = probe.read_u16(a0 + 2)
                c0 = probe.read_u16(a0 + 4)
                c1 = probe.read_u16(a0 + 6)
            end
            csv:row("%d,0x%08X,%d,0x%04X,0x%04X,0x%04X,0x%04X", n, a0, a3, base, count, c0, c1)
            PCSX.log(string.format(
                "[clut] #%d src=0x%08X slot=%d base=0x%X count=0x%X col0=%04X col1=%04X",
                n, a0, a3, base, count, c0, c1))
        end)
        return { { addr = FUN, name = "clutcopy" } }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== clut-copy-calls probe: %d calls ===", n))
    end,
})
