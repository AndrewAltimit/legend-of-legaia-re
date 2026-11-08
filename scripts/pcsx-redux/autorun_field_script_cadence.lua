-- autorun_field_script_cadence.lua
--
-- How often retail runs the field script: counts, per vsync, the entries into
-- the field-VM dispatcher `FUN_801DE840` (one per executed op), the distinct
-- script contexts it ran on (`a2`, the context pointer the prologue keeps in
-- `s5`), the per-actor field tick `FUN_8003BC08` and the actor-list walk
-- `FUN_8002519C`, beside the frame step `*(0x1F800393)`.
--
-- The question it settles: does a script context step once per **game
-- frame** (the vsyncs the actor walk runs on) or once per **vsync**? A
-- vsync with dispatcher entries and no actor walk would be the second.
--
--   timeout --kill-after=30s 900s bash scripts/pcsx-redux/run_probe.sh \
--       --headless --lua scripts/pcsx-redux/autorun_field_script_cadence.lua \
--       --scenario bubu2_field_door_hop --frames 600
--
-- Output: <out>/field_script_cadence.csv, one row per vsync:
--   vsync, step393, mode, de840 (entries), ctxs (distinct a2), bc08, walk519c,
--   ctx_list (the distinct a2 pointers, `;`-joined, low 24 bits).
-- Pure RAM / register observation - integers only.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 600)

local FIELD_VM = 0x801DE840
local FIELD_TICK = 0x8003BC08
local ACTOR_WALK = 0x8002519C

local n_vm, n_tick, n_walk = 0, 0, 0
local ctxs, ctx_order = {}, {}
local totals = { vm = 0, tick = 0, walk = 0 }

local out = probe.out_path("field_script_cadence.csv")
local CSV = io.open(out, "w")
CSV:write("vsync,step393,mode,de840,ctxs,bc08,walk519c,ctx_list\n")

probe.run({
    sstate = SSTATE_PATH,
    capture_frames = FRAMES,
    on_arm = function()
        local descs = {}
        local function arm(addr, name, cb)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Exec", 4, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                cb()
            end)
            descs[#descs + 1] = d
        end
        arm(FIELD_VM, "FUN_801DE840", function()
            n_vm = n_vm + 1
            local r = PCSX.getRegisters()
            local a2 = bit.band(tonumber(r.GPR.n.a2), 0xFFFFFF)
            if not ctxs[a2] then
                ctxs[a2] = true
                ctx_order[#ctx_order + 1] = a2
            end
        end)
        arm(FIELD_TICK, "FUN_8003BC08", function() n_tick = n_tick + 1 end)
        arm(ACTOR_WALK, "FUN_8002519C", function() n_walk = n_walk + 1 end)
        return descs
    end,
    on_capture = function(ctx, e)
        local list = {}
        for _, a in ipairs(ctx_order) do list[#list + 1] = string.format("%06X", a) end
        CSV:write(string.format("%d,%d,%d,%d,%d,%d,%d,%s\n",
            e, probe.mem.read_scratch_u8(0x1F800393) or -1,
            probe.mem.read_u16(0x8007B83C) or -1,
            n_vm, #ctx_order, n_tick, n_walk, table.concat(list, ";")))
        totals.vm = totals.vm + n_vm
        totals.tick = totals.tick + n_tick
        totals.walk = totals.walk + n_walk
        n_vm, n_tick, n_walk = 0, 0, 0
        ctxs, ctx_order = {}, {}
        if e % 100 == 0 then CSV:flush() end
    end,
    on_done = function()
        CSV:close()
        PCSX.log(string.format("[field_script_cadence] vm=%d tick=%d walk=%d -> %s",
            totals.vm, totals.tick, totals.walk, out))
    end,
})
