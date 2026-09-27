-- autorun_dialog_picker_open.lua
--
-- Retail trace of the field pager (`FUN_801D84D0`, PROT 0897) around a
-- page that ends on a picker open byte (`0x27` / `0x28` / `0x29` / `0x2A`),
-- from a state whose conversation is already up
-- (e.g. `retock_innkeeper_talk_open`). The question: does the picker open as
-- soon as the prompt's page is shown, or only on the confirm press the page
-- waits for in state `0x19` (`0x801D8FD8..0x801D909C`)? And the automatic
-- press: while `_DAT_80073F00 > 0`, state `0x19` counts it down by the frame
-- step and presses for the player when it reaches zero (`0x801D8F4C..
-- 0x801D8F88`).
--
-- Per vsync, one CSV row whenever something changes:
--   v, dt, state (_DAT_801F2734), auto (_DAT_80073F00), cursor
--   (*(_DAT_801C6EA4) + 0xC), press
--
-- Press schedule: CROSS is tapped (held 4 vsyncs) LEGAIA_PICKER_WAIT vsyncs
-- after the pager first reaches state 0x19, up to LEGAIA_PICKER_PRESSES
-- times. LEGAIA_PICKER_AUTO=N instead pokes _DAT_80073F00 = N on the first
-- 0x19 vsync and presses nothing, to watch the automatic press.
--
-- Vsync-driven, no breakpoints: run with --fast. Output <OUT_DIR>/picker.csv.
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local pad = require("probe.pad")

local SSTATE = probe.getenv("LEGAIA_SSTATE",
    os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 400)
local OUT_DIR = probe.getenv("LEGAIA_OUT_DIR", "captures/dialog_picker_open")
local WAIT = probe.getenv_num("LEGAIA_PICKER_WAIT", 30)
local PRESSES = probe.getenv_num("LEGAIA_PICKER_PRESSES", 1)
local AUTO = probe.getenv_num("LEGAIA_PICKER_AUTO", 0)
os.execute(string.format("mkdir -p %q", OUT_DIR))
local CSV = io.open(OUT_DIR .. "/picker.csv", "w")
CSV:write("v,dt,state,auto,cursor,press\n")

local function s16(v) v = v or 0; if v >= 0x8000 then return v - 0x10000 end; return v end

local first19, pressed, release_v, held, last = nil, 0, nil, false, nil
probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function() return {} end,
    on_capture = function(_ctx, v)
        local st = probe.read_u32(0x801F2734) or 0
        local dt = probe.read_scratch_u8(0x1F800393)
        local ctx = probe.read_u32(0x801C6EA4) or 0
        local cur = 0
        if ctx >= 0x80000000 and ctx < 0x80200000 then cur = probe.read_u32(ctx + 0xC) or 0 end
        if st == 0x19 and first19 == nil then
            first19 = v
            if AUTO > 0 then
                probe.write_u16(0x80073F00, AUTO)
            end
        end
        if release_v and v >= release_v then pad.release(pad.BTN.CROSS); held = false; release_v = nil end
        if AUTO == 0 and first19 and pressed < PRESSES and not held
            and st == 0x19 and v - first19 >= WAIT then
            pad.force(pad.BTN.CROSS); held = true; release_v = v + 4; pressed = pressed + 1
            first19 = v -- a later 0x19 waits WAIT again
        end
        local auto = s16(probe.read_u16(0x80073F00))
        local key = string.format("%d,%X,%d,%d,%d", dt, st, auto, cur, held and 1 or 0)
        if key ~= last then
            CSV:write(string.format("%d,%d,%X,%d,%d,%d\n", v, dt, st, auto, cur, held and 1 or 0))
            CSV:flush()
            last = key
        end
    end,
    on_done = function()
        pad.release(pad.BTN.CROSS)
        CSV:close()
        PCSX.log("=== dialog_picker_open done ===")
    end,
})
