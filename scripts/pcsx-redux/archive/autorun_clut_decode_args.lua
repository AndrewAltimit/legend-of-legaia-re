-- autorun_clut_decode_args.lua
--
-- Traces 0861's per-record decompression. The party CLUT structs are LZS-
-- decompressed (FUN_8001A55C) from records inside the raw-loaded PROT 0861
-- buffer into the 0x800Dxxxx struct region, then copied by FUN_80053B9C. The
-- `lzs-decode find` offsets are LZS re-sync points, not real record starts; the
-- real start is the decoder's `a1` (src ptr) at entry. This probe arms an Exec
-- BP at the decoder entry and, ONLY for decodes whose destination (a2) lands in
-- the CLUT-struct region, logs a0(src_len)/a1(src_ptr)/a2(dst)/ra(caller). a1
-- gives the real 0861 record offset (a1 - 0861_buffer_base); ra identifies the
-- index-walker routine that computed it (the archive index).
--
-- Run (PCSX sstate5 = agreed-to-fight):
--   LEGAIA_FRAMES=900 \
--   timeout --kill-after=20s 500s \
--   bash scripts/pcsx-redux/run_probe.sh \
--       --sstate $HOME/Tools/pcsx-redux/SCUS94254.sstate5 \
--       --lua scripts/pcsx-redux/autorun_clut_decode_args.lua

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate5")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 900)
local OUT_PATH = probe.out_path("clut_decode_args.csv")

local LZS = 0x8001A55C
-- The CLUT-struct buffer region (decompress destinations we care about).
local DST_LO, DST_HI = 0x800D0000, 0x800E4000

local csv = probe.csv_open(OUT_PATH, "tick,src_len,src_ptr,dst_ptr,ra")
local n = 0
local CAP = 24

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        probe.arm_breakpoint(LZS, "Exec", 4, "lzs", function()
            if n >= CAP then return end
            local r = PCSX.getRegisters()
            -- minimal: read dst (a2) first, bail fast if not a CLUT decode
            local a2 = (tonumber(r.GPR.n.a2) or 0) % 0x100000000
            if a2 < DST_LO or a2 >= DST_HI then return end
            local a0 = (tonumber(r.GPR.n.a0) or 0) % 0x100000000
            local a1 = (tonumber(r.GPR.n.a1) or 0) % 0x100000000
            local ra = (tonumber(r.GPR.n.ra) or 0) % 0x100000000
            n = n + 1
            csv:row("%d,0x%08X,0x%08X,0x%08X,0x%08X", n, a0, a1, a2, ra)
            PCSX.log(string.format("[dec] #%d len=0x%X src=0x%08X dst=0x%08X ra=0x%08X",
                n, a0, a1, a2, ra))
        end)
        return { { addr = LZS, name = "lzs" } }
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== clut-decode-args probe: %d CLUT-region decodes ===", n))
    end,
})
