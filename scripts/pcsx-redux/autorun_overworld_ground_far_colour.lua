-- autorun_overworld_ground_far_colour.lua
--
-- Pins the GTE far colour (FC, control regs 21-23 = RFC/GFC/BFC) and the
-- colour word the overworld walk-view ground emitter `FUN_801F89B8`
-- (PROT 0901) depth-cues with. The emitter is a leaf with no `ctc2`, so the
-- FC its `DPCS` at 0x801F8DB0 consumes is whatever its caller left in the
-- control file. Its caller `FUN_801F69D8` calls `SetFarColor`
-- (`FUN_8005B7D8`) at 0x801F729C with a literal argument just before the
-- `jal 0x801F89B8` at 0x801F733C; this probe confirms that by construction.
--
-- Taps (all Exec):
--   * 0x801F729C - the SetFarColor call site in FUN_801F69D8 (a0/a1/a2)
--   * 0x801F89B8 - ground emitter entry: FC, scratch RGBC word 0x1F800398,
--                  the staging words 0x8007BB48 (gp+0x830) / 0x8007B7B0
--   * 0x801F8DB4 - the instruction after the DPCS: FC, RGBC (CP2D 6),
--                  IR0 (CP2D 8), SZ1 (CP2D 17), RGB2 (CP2D 22, the output)
--   * 0x801F7254 - a decoration-cell FUN_80043390 call (a1 = its far colour)
--
-- Run from the repo root against a world-map walk state:
--
--   LEGAIA_OUT=captures/w4-d/ground_fc_karisto.csv LEGAIA_FRAMES=120 \
--       timeout --kill-after=10s 300 \
--       bash scripts/pcsx-redux/run_probe.sh --isolate-config \
--       --lua scripts/pcsx-redux/autorun_overworld_ground_far_colour.lua \
--       --scenario karisto_overworld_resident

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 120)
local OUT_PATH = probe.out_path("ground_fc.csv")
local MAX_DPCS = probe.getenv_num("LEGAIA_MAX_DPCS", 400)
local SSTATE   = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")

local SET_FC_SITE = 0x801F729C
local GROUND      = 0x801F89B8
local AFTER_DPCS  = 0x801F8DB4
local DECO_CALL   = 0x801F7254
local RGBC_SCRATCH = 0x1F800398
local FAR_STAGE   = 0x8007BB48
local BASE_COLOUR = 0x8007B7B0

local csv = probe.csv_open(OUT_PATH,
    "site,hit,a0,a1,a2,rfc,gfc,bfc,rgbc,ir0,sz1,rgb2,scratch_398,stage_bb48,base_b7b0")

local hits = { setfc = 0, entry = 0, dpcs = 0, deco = 0 }

local function n32(v) return bit.band(v, 0xFFFFFFFF) end

local function row(site, key)
    hits[key] = hits[key] + 1
    if key == "dpcs" and hits[key] > MAX_DPCS then return end
    if key == "deco" and hits[key] > 64 then return end
    if (key == "entry" or key == "setfc") and hits[key] > 32 then return end
    local r = PCSX.getRegisters()
    local g = r.GPR.n
    csv:row("%s,%d,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
        site, hits[key], n32(g.a0), n32(g.a1), n32(g.a2),
        n32(r.CP2C.r[21]), n32(r.CP2C.r[22]), n32(r.CP2C.r[23]),
        n32(r.CP2D.r[6]), n32(r.CP2D.r[8]), n32(r.CP2D.r[17]), n32(r.CP2D.r[22]),
        n32(probe.read_scratch_u32(RGBC_SCRATCH) or 0),
        n32(probe.read_u32(FAR_STAGE) or 0),
        n32(probe.read_u32(BASE_COLOUR) or 0))
end

probe.run({
    sstate         = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.arm_breakpoint(SET_FC_SITE, "Exec", 4, "set_fc",
            function() row("setfc", "setfc") end)
        probe.arm_breakpoint(GROUND, "Exec", 4, "ground_entry",
            function() row("entry", "entry") end)
        probe.arm_breakpoint(AFTER_DPCS, "Exec", 4, "ground_dpcs",
            function() row("dpcs", "dpcs") end)
        probe.arm_breakpoint(DECO_CALL, "Exec", 4, "deco_call",
            function() row("deco", "deco") end)
        PCSX.log("[ground_fc] armed")
        return {}
    end,
    on_capture = function(ctx, _elapsed)
        if hits.entry >= 16 and hits.dpcs >= MAX_DPCS then
            ctx.request_quit = true
        end
    end,
    on_done = function()
        csv:close()
        PCSX.log(string.format(
            "[ground_fc] setfc=%d entry=%d dpcs=%d deco=%d out=%s",
            hits.setfc, hits.entry, hits.dpcs, hits.deco, OUT_PATH))
    end,
})
