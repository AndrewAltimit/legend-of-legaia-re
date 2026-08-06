-- autorun_battle_item_target_capture.lua
--
-- Pad-walk a battle command-input save state through the ITEM window into
-- the item TARGET panel (menu-SM flow byte 0x64) and capture it. Sibling
-- of autorun_battle_item_window_capture.lua, one step deeper: the battle
-- menu SM (FUN_801D0748, flow byte ctx[+0x06] via _DAT_8007BD24) walks
-- 0x1E (Begin|Run) --LEFT--> 0x28 (command ring) --UP--> 0x3C (item
-- window) --CROSS--> 0x64 (item target confirm - the state whose panel
-- the engine's battle_item_ui target column reproduces). The probe:
--   1. saves battle_item_target.sstate      (target panel open, cursor on
--      the acting member - retail seeds ctx[+0x13] into +0x1DD)
--   2. presses RIGHT once (state 0x64 steps on the horizontal masks
--      0x2000/0x8000, not up/down), saves battle_item_target_cursor1.sstate
--   3. takes a screenshot at each save point (.raw + .raw.meta)
--
-- The saved states feed scripts/mednafen/display-list.py /
-- scripts/mednafen/widget-draw-sweep.py, whose OT walk pins the target
-- panel's chrome for engine-ui's battle_item_ui target column.
--
-- If the item under the cursor is unusable (CROSS buzzes and the flow
-- byte stays 0x3C), the walker steps DOWN one row after every three
-- failed presses and retries.
--
-- Poll-only (no breakpoints) - safe to run --fast.
--
-- Scenario: any battle state parked at flow 0x1E or 0x28, e.g.
--   timeout 600 bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_battle_item_target_capture.lua \
--     --scenario cort_evolved_battle_first_menu --frames 1400

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local sstate = require("probe.sstate")

local CTX_PTR = 0x8007BD24

local function flow_byte()
    local c = probe.read_u32(CTX_PTR)
    if c == nil or c < 0x80000000 or c >= 0x80200000 then return nil end
    return probe.read_u8(c + 6)
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 228,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
end

-- Walk plan: press `button` until the flow byte reads `want`, then advance.
local PLAN = {
    { name = "begin",  button = probe.BTN.LEFT,  want = 0x28, settle = 20 },
    { name = "item",   button = probe.BTN.UP,    want = 0x3C, settle = 45 },
    { name = "target", button = probe.BTN.CROSS, want = 0x64, settle = 45 },
}

local step_i = 1
local press_at = nil
local settled_at = nil
local last_flow = nil
local save_stage = 0
local move_at = nil
local fails = 0            -- failed presses on the current step

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 1400),
    on_arm = function() return {} end,
    on_capture = function(ctx, el)
        local f = flow_byte()
        if f ~= last_flow then
            PCSX.log(string.format("[itemtgt] f=%d flow=%s -> %s",
                el, tostring(last_flow), tostring(f)))
            last_flow = f
        end

        -- Release any held button after 4 vsyncs.
        if press_at ~= nil and el >= press_at + 4 then
            probe.pad_release(PLAN[step_i] and PLAN[step_i].button
                or probe.BTN.RIGHT)
            probe.pad_release(probe.BTN.DOWN)
            press_at = nil
        end

        if step_i <= #PLAN then
            local step = PLAN[step_i]
            if f == step.want then
                if settled_at == nil then settled_at = el end
                if el >= settled_at + step.settle then
                    PCSX.log(string.format("[itemtgt] step '%s' done at f=%d",
                        step.name, el))
                    step_i = step_i + 1
                    settled_at = nil
                    fails = 0
                end
            elseif press_at == nil and el >= 30 and (el % 45) == 0 then
                fails = fails + 1
                if step.name == "target" and (fails % 4) == 0 then
                    -- Item under the cursor may be unusable: step down a
                    -- row, then keep pressing CROSS.
                    probe.pad_force(probe.BTN.DOWN)
                    PCSX.log(string.format(
                        "[itemtgt] f=%d CROSS not taken - DOWN a row", el))
                else
                    probe.pad_force(step.button)
                    PCSX.log(string.format(
                        "[itemtgt] f=%d press btn %d (step %s)",
                        el, step.button, step.name))
                end
                press_at = el
            end
            return
        end

        -- Target panel open + settled.
        if save_stage == 0 then
            sstate.save(probe.out_path("battle_item_target.sstate"))
            screenshot("battle_item_target")
            PCSX.log(string.format("[itemtgt] saved target state at f=%d "
                .. "(flow=0x%02X)", el, f or 0xFF))
            save_stage = 1
            probe.pad_force(probe.BTN.RIGHT)
            move_at = el
        elseif save_stage == 1 then
            if move_at ~= nil and el >= move_at + 4 then
                probe.pad_release(probe.BTN.RIGHT)
            end
            if move_at ~= nil and el >= move_at + 30 then
                sstate.save(probe.out_path("battle_item_target_cursor1.sstate"))
                screenshot("battle_item_target_cursor1")
                PCSX.log(string.format("[itemtgt] saved cursor1 state at f=%d",
                    el))
                save_stage = 2
                ctx.request_quit = true
            end
        end
    end,
    on_done = function()
        PCSX.log(string.format("[itemtgt] done: step_i=%d save_stage=%d "
            .. "flow=%s", step_i, save_stage, tostring(flow_byte())))
    end,
})
