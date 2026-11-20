-- autorun_actor_angle_writers.lua
--
-- Who writes a field actor's X / Z Euler angles (`+0x24` pitch, `+0x28`
-- roll), and in what order inside a frame. Two writers share the words: the
-- field VM's `4C 43` / `4C 44` on the actor's context (`0x801E1234..
-- 0x801E12A0`, a write or a tween) and the scripted-motion channel's
-- ops `0x15` / `0x16`. The angle that draws is whichever wrote last before
-- the actor renderer read it; this film logs every store with its `pc` and
-- `ra` and a per-vsync row of both words, so the order is read off retail.
--
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--       --headless --lua scripts/pcsx-redux/autorun_actor_angle_writers.lua \
--       --scenario juui1_vortex_entry --frames 900
--
-- Env: LEGAIA_SSTATE, LEGAIA_FRAMES (default 900), LEGAIA_ACTOR_IDS (comma
--      list of actor `+0x50` ids; default `9,10,11`, `juui1`'s party),
--      LEGAIA_OUT_DIR.
-- Output: actor_angle_writes.csv (one row a store), actor_angles.csv (one
-- row a vsync per actor), actor_angle_writes.detail.txt (first stores' call
-- context). Pure RAM observation, no pad input.

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 900)
local IDS = {}
for tok in string.gmatch(probe.getenv("LEGAIA_ACTOR_IDS", "9,10,11"), "[^,]+") do
    IDS[tonumber(tok)] = true
end

local function u16(a) return probe.mem.read_u16(a) or 0 end
local function u32(a) return probe.mem.read_u32(a) or 0 end
local function s16(a)
    local v = u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end

-- Every live actor node on the nine list heads whose `+0x50` id is wanted.
local function scan()
    local found = {}
    local seen = {}
    for k = 0, 8 do
        local p = u32(0x8007C34C + 4 * k)
        local n = 0
        while probe.in_ram(p) and not seen[p] and n < 512 do
            seen[p] = true
            local id = u16(p + 0x50)
            if IDS[id] and found[id] == nil then
                found[id] = { node = p, tick = u32(p + 0x0C) }
            end
            p = u32(p)
            n = n + 1
        end
    end
    return found
end

local elapsed = 0
local writes = probe.csv_open(probe.out_path("actor_angle_writes.csv"),
    "tick,label,addr,pc,ra,prev_value")
local w = probe.watch.new({
    csv = writes,
    detail_path = probe.out_path("actor_angle_writes.detail.txt"),
    max_detail = 24,
    elapsed = function() return elapsed end,
})
local ANG = io.open(probe.out_path("actor_angles.csv"), "w")
ANG:write("vsync,id,node,tick,x24,x26,x28,flags\n")
local armed = {}

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_actor_angle_writers.lua",
            { sstate = SSTATE, frames = FRAMES })
        return {}
    end,
    on_capture = function(_, e)
        elapsed = e
        for id, a in pairs(scan()) do
            if not armed[a.node] then
                armed[a.node] = true
                w:arm(a.node + 0x24, 2, string.format("id%d_x24", id))
                w:arm(a.node + 0x28, 2, string.format("id%d_x28", id))
            end
            ANG:write(string.format("%d,%d,0x%08X,0x%08X,%d,%d,%d,0x%08X\n", e, id, a.node,
                a.tick, s16(a.node + 0x24), s16(a.node + 0x26), s16(a.node + 0x28),
                u32(a.node + 0x10)))
        end
        if e % 60 == 0 then ANG:flush() end
    end,
    on_done = function()
        ANG:close()
        PCSX.log(string.format("[actor_angle_writers] %d stores", w:total()))
    end,
})
