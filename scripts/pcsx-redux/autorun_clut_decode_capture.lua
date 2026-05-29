-- autorun_clut_decode_capture.lua
--
-- Single combined capture to pin the party-CLUT records' 0861 offsets. Arms an
-- Exec BP at the LZS decoder entry (FUN_8001A55C), filtered to CLUT-region
-- destinations, and logs each decode's a0(len)/a1(src)/a2(dst)/ra. On the first
-- such decode it ALSO dumps the loaded-0861 buffer window around a1 -- so that an
-- offline grep of that dump against PROT 0861 establishes this run's buffer_base
-- and maps every captured a1 to a stable 0861 file offset (the record start).
--
-- Run (PCSX sstate5 = agreed-to-fight):
--   LEGAIA_FRAMES=900 \
--   timeout --kill-after=20s 500s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate $HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--       --lua scripts/pcsx-redux/autorun_clut_decode_capture.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_PATH = probe.out_path("clut_decode_capture.csv")
local OUT_DIR  = OUT_PATH:gsub("/[^/]*$", "")

local LZS = 0x8001A55C
local DST_LO, DST_HI = 0x800D0000, 0x800E4000

local csv = probe.csv_open(OUT_PATH, "tick,src_len,src_ptr,dst_ptr,ra")
local n = 0
local CAP = 24
local dumped = false

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(LZS, "Exec", 4, "lzs", function()
            if n >= CAP then return end
            local r = PCSX.getRegisters()
            local a2 = (tonumber(r.GPR.n.a2) or 0) % 0x100000000
            if a2 < DST_LO or a2 >= DST_HI then return end
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100000000
            local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100000000
            local ra = (tonumber(r.GPR.n.ra) or 0) % 0x100000000
            n = n + 1
            csv:row("%d,0x%08X,0x%08X,0x%08X,0x%08X", n, a0, a1, a2, ra)
            PCSX.log(string.format("[cap] #%d len=0x%X src=0x%08X dst=0x%08X ra=0x%08X",
                n, a0, a1, a2, ra))
            -- On the first CLUT decode, dump the 0861 buffer window around a1.
            if not dumped then
                dumped = true
                local lo = a1 - 0x6000
                lo = lo - (lo % 0x1000)
                local sz = 0x20000  -- 128 KB: covers all records' compressed data
                local b = probe.read_bytes(lo, sz)
                if b ~= nil then
                    local f = string.format("%s/buf0861_%08X.bin", OUT_DIR, lo)
                    local fh = io.open(f, "wb")
                    if fh then fh:write(tostring(b)); fh:close()
                        PCSX.log(string.format("[cap] dumped 0861 buffer 0x%08X..0x%08X -> %s",
                            lo, lo + sz, f))
                    end
                end
            end
        end)
        return { { addr = LZS, name = "lzs" } }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== clut-decode-capture: %d decodes ===", n))
    end,
})
