-- autorun_battle_palette_lzs_src.lua
--
-- Captures the COMPRESSED on-disc bytes of the in-battle party palette.
--
-- The party palette is LZS-decompressed (FUN_8001A55C, signature
-- (a0=src_len, a1=src_ptr, a2=dst_ptr)) directly into the resident block at
-- 0x800EBEE8 (Vahn) / 0x800EC0C8 (Noa) / 0x800EC2A8 (Gala) -- pinned by the
-- write-watchpoint in autorun_battle_palette_source.lua. The DECOMPRESSED
-- bytes are absent from the disc by every byte search; the on-disc form is the
-- COMPRESSED stream. This probe arms an Exec BP at the LZS entry and, for every
-- decode whose dst lands in the palette region, dumps the compressed source
-- (a1, a0 bytes) so an offline grep pins the exact PROT entry + offset.
--
-- Run (queen_bee battle auto-starts after load, no input):
--   LEGAIA_FRAMES=1800 \
--   timeout --kill-after=30s 900s \
--   bash scripts/pcsx-redux/run_probe.sh --scenario rim_elm_queen_bee_battle \
--       --lua scripts/pcsx-redux/autorun_battle_palette_lzs_src.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local LZS_ENTRY = 0x8001A55C
local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate8")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 1800)
local OUT_PATH = probe.out_path("battle_palette_lzs_src.csv")
local OUT_DIR  = OUT_PATH:gsub("/[^/]*$", "")

-- Palette region with margin (3 blocks of 0x1E0 from 0x800EBEE8, + slack).
local PAL_LO = 0x800EB000
local PAL_HI = 0x800ED000

local function n32(v) return bit.band(v, 0xFFFFFFFF) end
local function head_hex(buf, n)
    local s = tostring(buf); local lim = math.min(n or 16, #s); local p = {}
    for i = 1, lim do p[#p + 1] = string.format("%02X", s:byte(i)) end
    return table.concat(p)
end

local csv = probe.csv_open(OUT_PATH, "decode_idx,src_len,src_ptr,dst_ptr,pc,ra,head_hex,file")
local idx = 0
local pal_hits = 0
local PAL_CAP = 16

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(LZS_ENTRY, "Exec", 4, "lzs", function()
            local r = PCSX.getRegisters()
            local src_len = n32(tonumber(r.GPR.n.a0) or 0)
            local src_ptr = n32(tonumber(r.GPR.n.a1) or 0)
            local dst_ptr = n32(tonumber(r.GPR.n.a2) or 0)
            local pc = n32(tonumber(r.pc) or 0)
            local ra = n32(tonumber(r.GPR.n.ra) or 0)
            idx = idx + 1
            -- Only care about decodes that target the palette region.
            if dst_ptr < PAL_LO or dst_ptr >= PAL_HI then return end
            if pal_hits >= PAL_CAP then return end
            pal_hits = pal_hits + 1
            local fp, file = "", ""
            local dump = math.min(src_len > 0 and src_len or 0, 0x4000)
            if dump > 0 and probe.in_ram(src_ptr, dump) then
                local buf = probe.read_bytes(src_ptr, dump)
                if buf ~= nil then
                    fp = head_hex(buf, 16)
                    file = string.format("%s/palsrc_%02d_src%08X_dst%08X_len%d.bin",
                        OUT_DIR, pal_hits, src_ptr, dst_ptr, src_len)
                    local fh = io.open(file, "wb")
                    if fh then fh:write(tostring(buf)); fh:close() end
                end
            end
            csv:row("%d,%d,0x%08X,0x%08X,0x%08X,0x%08X,%s,%s",
                idx, src_len, src_ptr, dst_ptr, pc, ra, fp, file)
            PCSX.log(string.format(
                "[palsrc] LZS dst=0x%08X src=0x%08X len=%d pc=0x%08X ra=0x%08X head=%s -> %s",
                dst_ptr, src_ptr, src_len, pc, ra, fp, file))
        end)
        return { { addr = LZS_ENTRY, name = "lzs" } }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== palette LZS-src probe: total decodes=%d palette-dst dumps=%d ===",
            idx, pal_hits))
    end,
})
