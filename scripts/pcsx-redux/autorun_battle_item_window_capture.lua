-- autorun_battle_item_window_capture.lua
--
-- Pad-walk a battle command-input save state into the ITEM window and
-- capture it. The battle menu SM (FUN_801D0748, flow byte ctx[+0x06] via
-- _DAT_8007BD24) walks 0x1E (Begin|Run turn prompt) --LEFT--> 0x28
-- (command ring) --TRIANGLE--> 0x3C (item window). The probe presses the
-- retail buttons, waits for each flow transition, then:
--   1. saves battle_item_window.sstate      (item window open, cursor row 0)
--   2. presses DOWN once, saves battle_item_window_cursor1.sstate
--      (cursor moved - diffing the two display lists isolates the cursor prim)
--   3. takes a screenshot at each save point (.raw + .raw.meta,
--      decode with scripts/pcsx-redux/decode_pcsx_screen.py)
--
-- The saved states feed scripts/mednafen/display-list.py, whose OT walk
-- pins the item window's chrome (window prims, row pens, quantity column)
-- for docs/subsystems/battle.md + engine-ui's battle item window builder.
--
-- Poll-only (no breakpoints) - safe to run --fast.
--
-- Scenario: any battle state parked at flow 0x1E or 0x28, e.g.
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--     --lua scripts/pcsx-redux/autorun_battle_item_window_capture.lua \
--     --scenario cort_evolved_battle_first_menu

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
-- `settle` frames run after the transition before the next step acts.
local PLAN = {
    { name = "begin",    button = probe.BTN.LEFT, want = 0x28, settle = 20 },
    { name = "item",     button = probe.BTN.UP,   want = 0x3C, settle = 45 },
}

local step_i = 1
local press_at = nil       -- elapsed vsync of the current press, nil = idle
local settled_at = nil     -- elapsed vsync the current step's `want` was seen
local last_flow = nil
local save_stage = 0       -- 0 = not saved, 1 = first save done, 2 = all done
local down_at = nil

probe.run({
    sstate = probe.getenv("LEGAIA_SSTATE",
        os.getenv("HOME") .. "/Tools/pcsx-redux/SCUS94254.sstate1"),
    capture_frames = probe.getenv_num("LEGAIA_FRAMES", 900),
    on_arm = function() return {} end,
    on_capture = function(ctx, el)
        local f = flow_byte()
        if f ~= last_flow then
            PCSX.log(string.format("[itemwin] f=%d flow=%s -> %s",
                el, tostring(last_flow), tostring(f)))
            last_flow = f
        end

        -- Release any held button after 4 vsyncs.
        if press_at ~= nil and el >= press_at + 4 then
            probe.pad_release(PLAN[step_i] and PLAN[step_i].button
                or probe.BTN.DOWN)
            press_at = nil
        end

        if step_i <= #PLAN then
            local step = PLAN[step_i]
            if f == step.want then
                if settled_at == nil then settled_at = el end
                if el >= settled_at + step.settle then
                    PCSX.log(string.format("[itemwin] step '%s' done at f=%d",
                        step.name, el))
                    step_i = step_i + 1
                    settled_at = nil
                end
            elseif press_at == nil and el >= 30 and (el % 45) == 0 then
                -- (Re-)press this step's button every 45 vsyncs until the
                -- flow byte moves.
                probe.pad_force(step.button)
                press_at = el
                PCSX.log(string.format("[itemwin] f=%d press btn %d (step %s)",
                    el, step.button, step.name))
            end
            return
        end

        -- Both transitions done: item window open + settled.
        if save_stage == 0 then
            sstate.save(probe.out_path("battle_item_window.sstate"))
            screenshot("battle_item_window")
            PCSX.log(string.format("[itemwin] saved item-window state at f=%d "
                .. "(flow=0x%02X)", el, f or 0xFF))
            save_stage = 1
            probe.pad_force(probe.BTN.DOWN)
            down_at = el
        elseif save_stage == 1 then
            if down_at ~= nil and el >= down_at + 4 then
                probe.pad_release(probe.BTN.DOWN)
            end
            if down_at ~= nil and el >= down_at + 30 then
                sstate.save(probe.out_path("battle_item_window_cursor1.sstate"))
                screenshot("battle_item_window_cursor1")
                PCSX.log(string.format("[itemwin] saved cursor1 state at f=%d",
                    el))
                save_stage = 2
                ctx.request_quit = true
            end
        end
    end,
    on_done = function()
        PCSX.log(string.format("[itemwin] done: step_i=%d save_stage=%d "
            .. "flow=%s", step_i, save_stage, tostring(flow_byte())))
    end,
})
