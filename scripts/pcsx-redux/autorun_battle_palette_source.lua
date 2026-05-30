-- autorun_battle_palette_source.lua
--
-- Pins the SOURCE of the in-battle party-character palette.
--
-- Background (docs/formats/character-mesh.md):
--   In a turn-based battle the party meshes (PROT 1204) are recoloured by a
--   per-character palette that is a separate, battle-allocated resident block in
--   main RAM -- contiguous at 0x800ebee8 (Vahn) / 0x800ec0c8 (Noa) /
--   0x800ec2a8 (Gala), a fixed 0x1E0 stride = 15 x 16-colour sub-CLUTs / char --
--   DMA'd verbatim to VRAM rows 481/482/483 at battle entry. Those exact bytes
--   are NOT recoverable from the disc by any byte search (raw, every 32-byte
--   sub-CLUT window, and the LZS-container sections of the whole corpus all
--   miss). The block is filled live during the battle-load by an uncaptured
--   overlay; even a screen-black save is already past the fill (the source
--   buffer is freed within a frame). This probe catches the WRITE live.
--
-- Method:
--   Write BPs on the first word of each party-palette block. The first hit per
--   block logs the writing PC + every GPR + the value. The PC localizes the
--   fill routine; the GPRs expose the SOURCE pointer (the buffer the bytes are
--   copied / decompressed from). That source address -> the resident
--   decompressed asset it belongs to -> the disc entry + offset.
--
-- Run (rim_elm_queen_bee_battle = field/pre-load; fight auto-starts a few
-- seconds after load, no input needed -> party palette is filled live):
--   LEGAIA_FRAMES=1200 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_battle_palette_source.lua \
--       timeout --kill-after=30s 900s \
--       bash scripts/pcsx-redux/run_probe.sh --scenario rim_elm_queen_bee_battle \
--           --lua scripts/pcsx-redux/autorun_battle_palette_source.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1200)
local OUT_PATH = probe.out_path("battle_palette_source.csv")

-- Party palette resident blocks (pinned from a clean full-party battle save).
local PAL = {
    { addr = 0x800EBEE8, name = "Vahn" },
    { addr = 0x800EC0C8, name = "Noa" },
    { addr = 0x800EC2A8, name = "Gala" },
}

local GPR_NAMES = {
    "at", "v0", "v1", "a0", "a1", "a2", "a3",
    "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7",
    "s0", "s1", "s2", "s3", "s4", "s5", "s6", "s7",
    "t8", "t9", "k0", "k1", "gp", "sp", "s8", "ra",
}

local function gpr_dump(r)
    local parts = {}
    for _, nm in ipairs(GPR_NAMES) do
        local ok, v = pcall(function()
            return bit.band(tonumber(r.GPR.n[nm]) or 0, 0xFFFFFFFF)
        end)
        if ok then parts[#parts + 1] = string.format("%s=%08X", nm, v) end
    end
    return table.concat(parts, " ")
end

-- For each GPR that points into main RAM, report whether the 32 bytes there
-- match the (eventual) palette block -- i.e. which register is the source.
local function reg_source_scan(r, pal_addr)
    local want = probe.read_bytes(pal_addr, 32)
    if want == nil then return "" end
    local parts = {}
    for _, nm in ipairs(GPR_NAMES) do
        local ok, v = pcall(function()
            return bit.band(tonumber(r.GPR.n[nm]) or 0, 0xFFFFFFFF)
        end)
        if ok and probe.in_ram(v, 32) then
            local got = probe.read_bytes(v, 32)
            if got ~= nil and tostring(got) == tostring(want) then
                parts[#parts + 1] = string.format("%s->0x%08X(SRC-MATCH)", nm, v)
            end
        end
    end
    return table.concat(parts, " ")
end

local csv = probe.csv_open(OUT_PATH, "tick,char,pc,value,src_regs")
local OUT_DIR = OUT_PATH:gsub("/[^/]*$", "")
local hits = {}
for _, p in ipairs(PAL) do hits[p.name] = 0 end
local HIT_CAP = 8

-- One-time dump of the loaded source-asset buffer (the LZS-compressed party
-- palette lives at ~0x80182xxx per the write-watchpoint's source cursor). Dump
-- a wide window the first time the LZS decoder (pc in 0x8001A5xx..0x8001A6xx)
-- writes a palette byte, so an offline grep can pin the disc PROT entry.
local SRC_LO, SRC_HI = 0x80180000, 0x80186000
local dumped_src = false
local dumped_pal = false
local function maybe_dump_src(pc)
    if dumped_src then return end
    pc = pc % 0x100000000 -- CPU regs come back sign-extended; bit.band(,0xFFFFFFFF) won't mask
    if pc < 0x8001A55C or pc > 0x8001A6FF then return end
    dumped_src = true
    local n = SRC_HI - SRC_LO
    local buf = probe.read_bytes(SRC_LO, n)
    if buf == nil then PCSX.log("[pal] src dump FAILED"); return end
    local f = string.format("%s/lzs_srcbuf_%08X_%08X.bin", OUT_DIR, SRC_LO, SRC_HI)
    local fh = io.open(f, "wb")
    if fh then fh:write(tostring(buf)); fh:close()
        PCSX.log(string.format("[pal] dumped source buffer 0x%08X..0x%08X -> %s",
            SRC_LO, SRC_HI, f))
    end
end

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local bps = {}
        for _, p in ipairs(PAL) do
            probe.arm_breakpoint(p.addr, "Write", 4, p.name, function()
                if hits[p.name] >= HIT_CAP then return end
                hits[p.name] = hits[p.name] + 1
                local r = PCSX.getRegisters()
                local pc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
                local val = probe.read_u32(p.addr)
                maybe_dump_src(pc)
                local src = reg_source_scan(r, p.addr)
                csv:row("%d,%s,0x%08X,0x%08X,%s", hits[p.name], p.name, pc, val, src)
                PCSX.log(string.format(
                    "[pal] WRITE %-4s #%d pc=0x%08X new=0x%08X  src=[%s]",
                    p.name, hits[p.name], pc, val, src))
                PCSX.log(string.format("[pal]   GPR %s", gpr_dump(r)))
            end)
            table.insert(bps, { addr = p.addr, name = p.name })
        end
        return bps
    end,

    on_capture = function(_ctx, vsync_in_capture)
        -- on_done doesn't fire in this battle scenario (the run never self-quits),
        -- so dump the resident palette region once at a late capture frame.
        if vsync_in_capture == 400 and not dumped_pal then
            dumped_pal = true
            local lo, hi = 0x800EB000, 0x800ED000
            local b = probe.read_bytes(lo, hi - lo)
            if b ~= nil then
                local f = string.format("%s/palregion_v400_%08X_%08X.bin", OUT_DIR, lo, hi)
                local fh = io.open(f, "wb")
                if fh then fh:write(tostring(b)); fh:close()
                    PCSX.log(string.format("[pal] dumped palette region @v400 -> %s", f))
                end
            end
        end
    end,

    on_done = function()
        csv:close()
        local lo, hi = 0x800EB000, 0x800ED000
        local b = probe.read_bytes(lo, hi - lo)
        if b ~= nil then
            local f = string.format("%s/final_palregion_%08X_%08X.bin", OUT_DIR, lo, hi)
            local fh = io.open(f, "wb")
            if fh then fh:write(tostring(b)); fh:close()
                PCSX.log(string.format("[pal] dumped final palette region -> %s", f))
            end
        end
        PCSX.log(string.format(
            "=== palette probe hits: Vahn=%d Noa=%d Gala=%d ===",
            hits.Vahn, hits.Noa, hits.Gala))
    end,
})
