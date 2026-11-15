-- autorun_field_frame_profile.lua
--
-- Where a field frame's time goes: load a state, leave the pad alone, and
-- count the CPU cycles spent inside each of a list of routines, vsync by
-- vsync, beside the frame pacer's newest frame time. An exec breakpoint on
-- each routine's entry stamps the cycle counter and arms one on the return
-- address it was called from; the return stamps the difference. Times are
-- inclusive (a routine's callees count in it). The field frame-cost model's
-- missing terms are read off it (`legaia_engine_core::world::field_frame_cost`).
--
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--     --lua scripts/pcsx-redux/autorun_field_frame_profile.lua \
--     --scenario son_field_door_hop --frames 60 --out-dir <dir>
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES, LEGAIA_OUT_DIR,
--      LEGAIA_PROFILE_FUNCS  comma-separated hex entries (default below).
-- Output: frame_profile.csv (`vsync,frame_cost,step,func,calls,cycles`, one
-- row per routine per vsync it ran in). Interpreter mode required (exec BP);
-- not for --fast.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 60)
local DEFAULT = table.concat({
    "80016444", -- master frame driver
    "8002519C", -- an actor-list tick pass
    "8001D140", -- an actor-list render pass
    "8001ADA4", -- one actor's draw
    "801F7088", -- the decoration pass
    "801F6D48", -- the ground emitter
    "801F69EC", -- its _DAT_8007BB4C twin
    "801DE840", -- the field VM
    "801D6058", -- the ambient emitter
    "8003F3FC", -- the fog walk
    "8005FB84", -- libetc VSync
}, ",")
local LIST = probe.getenv("LEGAIA_PROFILE_FUNCS", DEFAULT)
local RING, RING_CURSOR = 0x80084098, 0x8007B758

local fh = assert(io.open(probe.out_path("frame_profile.csv"), "w"))
fh:write("vsync,frame_cost,step,func,calls,cycles\n")

local funcs = {}
for h in string.gmatch(LIST, "[^,]+") do funcs[#funcs + 1] = tonumber(h, 16) end
local calls, cycles, open, ret_bp = {}, {}, {}, {}

local function now() return tonumber(PCSX.getCPUCycles()) end

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_field_frame_profile.lua",
            { sstate = SSTATE, frames = FRAMES, funcs = LIST })
        for _, f in ipairs(funcs) do
            calls[f], cycles[f], open[f] = 0, 0, {}
            probe.arm_breakpoint(f, "Exec", 4, string.format("prof_%08X", f), function()
                local ra = PCSX.getRegisters().GPR.n.ra
                if ra < 0 then ra = ra + 0x100000000 end
                local st = open[f]
                st[#st + 1] = { ra = ra, t = now() }
                local key = string.format("%08X:%08X", f, ra)
                if not ret_bp[key] then
                    ret_bp[key] = true
                    probe.arm_breakpoint(ra, "Exec", 4, "ret_" .. key, function()
                        local s = open[f]
                        -- Pop the newest open call made from this site.
                        for i = #s, 1, -1 do
                            if s[i].ra == ra then
                                cycles[f] = cycles[f] + (now() - s[i].t)
                                calls[f] = calls[f] + 1
                                table.remove(s, i)
                                break
                            end
                        end
                    end)
                end
            end)
        end
        return {}
    end,
    on_capture = function(_, el)
        local cur = probe.read_u32(RING_CURSOR) or 1
        local cost = probe.read_u16(RING + ((cur - 1) % 16) * 2) or 0
        local step = probe.read_scratch_u8(0x1F800393) or 0
        for _, f in ipairs(funcs) do
            if calls[f] > 0 then
                fh:write(string.format("%d,%d,%d,%08X,%d,%.0f\n", el, cost, step, f, calls[f], cycles[f]))
            end
            calls[f], cycles[f] = 0, 0
        end
        fh:flush()
    end,
})
