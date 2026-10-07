-- autorun_house_door_live.lua
--
-- Capture LIVE registers at the teleport store (0x801D1AD4, sh v0,0x14(s3) in
-- FUN_801d1878) via an EXEC breakpoint (registers are mid-instruction, unlike a
-- write-watchpoint whose callback runs after the function restores saved regs).
-- The CSV from the trace probe showed 0x801D1AD4 is the first writer of the
-- interior X (12480) at frame 75; this reads s1/s0/v0 there so we can see the
-- displacement and trace its source. A position write-watch confirms the
-- teleport frame so we know the capture is the real one.
--
-- Run:
--   timeout --kill-after=30s 600s bash scripts/pcsx-redux/run_probe.sh \
--     --scenario mei_house_door_pcsx \
--     --lua scripts/pcsx-redux/autorun_house_door_live.lua --frames 300

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local PLAYER_PTR_ADDR = 0x8007C364
local STORE_X = 0x801D1AD4
local BDE0 = 0x8007BDE0
local csv = probe.csv_open(probe.out_path("house_door_live.csv"),
    "frame,x_mem,v0,v1,s0,s1,s2,a1,a2,bde0,bde4")
local f = assert(io.open(probe.out_path("house_door_live.txt"), "w"))

local g = 0
local player_ptr = nil
local armed = false
local teleport_frame = -1

local function s16(v) v = (v or 0) % 0x10000; if v >= 0x8000 then v = v - 0x10000 end; return v end
local function gp(r, name) return bit.band(tonumber(r.GPR.n[name]) or 0, 0xFFFFFFFF) end

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 300),
    snapshot_path = probe.out_path("house_door_live.hits.txt"),
    on_arm = function() return {} end,
    on_capture = function(ctx, elapsed)
        g = elapsed
        if not armed then
            if elapsed >= 2 then
                player_ptr = probe.read_u32(PLAYER_PTR_ADDR)
                f:write(string.format("player_ptr=0x%08X X=%d Z=%d\n", player_ptr or 0,
                    s16(probe.read_u16(player_ptr + 0x14)), s16(probe.read_u16(player_ptr + 0x18))))
                -- EXEC bp at the store: live registers.
                probe.arm_breakpoint(STORE_X, "Exec", 4, "store", function()
                    local r = PCSX.getRegisters()
                    local x_mem = s16(probe.read_u16(player_ptr + 0x14))
                    csv:row("%d,%d,0x%X,0x%X,0x%X,0x%X,0x%X,0x%X,0x%X,%d,%d",
                        g, x_mem, gp(r, "v0"), gp(r, "v1"), gp(r, "s0"), gp(r, "s1"), gp(r, "s2"),
                        gp(r, "a1"), gp(r, "a2"),
                        s16(probe.read_u16(BDE0)), s16(probe.read_u16(BDE0 + 4)))
                end)
                -- Write-watch on X to flag the teleport frame.
                probe.arm_breakpoint(player_ptr + 0x14, "Write", 2, "xw", function()
                    local x = s16(probe.read_u16(player_ptr + 0x14))
                    if x > 8000 and teleport_frame < 0 then
                        teleport_frame = g
                        f:write(string.format("TELEPORT at frame %d (X now %d)\n", g, x))
                        f:flush()
                    end
                end)
                f:write("armed; holding UP\n")
                armed = true
            end
            return
        end
        if elapsed == 3 then probe.pad_force(probe.BTN.UP) end
        if elapsed >= 290 then probe.pad_release(probe.BTN.UP); ctx.request_quit = true end
    end,
    on_done = function()
        probe.pad_release(probe.BTN.UP)
        csv:close()
        f:write(string.format("done; teleport_frame=%d final_X=%d\n", teleport_frame,
            player_ptr and s16(probe.read_u16(player_ptr + 0x14)) or -1))
        f:close()
    end,
})
