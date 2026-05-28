-- autorun_find_clut_decode.lua
--
-- Pins the LZS decode that produces the battle-form party CLUT band.
-- Hooks the universal LZS decoder FUN_8001A55C (a0=src_len, a1=src_ptr,
-- a2=dst_ptr); on each call it arms a one-shot Exec BP at the return so
-- it can read the DECOMPRESSED output buffer, and scans that buffer for
-- the retail Noa (row 492) / Gala (row 494) palette signature passed in
-- LEGAIA_NEEDLE_HEX. On a hit it logs src_len/src_ptr/dst_ptr + caller
-- and dumps the compressed source bytes (to byte-match against a PROT
-- entry offline) and the decompressed output.
--
-- Run from a BAND-ABSENT full-party field sstate (e.g. slot 4: on the
-- map, full party, no battle yet) and hold a walk direction so a random
-- encounter fires and the band loads fresh:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate4 \
--   LEGAIA_FRAMES=2400 LEGAIA_HOLD=DOWN LEGAIA_HOLD_FRAMES=1800 \
--   LEGAIA_NEEDLE_HEX=<64-hex-bytes from a battle VRAM row 492> \
--   LEGAIA_OUT_DIR=/tmp/clutprobe/decodes \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_find_clut_decode.lua \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local LZS_ENTRY  = 0x8001A55C
local SSTATE     = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4")
local FRAMES     = probe.getenv_num("LEGAIA_FRAMES", 2400)
local HOLD_NAME  = probe.getenv("LEGAIA_HOLD", "DOWN")
local HOLD_FR    = probe.getenv_num("LEGAIA_HOLD_FRAMES", 1800)
local OUT_DIR    = probe.getenv("LEGAIA_OUT_DIR", "/tmp/clutprobe/decodes")
local NEEDLE_HEX = probe.getenv("LEGAIA_NEEDLE_HEX", "")
local MAX_SCAN   = probe.getenv_num("LEGAIA_MAX_SCAN", 0x80000)

os.execute(string.format("mkdir -p %q", OUT_DIR))
local HOLD_BTN = pad.BTN[HOLD_NAME] or pad.BTN.DOWN

-- needle bytes
local needle = {}
for i = 1, #NEEDLE_HEX, 2 do
    needle[#needle + 1] = tonumber(NEEDLE_HEX:sub(i, i + 1), 16)
end
local nlen = #needle
if nlen == 0 then
    PCSX.log("[clut] WARNING: LEGAIA_NEEDLE_HEX empty; will log all decodes without matching")
end

local csv = probe.csv_open(OUT_DIR .. "/clut_decode.csv",
    "decode_idx,src_len,src_ptr,dst_ptr,ra,matched")

local function dump_bytes(path, addr, len)
    if not probe.in_ram(addr, 1) then return false end
    local fh = io.open(path, "wb")
    if not fh then return false end
    local off = 0
    while off < len do
        local n = math.min(0x4000, len - off)
        local chunk = probe.read_bytes(addr + off, n)
        if chunk == nil then break end
        fh:write(tostring(chunk))
        off = off + n
    end
    fh:close()
    return true
end

-- Search a RAM window for the needle; returns offset or -1.
local function scan_for_needle(base, span)
    if nlen == 0 then return -1 end
    local step = 0x4000
    local off = 0
    while off < span do
        local n = math.min(step + nlen, span - off)
        local buf = probe.read_bytes(base + off, n)
        if buf == nil then break end
        local s = tostring(buf)
        local idx = s:find(string.char(table.unpack(needle)), 1, true)
        if idx then return off + idx - 1 end
        off = off + step
    end
    return -1
end

local decode_idx = 0
local hits = 0

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    hold_button    = HOLD_BTN,
    hold_frames    = HOLD_FR,
    out_path       = OUT_DIR .. "/clut_decode.csv",

    on_arm = function()
        probe.arm_breakpoint(LZS_ENTRY, "Exec", 4, "lzs", function()
            local r = PCSX.getRegisters()
            local src_len = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local src_ptr = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            local dst_ptr = bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)
            local ra      = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            decode_idx = decode_idx + 1
            local my_idx = decode_idx
            -- one-shot BP at return: decode is done, scan dst.
            local bp
            bp = PCSX.addBreakpoint(ra, "Exec", 4, "probe:lzs_ret", function()
                pcall(function() bp:remove() end)
                local matched = -1
                if probe.in_ram(dst_ptr, 1) then
                    matched = scan_for_needle(dst_ptr, MAX_SCAN)
                end
                csv:row("%d,%d,0x%08X,0x%08X,0x%08X,%d",
                    my_idx, src_len, src_ptr, dst_ptr, ra, matched)
                if matched >= 0 then
                    hits = hits + 1
                    PCSX.log(string.format(
                        "[clut] *** HIT decode #%d: needle at dst+0x%X (dst=0x%08X) "
                        .. "src_ptr=0x%08X src_len=%d ra=0x%08X ***",
                        my_idx, matched, dst_ptr, src_ptr, src_len, ra))
                    dump_bytes(string.format("%s/hit_%03d_src.bin", OUT_DIR, my_idx),
                        src_ptr, math.min(src_len, 0x40000))
                    dump_bytes(string.format("%s/hit_%03d_dst.bin", OUT_DIR, my_idx),
                        dst_ptr, math.min(MAX_SCAN, 0x40000))
                end
            end)
        end)
        return {}
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== find-clut-decode: %d decodes, %d hit(s) ===",
            decode_idx, hits))
    end,
})
