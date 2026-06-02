-- autorun_house_door_writer.lua
--
-- Find the TRUE intra-town door reposition writer using the new
-- probe.step.find_writer primitive. The earlier width-2 watch on player+0x14
-- only caught FUN_801d1878's 2-byte no-op re-store; the real teleport store is
-- a wider/offset write into the position struct. A Write breakpoint covering
-- the whole position block [player+0x10, +0x20) catches it with the correct
-- faulting PC + live registers. Hold Up to enter Mei's house; the record whose
-- bytes show X jump to the interior (0x30C0) is the door writer.
--
-- Run:
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario mei_house_door_pcsx \
--     --lua scripts/pcsx-redux/autorun_house_door_writer.lua --frames 300

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local PLAYER_PTR_ADDR = 0x8007C364
local OUT = probe.out_path("house_door_writer.txt")

local g = 0
local armed = false
local handle = nil
local f = assert(io.open(OUT, "w"))

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 150),
    snapshot_path = OUT:gsub("%.txt$", ".hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g = elapsed
        if not armed then
            if elapsed >= 2 then
                local player = probe.read_u32(PLAYER_PTR_ADDR)
                f:write(string.format("player_ptr=0x%08X\n", player or 0))
                -- Watch the whole position block; write EACH store to the file
                -- immediately (+flush) so the data survives even if the window
                -- is closed before on_done (PCSX.quit isn't reliably ending the
                -- run in this setup). Only log distinct (pc, bytes) per frame.
                local seen = {}
                handle = probe.step.find_writer(player + 0x10, 0x10, {
                    read_len = 0x10,
                    on_write = function(rg)
                        local key = string.format("%d:%08X:%s", g, rg.pc, rg.note)
                        if seen[key] then return end
                        seen[key] = true
                        f:write(string.format("f=%-4d pc=0x%08X %s s1=%08X s0=%08X v0=%08X a1=%08X a2=%08X\n",
                            g, rg.pc, rg.note, rg.s1, rg.s0, rg.v0, rg.a1, rg.a2))
                        f:flush()
                    end,
                })
                f:write("armed find_writer on player[+0x10..+0x20]; holding UP\n")
                f:flush()
                armed = true
            end
            return
        end
        if elapsed == 3 then probe.pad_force(probe.BTN.UP) end
        if elapsed >= 140 then probe.pad_release(probe.BTN.UP); ctx.request_quit = true end
    end,
    on_done = function()
        probe.pad_release(probe.BTN.UP)
        if handle then handle:dump(OUT:gsub("%.txt$", ".records.txt")) end
        f:write(string.format("done; %d position-block writes recorded\n", handle and handle:count() or 0))
        f:close()
    end,
})
