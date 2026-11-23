-- autorun_seek_ahead_hold.lua
--
-- How long a field-VM op 0x36 seek-ahead (sel == 0) holds its script on
-- retail, and how far its voice one-shot lands after it. Pure retail play:
-- the pad is the only input (LEGAIA_MASH_EVERY > 0 pulses Cross every that
-- many vsyncs, as the story-beat recordings do).
--
-- Logged per vsync of the capture (LEGAIA_OUT, CSV):
--   seek    - each pass of the op's `FUN_80019794` answer (0x801E0438): v0
--             (1 = hold), the op's PC (s8) and context (s5), the starter's
--             stage word 0x8007B9C8 and busy byte 0x8007B648
--   oneshot - each `FUN_8003D53C(clip, channel, dur)` call
--   drive   - each pass of `36 FF FF`'s drive-word test (0x801E02FC): the
--             word 0x8007BC20 (non-zero holds the op) and the op's PC
--   word    - each change of the drive word 0x8007BC20 (vsync, new value)
--   pc      - with LEGAIA_TRACE_CTX set, each new PC the field VM
--             (FUN_801DE840) is entered on for that context
--   cdsync  - each answer of the stage-3 settle wait `FUN_8003F2B8(1)`
--             (0x8001985C; non-zero holds the stage)
--
-- Env: LEGAIA_SSTATE, LEGAIA_OUT, LEGAIA_FRAMES, LEGAIA_MASH_EVERY (20).
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local out = probe.getenv("LEGAIA_OUT", "/tmp/seek_ahead_hold.csv")
local every = probe.getenv_num("LEGAIA_MASH_EVERY", 20)
local csv = probe.csv_open(out, "vsync,kind,a,b,c,d,e")
local vsync = 0
local word_was = nil
local trace_ctx = tonumber(probe.getenv("LEGAIA_TRACE_CTX", "0"))
local pc_was = nil

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE", ""),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 1500),
    on_arm = function()
        local descs = {}
        local function arm(addr, name, fn)
            local d = { addr = addr, name = name, hits_ref = { n = 0 } }
            probe.arm_breakpoint(addr, "Exec", 4, name, function()
                d.hits_ref.n = d.hits_ref.n + 1
                fn(PCSX.getRegisters().GPR.n)
            end)
            descs[#descs + 1] = d
        end
        arm(0x801E0438, "seek", function(r)
            csv:row("%d,seek,%d,0x%X,0x%08X,%d,0x%X", vsync, tonumber(r.v0),
                tonumber(r.s8), tonumber(r.s5),
                probe.read_u32(0x8007B9C8), probe.read_u8(0x8007B648))
        end)
        arm(0x8003D53C, "oneshot", function(r)
            csv:row("%d,oneshot,0x%X,%d,0x%X,0x%08X,0", vsync, tonumber(r.a0),
                tonumber(r.a1), tonumber(r.a2), tonumber(r.ra))
        end)
        arm(0x801E02FC, "drive", function(r)
            csv:row("%d,drive,%d,0x%X,0x%08X,0,0", vsync, tonumber(r.v0),
                tonumber(r.s8), tonumber(r.s5))
        end)
        if trace_ctx ~= 0 then
            arm(0x801DE840, "pc", function(r)
                if tonumber(r.a2) == trace_ctx and tonumber(r.a1) ~= pc_was then
                    pc_was = tonumber(r.a1)
                    csv:row("%d,pc,0x%X,0,0,0,0", vsync, pc_was)
                end
            end)
        end
        arm(0x8001985C, "cdsync", function(r)
            csv:row("%d,cdsync,%d,0,0,0,0", vsync, tonumber(r.v0))
        end)
        return descs
    end,
    on_capture = function(_, elapsed)
        vsync = elapsed
        local w = probe.read_u32(0x8007BC20)
        if w ~= word_was then
            csv:row("%d,word,%d,0,0,0,0", vsync, w)
            word_was = w
        end
        if every > 0 then
            if elapsed % every == 0 then
                probe.pad_force(probe.BTN.CROSS)
            elseif elapsed % every == 2 then
                probe.pad_release(probe.BTN.CROSS)
            end
        end
    end,
    on_done = function() csv:close() end,
})
