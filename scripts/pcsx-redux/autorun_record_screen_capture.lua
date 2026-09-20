-- autorun_record_screen_capture.lua
--
-- Reach menu sub-screen `0x15` - the per-character record/list screen
-- (`FUN_801DA2A0`) - the way retail does, and capture it.
--
-- `0x801D6C4C` is the only site in PROT 0899 that writes `0x15` into the
-- submenu word `DAT_801E46A4`, and it is the **row-3** arm of the root
-- command picker `FUN_801D6B20`. So the probe cancels whatever screen the
-- save state is parked on back to the root picker (`DAT_801E46A4 == 1`),
-- walks the root cursor `DAT_801E46BC & 0xFFF` to row 3, confirms, and then
-- records the screen's own state each frame: the step counter
-- `DAT_801E46AC` and the second cursor `DAT_801E46C0`, whose folded low
-- nibble is what picks which of the three lists opens.
--
-- Outputs a screenshot + save state at the character picker (step 1) and
-- again after a confirm, so the list half has a reference frame too.
--
-- Poll-only (no breakpoints) - safe to run --fast.
--
-- Usage:
--   bash scripts/pcsx-redux/run_probe.sh --fast \
--       --iso <a PPF-free copy of the disc> \
--       --scenario equip_ui_vahn_astral \
--       --lua scripts/pcsx-redux/autorun_record_screen_capture.lua \
--       --frames 1800

package.path = package.path .. ";scripts/pcsx-redux/lib/?.lua"
local probe = require("probe")
local sstate = require("probe.sstate")

local SSTATE = probe.getenv("LEGAIA_SSTATE", "")
local FRAMES = probe.getenv_num("LEGAIA_FRAMES", 1800)

local SUB     = 0x801E46A4 -- requested sub-screen id
local STEP    = 0x801E46AC -- shared per-submenu phase / step counter
local ROOT    = 0x801E46BC -- root picker cursor
local CURSOR2 = 0x801E46C0 -- the record screen's second cursor
local PICK    = 0x801E46C4 -- its character picker cursor

local TARGET_ROW = 3 -- the root picker row that routes to sub-screen 0x15

local lines = {}
local function logf(fmt, ...)
    local s = string.format(fmt, ...)
    lines[#lines + 1] = s
    PCSX.log("[recscreen] " .. s)
end

local function w(addr) return probe.read_u32(addr) end
local function low12(addr)
    local v = w(addr)
    if v == nil then return nil end
    return bit.band(v, 0xFFF)
end

local function screenshot(stem)
    local ok, ss = pcall(PCSX.GPU.takeScreenShot)
    if not ok or ss == nil then return false end
    local fh = io.open(probe.out_path(stem .. ".raw"), "wb")
    if fh == nil then return false end
    fh:write(tostring(ss.data)); fh:close()
    local mh = io.open(probe.out_path(stem .. ".raw.meta"), "w")
    if mh then
        mh:write(string.format("width=%d\nheight=%d\nbpp=%d\n",
            ss.width or 320, ss.height or 240,
            (ss.bpp == "BPP_24") and 24 or 16))
        mh:close()
    end
    return true
end

local HOLD, GAP = 3, 14
local press_btn, press_at, idle_until = nil, nil, 0
local function press(btn, el) probe.pad_force(btn); press_btn, press_at = btn, el end

local state, saved, trace, last = "to_root", 0, {}, nil

probe.run({
    sstate = SSTATE,
    capture_frames = FRAMES,
    on_arm = function()
        probe.env.write_manifest("autorun_record_screen_capture.lua",
            { sstate = SSTATE, frames = FRAMES })
        return {}
    end,
    on_capture = function(ctx, el)
        local s, st = w(SUB), w(STEP)
        local key = string.format("%s/%s/%s/%s", tostring(s), tostring(st),
            tostring(low12(CURSOR2)), tostring(low12(PICK)))
        if key ~= last then
            trace[#trace + 1] = string.format(
                "%d:sub=%s step=%s cur2=%s pick=%s", el,
                s and string.format("0x%02X", s) or "nil", tostring(st),
                tostring(low12(CURSOR2)), tostring(low12(PICK)))
            last = key
        end

        if press_at ~= nil and el >= press_at + HOLD then
            probe.pad_release(press_btn); press_at = nil; idle_until = el + GAP
        end
        if el < 30 or press_at ~= nil or el < idle_until then return end

        if state == "to_root" then
            if s == 1 then
                logf("f=%d at the root picker (row %s)", el, tostring(low12(ROOT)))
                state = "seek"
            else
                press(probe.BTN.CIRCLE, el)
            end
            return
        end

        if state == "seek" then
            if s ~= 1 then return end
            local r = low12(ROOT)
            if r == nil then return end
            if r < TARGET_ROW then press(probe.BTN.DOWN, el)
            elseif r > TARGET_ROW then press(probe.BTN.UP, el)
            else
                logf("f=%d confirm root row %d", el, TARGET_ROW)
                press(probe.BTN.CROSS, el)
                state = "wait15"
            end
            return
        end

        if state == "wait15" then
            if s ~= 0x15 then return end
            if saved == 0 then
                sstate.save(probe.out_path("record_screen_picker.sstate"))
                logf("f=%d sub 0x15 reached: step=%s cur2=%s shot=%s", el,
                    tostring(st), tostring(low12(CURSOR2)),
                    tostring(screenshot("record_screen_picker")))
                saved = 1
                press(probe.BTN.CROSS, el)
            elseif saved == 1 then
                sstate.save(probe.out_path("record_screen_after_confirm.sstate"))
                logf("f=%d after confirm: step=%s cur2=%s shot=%s", el,
                    tostring(st), tostring(low12(CURSOR2)),
                    tostring(screenshot("record_screen_after_confirm")))
                saved = 2
                ctx.request_quit = true
            end
            return
        end
    end,
    on_summary = function()
        logf("--- record screen capture --- state=%s saved=%d", state, saved)
        logf("trace: %s", table.concat(trace, " | "))
        local fh = io.open(probe.out_path("record_screen.log"), "w")
        if fh then fh:write(table.concat(lines, "\n") .. "\n"); fh:close() end
    end,
})
