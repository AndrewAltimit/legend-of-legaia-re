-- probe_world_map_callchain.lua
--
-- Diagnostic Lua hook to figure out WHY log_world_map_vm.lua saw
-- zero dispatches at FUN_801D362C during retail world-map play.
-- Arms Exec breakpoints at several related addresses and tallies
-- hits per address, then prints the table on stop.
--
-- Each probe answers a specific question:
--
--   0x801D362C  world-map drawing VM dispatcher
--                 0 hits + others 0 = breakpoints not firing at all
--                 0 hits + move-VM nonzero = op 0x2F never dispatched
--                 nonzero = original hook should have caught it (bug)
--
--   0x80023070  move-VM entry point (FUN_80023070)
--                 nonzero = move-VM is running every actor each frame
--                 0 hits = move-VM isn't running at all in world-map mode
--
--   0x80023AE0  move-VM op 0x2F handler (the jal site to 0x801D362C)
--                 reached only when an actor's bytecode hits sub-op 0x2F
--
--   0x801E76D4  world-map controller (FUN_801E76D4)
--                 sanity check: this MUST fire during world-map play.
--                 If 0, then either the overlay isn't loaded at this
--                 address or breakpoints are broken in dynarec
--
--   0x80017ec8  GsAddPrim-equivalent dispatcher (called every frame)
--                 absolute sanity: this WILL fire if the emulator is
--                 running at all and breakpoints work
--
-- Run order:
--   1. Boot PCSX-Redux, load Legaia, get onto the world map.
--   2. In the Lua console:  dofile("scripts/pcsx-redux/probe_world_map_callchain.lua")
--   3. Walk for 10+ seconds.
--   4. In the Lua console:  worldMapProbe.stop()
--   5. Read the printed hit table.
--   6. (Optional) worldMapProbe.dump() prints again without detaching.

local PROBES = {
    { addr = 0x80017EC8, name = "AddPrim_dispatch (sanity)"   },
    { addr = 0x801E76D4, name = "world_map_controller"        },
    { addr = 0x80023070, name = "move_vm_entry"               },
    { addr = 0x80023AE0, name = "move_vm_op_0x2F"             },
    { addr = 0x801D362C, name = "world_map_draw_vm"           },
    { addr = 0x801D31B0, name = "scanline_emitter (mid-fn)"   },
    { addr = 0x801D30B8, name = "scanline_emitter_parent"     },
}

local hits = {}
for _, p in ipairs(PROBES) do hits[p.addr] = 0 end

local bps = {}

local function arm()
    for _, p in ipairs(PROBES) do
        local bp = PCSX.addBreakpoint(
            p.addr, "Exec", 4, "probe:" .. p.name,
            function() hits[p.addr] = hits[p.addr] + 1 end)
        table.insert(bps, bp)
    end
end

local function dump()
    PCSX.log("=== world-map probe hits ===")
    for _, p in ipairs(PROBES) do
        PCSX.log(string.format("  0x%08X  %8d  %s",
            p.addr, hits[p.addr], p.name))
    end
    PCSX.log("=== end ===")
end

local function stop()
    for _, bp in ipairs(bps) do bp:remove() end
    bps = {}
    dump()
end

arm()
worldMapProbe = { stop = stop, dump = dump, hits = function() return hits end }
PCSX.log(string.format(
    "[world_map_probe] %d probes armed; call worldMapProbe.stop() when done",
    #PROBES))
