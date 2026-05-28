-- autorun_clut_uploader_pc.lua
--
-- Finds the VRAM CLUT-upload routine by watching READS of Vahn's known
-- battle CLUT source. Vahn's row-490 palette lives in the resident
-- field-scene buffer at 0x800E96A0 (pinned across every save). Whatever
-- reads it to DMA it into VRAM (0,490) is the upload routine — and the
-- same routine uploads Noa/Gala (rows 492/494) from their transient
-- buffers. This probe arms a READ breakpoint on 0x800E96A0 and logs the
-- reading PC + caller RA + GPRs each fire, so the uploader can be hooked
-- next to dump the Noa/Gala source pointers.
--
-- Run from a field state and drive a scene transition (which re-uploads
-- the party CLUTs); LEGAIA_HOLD walks into the transition:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate4 \
--   LEGAIA_FRAMES=2400 LEGAIA_HOLD=DOWN LEGAIA_HOLD_FRAMES=1800 \
--   LEGAIA_WATCH=0x800E96A0 \
--   LEGAIA_OUT_DIR=/tmp/clutprobe/uploaderpc \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_clut_uploader_pc.lua \
--       timeout --kill-after=30s 700s bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad   = require("probe.pad")

local SSTATE    = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate4")
local FRAMES    = probe.getenv_num("LEGAIA_FRAMES", 2400)
local HOLD_NAME = probe.getenv("LEGAIA_HOLD", "DOWN")
local HOLD_FR   = probe.getenv_num("LEGAIA_HOLD_FRAMES", 1800)
local OUT_DIR   = probe.getenv("LEGAIA_OUT_DIR", "/tmp/clutprobe/uploaderpc")
local WATCH     = probe.getenv_num("LEGAIA_WATCH", 0x800E96A0)

os.execute(string.format("mkdir -p %q", OUT_DIR))
local HOLD_BTN = pad.BTN[HOLD_NAME] or pad.BTN.DOWN

local csv = probe.csv_open(OUT_DIR .. "/uploader_pc.csv",
    "tick,pc,ra,a0,a1,a2,a3,t0,t1")
local seen = {}
local hits = 0
local tick = 0

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    hold_button    = HOLD_BTN,
    hold_frames    = HOLD_FR,
    out_path       = OUT_DIR .. "/uploader_pc.csv",

    on_arm = function()
        -- Read watchpoint on Vahn's CLUT source. Width 4; the uploader
        -- streams the 512-byte CLUT so any 4-byte read in it fires.
        probe.arm_breakpoint(WATCH, "Read", 4, "vahn_clut_read", function()
            local r = PCSX.getRegisters()
            local pc = bit.band(tonumber(r.GPR.n.ra) or 0, 0xFFFFFFFF)
            local realpc = bit.band(tonumber(r.pc) or 0, 0xFFFFFFFF)
            local key = realpc
            hits = hits + 1
            if seen[key] then return end       -- one row per distinct PC
            seen[key] = true
            csv:row("%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
                tick, realpc, pc,
                bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.a3) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.t0) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.t1) or 0, 0xFFFFFFFF))
            PCSX.log(string.format(
                "[uploader] read of 0x%08X by pc=0x%08X ra=0x%08X a0=0x%08X a1=0x%08X a2=0x%08X",
                WATCH, realpc, pc,
                bit.band(tonumber(r.GPR.n.a0) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.a1) or 0, 0xFFFFFFFF),
                bit.band(tonumber(r.GPR.n.a2) or 0, 0xFFFFFFFF)))
        end)
        return {}
    end,

    on_capture = function(_ctx, elapsed) tick = elapsed end,

    on_done = function()
        csv:close()
        PCSX.log(string.format("=== clut_uploader_pc: %d read hit(s), %d distinct PC(s) ===",
            hits, (function() local n = 0; for _ in pairs(seen) do n = n + 1 end; return n end)()))
    end,
})
