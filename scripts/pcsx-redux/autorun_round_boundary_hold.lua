-- autorun_round_boundary_hold.lua
--
-- How long the Done band's closing glides hold the next round prompt
-- (docs/subsystems/battle-hud.md, "The stepper's return holds the pad").
--
-- From a battle state inside a round's action phase, log one row per battle
-- pass for each of the two state machines the frame driver FUN_80046A20 runs
-- in turn:
--   menu  at 0x801D0B34, just after the menu SM's call of the glide stepper
--         FUN_801D9BBC: the stepper's return (v0), the flow byte ctx[+0x06],
--         the action byte ctx[+0x07], the latch ctx[+0x6B2], the live glide
--         records and the largest remaining total - elapsed among them;
--   disp  at 0x801D0C84, the menu SM's state dispatch (reached only on a pass
--         the gate let through);
--   act   at 0x801E295C, the action SM's entry: ctx[+0x07] and the Done
--         band countdown ctx[+0x6D8].
-- Every row carries the frame step 0x1F800393 and the stage id 0x8007B64A.
--
-- Pad: LEGAIA_PRESS_X=1 taps Cross every 60 vsyncs until the action phase is
-- seen (a state saved on the Begin confirm), then releases the pad.
-- Stops LEGAIA_TAIL passes after the first dispatched pass at flow 0x14.
--
-- Needs the BP core (no --fast). Output passes.csv is retail-RAM-derived:
-- never commit it.
--
-- Run:
--   bash scripts/pcsx-redux/run_probe.sh --headless \
--       --scenario chitei2_kemaro_v2_pre_lava_breath \
--       --lua scripts/pcsx-redux/autorun_round_boundary_hold.lua --frames 3000
package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")

local SSTATE_PATH = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES      = probe.getenv_num("LEGAIA_FRAMES", 3000)
local PRESS_X     = probe.getenv_num("LEGAIA_PRESS_X", 0)
local TAIL        = probe.getenv_num("LEGAIA_TAIL", 8)

local CTX_PTR    = 0x8007BD24
local FRAME_STEP = 0x1F800393
local STAGE_ID   = 0x8007B64A
local MENU_AFTER_STEPPER = 0x801D0B34
local MENU_DISPATCH      = 0x801D0C84
local ACTION_SM          = 0x801E295C

local function u8(a)  return probe.read_u8(a)  or 0 end
local function u16(a) return probe.read_u16(a) or 0 end
local function u32(a) return probe.read_u32(a) or 0 end
local function s16(a)
    local v = u16(a)
    if v >= 0x8000 then v = v - 0x10000 end
    return v
end
local function ptr_ok(p) return p >= 0x80000000 and p < 0x80200000 end
local function tou32(v)
    v = tonumber(v) or 0
    if v < 0 then v = v + 0x100000000 end
    return v
end

local rows
local vsync = -1
local seen_action, done_at = false, nil
local tail_left = nil

local function glides(cx)
    local live, worst = 0, 0
    for i = 0, 0x27 do
        local at = cx + 0x11B4 + i * 0xC
        local total = u8(at)
        if total ~= 0 then
            live = live + 1
            local rem = total - u8(at + 1)
            if rem > worst then worst = rem end
        end
    end
    return live, worst
end

local function row(kind, ret)
    local cxv = u32(CTX_PTR)
    if not ptr_ok(cxv) then return end
    local live, worst = glides(cxv)
    rows:row("%d,%s,%d,%d,0x%02X,0x%02X,%d,%d,%d,%d,%d", vsync, kind, ret,
        probe.read_scratch_u8(FRAME_STEP), u8(cxv + 6), u8(cxv + 7), s16(cxv + 0x6B2),
        s16(cxv + 0x6D8), live, worst, u8(STAGE_ID))
    return cxv
end

probe.run({
    sstate = SSTATE_PATH, capture_frames = FRAMES,
    on_arm = function()
        rows = probe.csv_open(probe.out_path("passes.csv"),
            "vsync,kind,ret,step,flow,action,latch_6b2,timer_6d8,live,worst,stage")
        probe.arm_breakpoint(MENU_AFTER_STEPPER, "Exec", 4, "menu", function()
            local r = PCSX.getRegisters()
            row("menu", tou32(r.GPR.n.v0))
        end)
        probe.arm_breakpoint(MENU_DISPATCH, "Exec", 4, "disp", function()
            local cx = row("disp", -1)
            if cx and seen_action and u8(cx + 6) == 0x14 and tail_left == nil then
                tail_left = TAIL
            end
        end)
        probe.arm_breakpoint(ACTION_SM, "Exec", 4, "act", function()
            local cx = row("act", -1)
            if cx and u8(cx + 6) == 0xFF then seen_action = true end
        end)
        return {}
    end,

    on_capture = function(c, elapsed)
        vsync = elapsed
        probe.pad_release(probe.BTN.CROSS)
        if PRESS_X ~= 0 and not seen_action then
            local sub = elapsed % 60
            if sub >= 30 and sub < 34 then probe.pad_force(probe.BTN.CROSS) end
        end
        if tail_left ~= nil then
            tail_left = tail_left - 1
            if tail_left <= 0 and not done_at then
                done_at = elapsed
                c.request_quit = true
            end
        end
    end,

    on_done = function()
        if rows then rows:close() end
        PCSX.quit(0)
    end,
})
