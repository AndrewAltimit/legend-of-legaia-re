-- autorun_battle_char_clut_source.lua
--
-- Pins the disc SOURCE of the battle-form party character CLUT band
-- (VRAM rows 490..497, x=0..255 — the 256-colour palettes the battle
-- character TMDs sample: Vahn 490/491, Noa 492/493, Gala 494/495,
-- aux 496/497).
--
-- Why a runtime probe (docs/reference/open-rev-eng-threads.md
-- "Battle character image + CLUT source"):
--   The PROT 1204 atlas TIMs carry the correct IMAGES but WRONG default
--   CLUTs (row-492 value match vs retail VRAM = 0/256). The correct
--   party palettes are uploaded to VRAM at battle-context entry and
--   then the staging buffer is freed — they are NOT in main RAM in any
--   captured save state (checked across 7 mednafen saves) and NOT
--   verbatim on disc except Vahn's row 490 (map01/map02 sec0). Only
--   Vahn appears because he is the lone overworld walker; Noa/Gala come
--   from a battle-entry party-load path whose source is a transient
--   decompress->DMA->free that manual save-state granularity can't
--   catch. This probe logs the disc reads live so the source PROT entry
--   can be pinned.
--
-- Strategy:
--   Load a field sstate where the band is NOT yet resident (e.g. early
--   game / a state where VRAM was cleared — the band is battle-context
--   loaded, not boot-global), hold a walk direction to trigger a random
--   encounter, and log EVERY disc read's CdlLOC -> absolute LBA ->
--   PROT.DAT byte offset over the capture window. The read that fills
--   the character CLUT staging buffer is among them; cross-reference the
--   logged LBAs against the PROT TOC offline (decompress + search for
--   the row-492 palette) to pin the entry.
--
--   PROT.DAT begins at disc LBA 242, so prot.dat_off = (lba - 242)*2048
--   (same mapping the monster-record probe validated).
--
-- Run (provide a band-absent FIELD sstate; hold DOWN ~900 frames to walk
-- into an encounter; capture 1800 frames so battle-init completes):
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate3 \
--   LEGAIA_FRAMES=1800 LEGAIA_HOLD=DOWN LEGAIA_HOLD_FRAMES=900 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_battle_char_clut_source.lua \
--       timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh
--
-- Then map the logged LBAs to PROT entries:
--   python3 scripts/pcsx-redux/map_clut_disc_reads.py \
--       <out>/battle_char_clut_source.csv

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate3")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 1800)
local HOLD_FRAMES = probe.getenv_num("LEGAIA_HOLD_FRAMES", 900)
local HOLD_NAME   = probe.getenv("LEGAIA_HOLD", "DOWN")
local OUT_PATH    = probe.out_path("battle_char_clut_source.csv")

-- Disc I/O primitives (validated by autorun_monster_record_source.lua).
local FUN_8003E800 = 0x8003E800 -- generic disc read (a0=dest, a1=sector count)
local FUN_8003E8A8 = 0x8003E8A8 -- absolute disc seek (a0=PROT index), sets CdlLOC
local FUN_8003E964 = 0x8003E964 -- relative disc seek (a0=sector delta)
local DAT_8007BC5C = 0x8007BC5C -- CdlLOC (BCD MSF, 3 bytes)
local PROT_DAT_BASE_LBA = 242   -- PROT.DAT starts here on disc

local HOLD_BTN = pad.BTN[HOLD_NAME] or pad.BTN.DOWN

-- BCD MSF -> absolute disc LBA.
local function cdloc_lba()
    local b = probe.read_bytes(DAT_8007BC5C, 3)
    if b == nil then return -1, "??:??:??" end
    local s = tostring(b)
    local function bcd(x) return math.floor(x / 16) * 10 + (x % 16) end
    local m   = bcd(string.byte(s, 1))
    local sec = bcd(string.byte(s, 2))
    local f   = bcd(string.byte(s, 3))
    local lba = (m * 60 + sec) * 75 + f - 150
    return lba, string.format("%02X:%02X:%02X",
        string.byte(s, 1), string.byte(s, 2), string.byte(s, 3))
end

local csv = probe.csv_open(OUT_PATH,
    "tick,kind,a0,sectors,cdloc,lba,prot_dat_off,dest,ra")

local tick = 0
local read_hits, seek_hits = 0, 0

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    hold_button    = HOLD_BTN,
    hold_frames    = HOLD_FRAMES,
    out_path       = OUT_PATH,

    on_arm = function()
        local bps = {}

        -- Every absolute seek: log the PROT index + resulting CdlLOC.
        table.insert(bps, { addr = FUN_8003E8A8, name = "disc_seek" })
        probe.arm_breakpoint(FUN_8003E8A8, "Exec", 4, "disc_seek", function()
            local r = PCSX.getRegisters()
            local idx = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local ra  = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local lba, msf = cdloc_lba()
            seek_hits = seek_hits + 1
            csv:row("%d,seek,0x%X,,%s,%d,0x%X,,0x%08X",
                tick, idx, msf, lba, (lba - PROT_DAT_BASE_LBA) * 2048, ra)
            PCSX.log(string.format("[clut] seek idx=0x%X (%d) CdlLOC=%s lba=%d ra=0x%08X",
                idx, idx, msf, lba, ra))
        end)

        -- Relative seek (delta sectors) — some loaders stream this way.
        table.insert(bps, { addr = FUN_8003E964, name = "rel_seek" })
        probe.arm_breakpoint(FUN_8003E964, "Exec", 4, "rel_seek", function()
            local r = PCSX.getRegisters()
            local delta = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local ra    = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local lba, msf = cdloc_lba()
            csv:row("%d,rel_seek,%d,,%s,%d,0x%X,,0x%08X",
                tick, delta, msf, lba, (lba - PROT_DAT_BASE_LBA) * 2048, ra)
        end)

        -- Every disc read: dest buffer + sector count + the LBA it reads
        -- from. The CLUT-bearing read is somewhere in this stream.
        table.insert(bps, { addr = FUN_8003E800, name = "disc_read" })
        probe.arm_breakpoint(FUN_8003E800, "Exec", 4, "disc_read", function()
            local r = PCSX.getRegisters()
            local dest  = bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF)
            local count = bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF)
            local ra    = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local lba, msf = cdloc_lba()
            read_hits = read_hits + 1
            csv:row("%d,read,,%d,%s,%d,0x%X,0x%08X,0x%08X",
                tick, count, msf, lba, (lba - PROT_DAT_BASE_LBA) * 2048, dest, ra)
            PCSX.log(string.format(
                "[clut] read #%d dest=0x%08X sectors=%d CdlLOC=%s lba=%d prot_off=0x%X ra=0x%08X",
                read_hits, dest, count, msf, lba, (lba - PROT_DAT_BASE_LBA) * 2048, ra))
        end)

        return bps
    end,

    on_capture = function(_ctx, elapsed)
        tick = elapsed
    end,

    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "=== battle-char CLUT source probe: %d read(s), %d seek(s) over %d frames ===",
            read_hits, seek_hits, FRAMES))
        PCSX.log("Map the logged LBAs to PROT entries: "
            .. "python3 scripts/pcsx-redux/map_clut_disc_reads.py " .. OUT_PATH)
    end,
})
