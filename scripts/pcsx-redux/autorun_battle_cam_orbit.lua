-- autorun_battle_cam_orbit.lua
--
-- Per-vsync battle-camera orbit trace: the yaw global `_DAT_8007B792`
-- sampled every vsync beside the two retail writers that decrement it,
-- so the idle-orbit RATE at a given battle state is a measurement rather
-- than a reading of one writer's disassembly.
--
-- The two writers (both `yaw -= DAT_1F800393 * 2`, masked to 12 bits):
--   0x801E2A6C  action SM `FUN_801E295C` prologue, gated on ctx[7] in {0x00, 0x0B}
--   0x801D07CC  battle tick `FUN_801D0748` prologue, gated on ctx[+6] in
--               {0x1E, 0x32, 0x6E, 0xFE} (the command-flow byte)
-- At the Begin/Run prompt both gates are open (ctx[7] == 0x00, ctx[+6] ==
-- 0x1E), so the per-step rate there is the SUM of whatever each writer
-- contributes per vsync - which is what this probe counts.
--
-- Columns: vsync, yaw, pitch, tr_z (0x800840C0), ctx6, ctx7, frame step
-- (scratchpad 0x1F800393), sm_hits / disp_hits = how many times each
-- writer's store executed since the previous row, game mode.
--
-- Run (a state parked at a Begin/Run prompt, e.g. scenario
-- `battle_gaza2_prompt` by fingerprint from saves/library/pcsx-redux/):
--   LEGAIA_SSTATE=saves/library/pcsx-redux/<fp>.sstate \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_battle_cam_orbit.lua \
--   LEGAIA_OUT=/tmp/battle_cam_orbit.csv LEGAIA_FRAMES=240 \
--       bash scripts/pcsx-redux/run_probe.sh

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local mem   = require("probe.mem")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/.config/pcsx-redux/SCUS94254.sstate9")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 240)
local OUT_PATH    = probe.out_path("battle_cam_orbit.csv")

local YAW_ADDR   = 0x8007B792
local PITCH_ADDR = 0x8007B790
local TRZ_ADDR   = 0x800840C0
local CTX_PTR    = 0x8007BD24
local MODE_ADDR  = 0x8007B83C
local STEP_ADDR  = 0x1F800393

local WRITERS = {
    { addr = 0x801E2A6C, name = "sm_orbit_store" },
    { addr = 0x801D07CC, name = "disp_orbit_store" },
}

local csv = probe.csv_open(OUT_PATH,
    "vsync,yaw,pitch,tr_z,ctx6,ctx7,step,sm_hits,disp_hits,mode")

local hits = { sm_orbit_store = 0, disp_orbit_store = 0 }

probe.run({
    sstate         = SSTATE_PATH,
    capture_frames = FRAMES,
    out_path       = OUT_PATH,
    snapshot_path  = OUT_PATH:gsub("%.csv$", ".hits.txt"),

    on_arm = function()
        local descs = {}
        for _, w in ipairs(WRITERS) do
            local d = { addr = w.addr, name = w.name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(w.addr, "Exec", 4, w.name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                hits[w.name] = hits[w.name] + 1
            end)
            descs[#descs + 1] = d
        end
        PCSX.log(string.format("[camorbit] %d yaw-writer Exec probes armed", #descs))
        return descs
    end,

    on_capture = function(_, vsync_in_capture)
        local c = probe.read_u32(CTX_PTR)
        local ctx6, ctx7 = 0, 0
        if c >= 0x80000000 and c < 0x80200000 then
            ctx6 = probe.read_u8(c + 6)
            ctx7 = probe.read_u8(c + 7)
        end
        csv:row("%d,%d,%d,%d,0x%02X,0x%02X,%d,%d,%d,0x%02X",
            vsync_in_capture,
            probe.read_u16(YAW_ADDR),
            probe.read_u16(PITCH_ADDR),
            probe.read_u32(TRZ_ADDR),
            ctx6, ctx7,
            mem.read_scratch_u8(STEP_ADDR),
            hits.sm_orbit_store, hits.disp_orbit_store,
            probe.read_u8(MODE_ADDR))
        hits.sm_orbit_store = 0
        hits.disp_orbit_store = 0
    end,

    on_done = function()
        csv:close()
        PCSX.log("[camorbit] CSV closed: " .. OUT_PATH)
    end,
})
