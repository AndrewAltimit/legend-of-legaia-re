-- autorun_hub_cadence.lua
--
-- Poll-only (no breakpoints, runs under --fast): the Muscle Dome hub's
-- cadence, one row per vsync from a parked hub state. Columns: the arm byte
-- DAT_801D1A78, the backdrop level 0x801D1A7C, the intro level 0x801D1A80,
-- the card counter 0x801D1A84, the card hold 0x801D1A70, the frame step
-- DAT_1F800393 and the frame-step floor _DAT_8007B9D8 the screen routine
-- FUN_801CF870 stores on every call (0x801CF8DC..0x801CF8E0).
--
--   bash scripts/pcsx-redux/run_probe.sh --headless --fast \
--     --scenario minigame_muscle_dome_retail_welcome --frames 500 \
--     --lua scripts/pcsx-redux/autorun_hub_cadence.lua --out-dir <dir>
--
-- Output: <dir>/hub.csv.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 400)
local csv
probe.run({
    sstate = SSTATE, capture_frames = FRAMES,
    on_arm = function(ctx)
        csv = probe.csv_open(probe.out_path("hub.csv"),
            "vsync,arm,lvl7c,lvl80,c84,c70,step,floor")
        return {}
    end,
    on_capture = function(_, elapsed)
        csv:row("%d,%d,%d,%d,%d,%d,%d,%d",
            elapsed,
            probe.read_u8(0x801D1A78),
            probe.read_u32(0x801D1A7C),
            probe.read_u32(0x801D1A80),
            probe.read_u32(0x801D1A84),
            probe.read_u32(0x801D1A70),
            probe.read_scratch_u8(0x1F800393),
            probe.read_u32(0x8007B9D8))
    end,
    on_done = function() end,
})
