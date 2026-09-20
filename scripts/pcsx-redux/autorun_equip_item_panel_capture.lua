-- autorun_equip_item_panel_capture.lua
--
-- Drive retail's Equip screen past slot-pick into the candidate list and
-- capture the **item-info panel** (window 24) on each of the seven browse
-- rows - weapon, helmet, body, footwear, Goods x3.
--
-- Every library state parked on the Equip screen sits at slot-pick with the
-- panel blank, so the port's window-24 panel had no reference frame. The
-- screen is three sub-screens of the `0x801E4F40` table, selected by
-- `DAT_801E46A4`: `0x12` picks the character, `0x13` browses the rows,
-- `0x14` drives the candidate list. The browse row is `DAT_801E46C0 & 0xFFF`
-- (row 0 = "Best Equipment", rows 1..7 = the slot rows), and the hovered
-- candidate's id lands in `DAT_801E46B0`, which is what makes the panel draw.
--
-- Per row the probe seeks the cursor, confirms into `0x14`, waits for a
-- staged id, then writes a screenshot (`.raw` + `.raw.meta`, decode with
-- scripts/pcsx-redux/decode_pcsx_screen.py) and a save state (which carries
-- the VRAM), and cancels back to `0x13`.
--
-- Poll-only (no breakpoints) - safe to run --fast.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--       --iso <a PPF-free copy of the disc> \
--       --scenario equip_ui_vahn_astral \
--       --lua scripts/pcsx-redux/autorun_equip_item_panel_capture.lua \
--       --frames 2400
--
-- Env vars:
--   LEGAIA_SSTATE   save state (run_probe.sh --scenario <label>)
--   LEGAIA_FRAMES   capture vsyncs (default 2400)
--   LEGAIA_ROWS     highest browse row to visit (default 7)
--
-- Outputs: equip_panel_row<N>.raw/.raw.meta/.sstate, equip_panel.log

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local sstate = require("probe.sstate")

local SSTATE   = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES   = probe.getenv_num("LEGAIA_FRAMES", 2400)
local LAST_ROW = probe.getenv_num("LEGAIA_ROWS", 7)

local SUB      = 0x801E46A4  -- requested sub-screen id
local SUB_SET  = 0x801E46A8  -- settled sub-screen id
local BROWSE   = 0x801E46C0  -- slot-browse cursor (low 12 bits = row)
local STAGED   = 0x801E46B0  -- hovered candidate item id
local LISTMODE = 0x8007BB94  -- kind-4 list kernel mode

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[equippanel] " .. s)
end

local function sub()    return probe.read_u32(SUB) end
local function row()
    local v = probe.read_u32(BROWSE)
    if v == nil then return nil end
    return bit.band(v, 0xFFF)
end
local function staged() return probe.read_u32(STAGED) end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return false end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data))
    fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
    return true
end

-- Press bookkeeping: hold for HOLD vsyncs, then wait GAP before the next.
local HOLD, GAP = 3, 14
local press_btn, press_at, idle_until = nil, nil, 0

local function press(btn, el)
    probe.pad_force(btn)
    press_btn, press_at = btn, el
end

local state = "to_browse"
local target = 1
local settle_at = nil
local captured = {}
local trace = {}
local last_key = nil

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,

    on_capture = function(ctx, el)
        local s, r, st = sub(), row(), staged()
        local key = string.format("%s/%s/%s", tostring(s), tostring(r),
            tostring(st))
        if key ~= last_key then
            trace[#trace + 1] = string.format("%d:sub=%s row=%s staged=%s",
                el, s and string.format("0x%02X", s) or "nil",
                tostring(r), tostring(st))
            last_key = key
        end

        if press_at ~= nil and el >= press_at + HOLD then
            probe.pad_release(press_btn)
            press_at = nil
            idle_until = el + GAP
        end
        if el < 30 or press_at ~= nil or el < idle_until then return end

        if state == "to_browse" then
            if s == 0x13 then
                logf("f=%d at slot browse (row %s)", el, tostring(r))
                state = "seek"
            elseif s == 0x12 then
                press(probe.BTN.CROSS, el)
            elseif s == 0x14 then
                press(probe.BTN.CIRCLE, el)
            else
                press(probe.BTN.CROSS, el)
            end
            return
        end

        if state == "seek" then
            if s ~= 0x13 then return end
            if r == nil then return end
            if r < target then
                press(probe.BTN.DOWN, el)
            elseif r > target then
                press(probe.BTN.UP, el)
            else
                logf("f=%d confirm row %d", el, target)
                press(probe.BTN.CROSS, el)
                state = "enter"
                settle_at = nil
            end
            return
        end

        if state == "enter" then
            if s ~= 0x14 then
                -- Retail refused the row (or the list is empty): record it
                -- and move on rather than stalling the ladder.
                if settle_at == nil then settle_at = el end
                if el >= settle_at + 90 then
                    logf("row %d: never reached sub 0x14 (sub=%s) - skipping",
                        target, s and string.format("0x%02X", s) or "nil")
                    captured[target] = "no-list"
                    target = target + 1
                    state = (target > LAST_ROW) and "done" or "to_browse"
                    settle_at = nil
                end
                return
            end
            if settle_at == nil then settle_at = el end
            if el >= settle_at + 45 then
                local stem = string.format("equip_panel_row%d", target)
                local okshot = screenshot(stem)
                sstate.save(probe.out_path(stem .. ".sstate"))
                logf("row %d captured at f=%d: staged=%s listmode=%s shot=%s",
                    target, el, tostring(st),
                    tostring(probe.read_u32(LISTMODE)), tostring(okshot))
                captured[target] = string.format("staged=%s", tostring(st))
                target = target + 1
                state = (target > LAST_ROW) and "done" or "leave"
                settle_at = nil
                press(probe.BTN.CIRCLE, el)
            end
            return
        end

        if state == "leave" then
            if s == 0x13 then
                state = "seek"
            else
                press(probe.BTN.CIRCLE, el)
            end
            return
        end

        if state == "done" then
            ctx.request_quit = true
        end
    end,

    on_arm = function()
        probe.env.write_manifest("autorun_equip_item_panel_capture.lua", {
            sstate = SSTATE, frames = FRAMES, rows = LAST_ROW,
        })
        return {}
    end,

    on_summary = function()
        logf("--- equip item-info panel capture ---")
        logf("state=%s target=%d", state, target)
        for i = 1, LAST_ROW do
            logf("row %d: %s", i, tostring(captured[i] or "MISSED"))
        end
        logf("trace: %s", table.concat(trace, " | "))
        local fh = io.open(probe.out_path("equip_panel.log"), "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    end,
})
