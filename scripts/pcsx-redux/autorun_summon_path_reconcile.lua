-- autorun_summon_path_reconcile.lua
--
-- Settle which render path animates the PLAYER Gimard "Burning Attack"
-- summon. Two competing models:
--   (A) move-VM scene-graph: the spawn-stager FUN_80021B04 seats each
--       part as an actor whose move-VM bytecode is ticked by FUN_80023070,
--       and the part mesh is oriented by the PROT 0900 builder FUN_801F7088.
--       (This is the documented ENEMY Gimard "Fire Tail" path.)
--   (B) battle per-actor draw: the summon is an ordinary battle actor
--       drawn by FUN_80048A08 -> monster-anim TRS decoder FUN_8004998C ->
--       cluster-A FUN_80043390 (same path as enemy monster bodies).
--
-- The summon_rotation probe already showed (B) fires 64x/frame and
-- FUN_801F7088 never really executes. This probe directly counts the (A)
-- entry points (FUN_80021B04 stager, FUN_80023070 move VM, FUN_801F7088
-- builder) ALONGSIDE the (B) draw FUN_80048A08, so a single run says
-- whether the move-VM path runs AT ALL during the player Burning Attack.
--
-- All four targets are SCUS-resident (static addresses), valid regardless
-- of which overlay is paged in.
--
-- Usage:
--   LEGAIA_SSTATE=$HOME/Tools/pcsx-redux/SCUS94254.sstate7 \
--   LEGAIA_LUA=scripts/pcsx-redux/autorun_summon_path_reconcile.lua \
--   LEGAIA_FRAMES=90 \
--       bash scripts/pcsx-redux/run_probe.sh
--
-- Output: summon_path_reconcile.csv (tick,fn,pc,ra,a0,a1,a2)

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

-- (fn entry, label, is-move-VM-path-A)
local TARGETS = {
    { 0x80021B04, "stager_21B04",  true  }, -- (A) part spawn-stager
    { 0x80023070, "movevm_23070",  true  }, -- (A) move-VM dispatcher
    { 0x801F7088, "builder_1F7088", true }, -- (A) PROT 0900 rotation builder
    { 0x80048A08, "battledraw_48A08", false }, -- (B) battle per-actor draw
}

local csv = probe.csv_open(probe.out_path("summon_path_reconcile.csv"),
    "tick,fn,pc,ra,a0,a1,a2")
local g_elapsed = 0
local seen = {}
local cap = 200 -- per-fn record cap (move VM can fire a lot)
for _, t in ipairs(TARGETS) do seen[t[2]] = 0 end

local function make_cb(label)
    return function()
        seen[label] = seen[label] + 1
        if seen[label] > cap then return end
        local r = PCSX.getRegisters()
        local g = r.GPR.n
        local function rr(x) return bit.band(tonumber(g[x]), 0xFFFFFFFF) end
        csv:row("%d,%s,0x%08X,0x%08X,0x%08X,0x%08X,0x%08X",
            g_elapsed, label, bit.band(tonumber(r.pc), 0xFFFFFFFF),
            rr("ra"), rr("a0"), rr("a1"), rr("a2"))
    end
end

local armed = false
local function arm()
    for _, t in ipairs(TARGETS) do
        probe.arm_breakpoint(t[1], "Exec", 4, t[2], make_cb(t[2]))
    end
    PCSX.log("[reconcile] armed Exec BPs at stager/movevm/builder/battledraw")
    armed = true
end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate7"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 90),
    snapshot_path  = probe.out_path("summon_path_reconcile.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(_, elapsed)
        g_elapsed = elapsed
        if not armed and elapsed >= 3 then arm() end
    end,
    on_done = function()
        csv:close()
        local parts = {}
        for _, t in ipairs(TARGETS) do
            parts[#parts + 1] = string.format("%s=%d", t[2], seen[t[2]])
        end
        PCSX.log("[reconcile] hit counts: " .. table.concat(parts, "  "))
    end,
})
