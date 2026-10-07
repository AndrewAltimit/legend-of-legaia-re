-- autorun_palette_src_writer.lua
--
-- Finds what populates the source CLUT struct that FUN_80053B9C copies into the
-- party-palette block. That source struct lives at 0x800D6C98 in a clean Tetsu
-- fight ([u16 base][u16 count][BGR555 colours]; see character-mesh.md). This
-- probe write-watchpoints the struct HEADER word (0x800D6C98) -- the copy
-- routine's STP-set writes the colours at +4, not the header, so header writes
-- are the loader that creates the struct. Each hit logs the writer PC + all GPRs
-- (so an LZS-decode input cursor or a memcpy source pointer surfaces), and the
-- first hit dumps a wide RAM window around any register that points into RAM.
--
-- Run (PCSX sstate5 = agreed-to-fight, auto-loads the battle, no input):
--   LEGAIA_FRAMES=900 \
--   timeout --kill-after=20s 500s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate $HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--       --lua scripts/pcsx-redux/autorun_palette_src_writer.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_PATH = probe.out_path("palette_src_writer.csv")
local OUT_DIR  = OUT_PATH:gsub("/[^/]*$", "")

local SRC = 0x800D6C98  -- source CLUT struct header word

local GPR_NAMES = {
    "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}
local function u32(r, nm)
    local ok, v = pcall(function() return tonumber(r.GPR.n[nm]) % 0x100000000 end)
    return ok and v or 0
end
local function gpr_dump(r)
    local p = {}
    for _, nm in ipairs(GPR_NAMES) do p[#p + 1] = string.format("%s=%08X", nm, u32(r, nm)) end
    return table.concat(p, " ")
end

local csv = probe.csv_open(OUT_PATH, "tick,pc,preval")
local logged = 0
local LOG_CAP = 40
local dumped = false

-- Dump a wide window around every distinct RAM-pointing register (the loader's
-- source is one of them), once, on the first write.
local function dump_sources(r)
    if dumped then return end
    dumped = true
    local seen = {}
    for _, nm in ipairs(GPR_NAMES) do
        local v = u32(r, nm)
        if v >= 0x80050000 and v < 0x80200000 then
            local lo = v - 0x2000
            lo = lo - (lo % 0x1000)
            if not seen[lo] then
                seen[lo] = true
                local b = probe.read_bytes(lo, 0x8000)
                if b ~= nil then
                    local f = string.format("%s/srcreg_%s_%08X.bin", OUT_DIR, nm, lo)
                    local fh = io.open(f, "wb")
                    if fh then fh:write(tostring(b)); fh:close() end
                end
            end
        end
    end
    PCSX.log(string.format("[psw] dumped source windows around RAM regs -> %s", OUT_DIR))
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(SRC, "Write", 4, "src", function()
            if logged >= LOG_CAP then return end
            logged = logged + 1
            local r = PCSX.getRegisters()
            local pc = u32(r, "pc") -- pc isn't in GPR.n; read directly
            pc = (tonumber(r.pc) or 0) % 0x100000000
            local preval = probe.read_u32(SRC)
            csv:row("%d,0x%08X,0x%08X", logged, pc, preval)
            PCSX.log(string.format("[psw] #%d pc=0x%08X preval=0x%08X", logged, pc, preval))
            PCSX.log(string.format("[psw]   GPR %s", gpr_dump(r)))
            dump_sources(r)
        end)
        return { { addr = SRC, name = "src" } }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== palette-src-writer probe: writes=%d  final 0x800D6C98=0x%08X ===",
            logged, probe.read_u32(SRC)))
    end,
})
