-- autorun_fight_pace.lua
--
-- A whole fight's pace from a save state: tap CROSS through every prompt (so
-- each member attacks), walk the field until an encounter if the state stands
-- outside a battle, and log one row per vsync. The frame counter and the step
-- byte give vsyncs per pass; the ring's newest entry gives the frame's cost.
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT_DIR, LEGAIA_FRAMES (default 9000).
-- Writes pace.csv: vsync,frame,step,mode,st7,acting,pc,mc,cost,hp0,hp1,hp2
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local GAME_MODE = 0x8007b83c
local CTX = 0x8007bd24
local FRAME = 0x8007b798
local CURSOR = 0x8007b758
local RING = 0x80084098

local csv = probe.csv_open(probe.out_path("pace.csv"),
    "vsync,frame,step,mode,st7,acting,pc,mc,cost,hp0,hp1,hp2")
local left_battle = nil

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 9000),
    on_arm = function(_) return {} end,
    on_capture = function(_, vsync)
        local mode = probe.read_u8(GAME_MODE) or 0
        local ctx = probe.read_u32(CTX) or 0
        local in_battle = mode == 0x15 and ctx >= 0x80000000 and ctx < 0x80200000
        local st7, acting, pc, mc = 0, 0, 0, 0
        if in_battle then
            pc = probe.read_u8(ctx) or 0
            mc = probe.read_u8(ctx + 1) or 0
            st7 = probe.read_u8(ctx + 7) or 0
            acting = probe.read_u8(ctx + 0x13) or 0
        end
        local cur = bit.band((probe.read_u32(CURSOR) or 1) - 1, 15)
        local hp = {}
        for i = 0, 2 do
            local a = in_battle and probe.read_u32(0x801c9370 + i * 4) or 0
            hp[i] = (a and a >= 0x80000000 and a < 0x80200000) and (probe.read_u16(a + 0x14c) or 0) or 0
        end
        csv:row("%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d,%d", vsync,
            probe.read_u32(FRAME) or 0, probe.read_scratch_u8(0x1F800393) or 0, mode, st7, acting,
            pc, mc, probe.read_u16(RING + cur * 2) or 0, hp[0], hp[1], hp[2])
        -- The pad: CROSS every other eight vsyncs; off a battle, walk.
        probe.pad_release(probe.BTN.CROSS)
        probe.pad_release(probe.BTN.LEFT)
        probe.pad_release(probe.BTN.RIGHT)
        if vsync % 16 < 4 then probe.pad_force(probe.BTN.CROSS) end
        if not in_battle then
            probe.pad_force(vsync % 240 < 120 and probe.BTN.LEFT or probe.BTN.RIGHT)
        end
    end,
})
