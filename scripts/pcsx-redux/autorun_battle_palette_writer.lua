-- autorun_battle_palette_writer.lua
--
-- Pins the routine that assembles the in-battle party palette.
--
-- The Vahn battle palette (0x800EBEE8 -> VRAM row 481, value word 0x90709D40)
-- is character-intrinsic and produced fresh at battle load, but is NOT a stored
-- disc blob (no LZS stream decompresses to it; see docs/formats/character-mesh.md
-- + the `lzs-decode find` brute). So it is assembled/computed at battle entry.
-- 0x800EBEE8 is a SHARED work-arena address that the scene-bundle LZS decompress
-- (FUN_8001A55C) and the arena memset (SCUS 0x80055F14) also write -- those are
-- noise. This probe watches the word and logs every writer EXCEPT those two, with
-- PC + all GPRs, so the palette-assembly routine + its source registers surface.
--
-- Run from a save just before the battle loads (the scripted Tetsu fight, which
-- needs no input):
--   LEGAIA_FRAMES=900 \
--   timeout --kill-after=20s 500s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate $HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--       --lua scripts/pcsx-redux/autorun_battle_palette_writer.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_PATH = probe.out_path("battle_palette_writer.csv")

local PAL = 0x800EBEE8        -- Vahn palette word 0 (-> VRAM row 481)
local PAL_WORD = 0x90709D40   -- bytes 40 9d 70 90 (palette[0..3])

-- Known noise writers to the shared arena address.
local function is_noise(pc)
    pc = pc % 0x100000000
    if pc >= 0x8001A55C and pc <= 0x8001A6FF then return true end -- LZS decoder
    if pc == 0x80055F14 then return true end                      -- arena memset
    return false
end

local GPR_NAMES = {
    "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}
local function pcall_u32(r, nm)
    local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
    return ok and v or 0
end
local function gpr_dump(r)
    local p = {}
    for _, nm in ipairs(GPR_NAMES) do
        local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
        if ok then p[#p + 1] = string.format("%s=%08X", nm, v) end
    end
    return table.concat(p, " ")
end

local csv = probe.csv_open(OUT_PATH, "tick,pc,preval,had_palword")
local OUT_DIR = OUT_PATH:gsub("/[^/]*$", "")
local noise = 0
local logged = 0
local LOG_CAP = 60
local dumped_src = false

-- On the first palette-assembler (0x80053C6C) write, dump the buffer around the
-- source CLUT pointer s0 so an offline grep can pin its disc PROT entry.
local function dump_source(r)
    if dumped_src then return end
    local s0 = pcall_u32(r, "s0")
    if s0 == 0 then return end
    dumped_src = true
    local lo = s0 - 0x4000
    lo = lo - (lo % 0x1000) -- page-align (LuaJIT has no & operator)
    local hi = lo + 0x10000
    local b = probe.read_bytes(lo, hi - lo)
    if b ~= nil then
        local f = string.format("%s/srcclut_%08X_%08X_s0=%08X.bin", OUT_DIR, lo, hi, s0)
        local fh = io.open(f, "wb")
        if fh then fh:write(tostring(b)); fh:close()
            PCSX.log(string.format("[palw] dumped source-CLUT region 0x%08X..0x%08X (s0=0x%08X) -> %s",
                lo, hi, s0, f))
        end
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(PAL, "Write", 4, "pal", function()
            local r = PCSX.getRegisters()
            local pc = (tonumber(r.pc) or 0) % 0x100000000
            if is_noise(pc) then noise = noise + 1; return end
            if logged >= LOG_CAP then return end
            logged = logged + 1
            local preval = probe.read_u32(PAL)
            -- flag if any GPR currently holds the palette word (the value being stored)
            local had = 0
            for _, nm in ipairs(GPR_NAMES) do
                local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
                if ok and (v == PAL_WORD) then had = 1 end
            end
            csv:row("%d,0x%08X,0x%08X,%d", logged, pc, preval, had)
            PCSX.log(string.format("[palw] #%d pc=0x%08X preval=0x%08X had_palword=%d",
                logged, pc, preval, had))
            PCSX.log(string.format("[palw]   GPR %s", gpr_dump(r)))
            if pc == 0x80053C6C then dump_source(r) end
        end)
        return { { addr = PAL, name = "pal" } }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== palette-writer probe: non-noise writes=%d  noise(LZS/memset)=%d  final 0x800EBEE8=0x%08X ===",
            logged, noise, probe.read_u32(PAL)))
    end,
})
