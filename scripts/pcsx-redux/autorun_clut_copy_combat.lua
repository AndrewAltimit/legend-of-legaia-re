-- autorun_clut_copy_combat.lua
--
-- Full enumeration of a character's party-palette CLUT copies, driven THROUGH
-- combat. FUN_80053B9C copies one source CLUT struct
-- [u16 base][u16 count][BGR555] into the palette row (a3 = slot -> VRAM row
-- 481+slot). A short load-only capture misses copies that fire when the party
-- first renders in combat, so this probe (a) runs a long window and (b) pulses
-- the Cross button to advance dialogue -> command menu -> Attack -> combat. For
-- each call it logs base, count, and the FULL colour list read from a0 (the
-- clean post-decompress source struct) -- giving the complete per-character
-- palette to reconstruct + to locate each stream in PROT 0861.
--
-- Run (PCSX sstate5 = agreed-to-fight, auto-loads the Tetsu battle):
--   LEGAIA_FRAMES=3000 \
--   timeout --kill-after=30s 900s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate $HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--       --lua scripts/pcsx-redux/autorun_clut_copy_combat.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 3000)
local OUT_PATH = probe.out_path("clut_copy_combat.csv")

local FUN = 0x80053B9C
local CROSS = probe.BTN.CROSS

local function u32(r, nm)
    local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
    return ok and v or 0
end

local csv = probe.csv_open(OUT_PATH, "tick,src_ptr,slot,base,count,colors_hex")
local n = 0
local CAP = 400
-- de-dup identical (slot,base,count,col0) so repeated per-frame uploads don't spam
local seen = {}

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(FUN, "Exec", 4, "clutcopy", function()
            if n >= CAP then return end
            local r = PCSX.getRegisters()
            local a0 = u32(r, "a0")
            local a3 = u32(r, "a3") % 0x100
            if not probe.in_ram(a0, 8) then return end
            local base = probe.read_u16(a0)
            local count = probe.read_u16(a0 + 2)
            if count == 0 or count > 0x100 then return end
            local col0 = probe.read_u16(a0 + 4)
            local key = string.format("%d:%x:%x:%x", a3, base, count, col0)
            if seen[key] then return end
            seen[key] = true
            n = n + 1
            -- read the full colour list
            local parts = {}
            for k = 0, count - 1 do
                parts[#parts + 1] = string.format("%04X", probe.read_u16(a0 + 4 + k * 2))
            end
            local hex = table.concat(parts)
            csv:row("%d,0x%08X,%d,0x%04X,0x%04X,%s", n, a0, a3, base, count, hex)
            PCSX.log(string.format("[clut] #%d slot=%d base=0x%X count=0x%X col0=%04X",
                n, a3, base, count, col0))
        end)
        return { { addr = FUN, name = "clutcopy" } }
    end,

    on_capture = function(_ctx, vsync)
        -- Drive through dialogue / command menu / attack: pulse Cross.
        -- Press for 6 of every 36 vsyncs (after the battle has had time to load).
        if vsync > 120 then
            local phase = vsync % 36
            if phase == 0 then
                probe.pad_force(CROSS)
            elseif phase == 6 then
                probe.pad_release(CROSS)
            end
        end
    end,

    on_done = function()
        csv:close()
        probe.pad_release(CROSS)
        PCSX.log(string.format("=== clut-copy-combat probe: %d distinct CLUT copies ===", n))
    end,
})
